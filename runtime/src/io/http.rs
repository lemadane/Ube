//! HTTP transports for the shared Ube route callback ABI.
use super::{dispatch_request, ServerContext, UbeServerRoute};
use bytes::{Buf, Bytes};
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use std::{
    io,
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
const IO_TIMEOUT: Duration = Duration::from_secs(10);

fn error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

pub(super) fn tls_config(cert: &str, key: &str) -> io::Result<rustls::ServerConfig> {
    let certs = CertificateDer::pem_file_iter(cert)
        .map_err(error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(error)?;
    let key = PrivateKeyDer::from_pem_file(key).map_err(error)?;
    rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(error)?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(error)
}

pub(super) fn serve_listener(
    listener: TcpListener,
    routes: &[UbeServerRoute],
    tls: Option<Arc<rustls::ServerConfig>>,
    http2: bool,
    running: &AtomicBool,
    sessions: crate::sessions::Store,
    udp: Option<std::net::UdpSocket>,
) -> io::Result<()> {
    use futures_util::{stream::FuturesUnordered, StreamExt};
    let alt_svc = udp
        .as_ref()
        .map(|socket| {
            socket
                .local_addr()
                .map(|address| format!("h3=\":{}\"; ma=3600", address.port()))
        })
        .transpose()?;
    let context = ServerContext {
        routes,
        sessions,
        secure: tls.is_some(),
        alt_svc,
    };
    crate::net::get_runtime().block_on(async {
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let endpoint = udp.map(|socket| super::http3::endpoint(socket, tls.clone().ok_or_else(|| error("HTTP/3 requires TLS"))?)).transpose()?;
        let mut quic_connections = FuturesUnordered::new();
        let mut connections = FuturesUnordered::new();
        let mut stopped = tokio::time::interval(Duration::from_millis(20));
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (stream, _) = accepted?;
                    connections.push(serve_connection(stream, &context, tls.clone(), http2, running));
                },
                incoming = async { endpoint.as_ref().unwrap().accept().await }, if endpoint.is_some() => {
                    if let Some(incoming) = incoming { quic_connections.push(super::http3::serve_connection(incoming, &context, running)); }
                },
                Some(result) = quic_connections.next(), if !quic_connections.is_empty() => {
                    if let Err(error) = result { eprintln!("Ube HTTP/3 connection error: {error}"); }
                },
                Some(result) = connections.next(), if !connections.is_empty() => {
                    if let Err(error) = result { eprintln!("Ube server request error: {error}"); }
                },
                _ = stopped.tick() => if !running.load(Ordering::Acquire) { break; }
            }
        }
        // Allow queued responses to finish after a handler calls server.stop().
        let _ = tokio::time::timeout(IO_TIMEOUT, async {
            tokio::join!(async { while connections.next().await.is_some() {} }, async { while quic_connections.next().await.is_some() {} });
        }).await;
        if let Some(endpoint) = endpoint {
            endpoint.close(quinn::VarInt::from_u32(0x100), b"server stopped");
            let _ = tokio::time::timeout(Duration::from_secs(1), endpoint.wait_idle()).await;
        }
        Ok(())
    })
}

async fn serve_connection(
    stream: tokio::net::TcpStream,
    context: &ServerContext<'_>,
    tls: Option<Arc<rustls::ServerConfig>>,
    http2: bool,
    running: &AtomicBool,
) -> io::Result<()> {
    if let Some(config) = tls {
        let mut config = (*config).clone();
        config.alpn_protocols = if http2 {
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        } else {
            vec![b"http/1.1".to_vec()]
        };
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let stream = tokio::time::timeout(IO_TIMEOUT, acceptor.accept(stream))
            .await
            .map_err(error)??;
        if stream.get_ref().1.alpn_protocol() == Some(b"h2") {
            serve_h2(stream, context, running).await
        } else {
            serve_h1(stream, context).await
        }
    } else if http2
        && tokio::time::timeout(IO_TIMEOUT, is_h2(&stream))
            .await
            .map_err(error)??
    {
        serve_h2(stream, context, running).await
    } else {
        serve_h1(stream, context).await
    }
}

// Peek rather than consume the preface: h2 needs the complete original bytes.
// Prefix matching also handles a preface split across multiple TCP packets.
async fn is_h2(stream: &tokio::net::TcpStream) -> io::Result<bool> {
    let mut bytes = [0u8; 24];
    loop {
        let count = stream.peek(&mut bytes).await?;
        if count == 0 || bytes[..count] != PREFACE[..count] {
            return Ok(false);
        }
        if count == PREFACE.len() {
            return Ok(true);
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

// Limits apply to decoded payload bytes and separately to framing metadata.
pub(super) const MAX_BODY: usize = 1024 * 1024;
const MAX_HEAD: usize = 16 * 1024;
const MAX_CHUNK_METADATA: usize = 64 * 1024;

#[derive(Debug)]
struct H1Request {
    method: String,
    url: String,
    protocol: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

async fn h1_line<S: AsyncRead + Unpin>(
    reader: &mut S,
    limit: usize,
    oversized: u16,
) -> Result<Vec<u8>, u16> {
    let mut line = Vec::new();
    loop {
        if line.len() >= limit {
            return Err(oversized);
        }
        let byte = reader.read_u8().await.map_err(|_| 400u16)?;
        line.push(byte);
        if byte == b'\n' {
            if !line.ends_with(b"\r\n") {
                return Err(400);
            }
            line.truncate(line.len() - 2);
            return Ok(line);
        }
        if line.len() > 1 && line[line.len() - 2] == b'\r' {
            return Err(400);
        }
    }
}

async fn read_h1_request<S: AsyncRead + AsyncWrite + Unpin>(
    reader: &mut S,
) -> Result<H1Request, u16> {
    let mut head = Vec::new();
    loop {
        let line = h1_line(reader, MAX_HEAD.saturating_sub(head.len()), 431).await?;
        let finished = line.is_empty();
        head.extend_from_slice(&line);
        head.extend_from_slice(b"\r\n");
        if finished {
            break;
        }
    }
    let mut slots = [httparse::EMPTY_HEADER; 128];
    let mut parsed = httparse::Request::new(&mut slots);
    match parsed.parse(&head) {
        Ok(httparse::Status::Complete(n)) if n == head.len() => {}
        Err(httparse::Error::TooManyHeaders) => return Err(431),
        _ => return Err(400),
    }
    let protocol = match parsed.version {
        Some(0) => "HTTP/1.0",
        Some(1) => "HTTP/1.1",
        _ => return Err(400),
    };
    let method = parsed.method.ok_or(400u16)?.to_owned();
    let url = parsed.path.ok_or(400u16)?.to_owned();
    let mut headers = Vec::new();
    for header in parsed.headers.iter() {
        if !header
            .value
            .iter()
            .all(|b| *b == b'\t' || (32..=126).contains(b))
        {
            return Err(400);
        }
        headers.push((
            header.name.to_owned(),
            std::str::from_utf8(header.value)
                .map_err(|_| 400u16)?
                .trim_matches([' ', '\t'])
                .to_owned(),
        ));
    }
    let single = |name: &str| -> Result<Option<&str>, u16> {
        let mut values = headers
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case(name));
        let first = values.next().map(|(_, value)| value.as_str());
        if values.next().is_some() {
            return Err(400);
        }
        Ok(first)
    };
    let length = single("content-length")?;
    let transfer = single("transfer-encoding")?;
    if length.is_some() && transfer.is_some() {
        return Err(400);
    }
    let length = match length {
        Some(value) => {
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(400);
            }
            let n = value.parse::<usize>().map_err(|_| 413u16)?;
            if n > MAX_BODY {
                return Err(413);
            }
            n
        }
        None => 0,
    };
    let chunked = if let Some(value) = transfer {
        // No stacked transfer codings or parameters are supported.
        if protocol != "HTTP/1.1" || !value.eq_ignore_ascii_case("chunked") {
            return Err(400);
        }
        true
    } else {
        false
    };
    if headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("trailer"))
    {
        return Err(400);
    }
    if let Some(expect) = single("expect")? {
        if protocol != "HTTP/1.1" || !expect.eq_ignore_ascii_case("100-continue") {
            return Err(417);
        }
        if chunked || length > 0 {
            reader
                .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
                .await
                .map_err(|_| 400u16)?;
            reader.flush().await.map_err(|_| 400u16)?;
        }
    }
    let mut body = Vec::new();
    if chunked {
        let mut metadata = 0;
        loop {
            let line = h1_line(reader, 8192.min(MAX_CHUNK_METADATA - metadata), 413).await?;
            metadata += line.len() + 2;
            if metadata > MAX_CHUNK_METADATA {
                return Err(413);
            }
            // Extensions and trailers are deliberately rejected rather than ignored.
            if line.is_empty() || !line.iter().all(u8::is_ascii_hexdigit) {
                return Err(400);
            }
            let size = usize::from_str_radix(std::str::from_utf8(&line).map_err(|_| 400u16)?, 16)
                .map_err(|_| 413u16)?;
            if size > MAX_BODY - body.len() {
                return Err(413);
            }
            if size == 0 {
                if !h1_line(reader, 8192.min(MAX_CHUNK_METADATA - metadata), 413)
                    .await?
                    .is_empty()
                {
                    return Err(400);
                }
                break;
            }
            let start = body.len();
            body.resize(start + size, 0);
            reader
                .read_exact(&mut body[start..])
                .await
                .map_err(|_| 400u16)?;
            let mut ending = [0; 2];
            reader.read_exact(&mut ending).await.map_err(|_| 400u16)?;
            if ending != *b"\r\n" {
                return Err(400);
            }
        }
    } else {
        body.resize(length, 0);
        reader.read_exact(&mut body).await.map_err(|_| 400u16)?;
    }
    Ok(H1Request {
        method,
        url,
        protocol,
        headers,
        body,
    })
}

async fn serve_h1<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    context: &ServerContext<'_>,
) -> io::Result<()> {
    let mut reader = BufReader::new(stream);
    let parsed = tokio::time::timeout(IO_TIMEOUT, read_h1_request(&mut reader)).await;
    let (status, body, cookie, location) = match parsed {
        Ok(Ok(request)) => {
            let cookies = request
                .headers
                .iter()
                .filter(|(name, _)| name.eq_ignore_ascii_case("cookie"))
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            dispatch_request(
                &request.method,
                &request.url,
                request.protocol,
                context,
                &cookies,
                request.headers,
                Some(request.body),
            )
        }
        result => {
            let status = match result {
                Ok(Err(status)) => status,
                _ => 408,
            };
            (status, Vec::new(), None, None)
        }
    };
    let stream = reader.get_mut();
    let reason = http::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or("Response");
    let cookie = cookie
        .map(|value| format!("Set-Cookie: {value}\r\n"))
        .unwrap_or_default();
    let location = location
        .map(|value| format!("Location: {value}\r\n"))
        .unwrap_or_default();
    let alt_svc = context
        .alt_svc
        .as_ref()
        .map(|value| format!("Alt-Svc: {value}\r\n"))
        .unwrap_or_default();
    tokio::time::timeout(IO_TIMEOUT, async {
    stream.write_all(format!("HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n{cookie}{location}{alt_svc}Connection: close\r\n\r\n", body.len()).as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.shutdown().await
    }).await.map_err(error)?
}

// HTTP/2 and HTTP/3 delimit bodies with END_STREAM, never transfer codings.
pub(super) fn multiplexed_body_length(headers: &http::HeaderMap) -> Result<Option<usize>, u16> {
    if headers.contains_key(http::header::TRANSFER_ENCODING)
        || headers.contains_key(http::header::TRAILER)
    {
        return Err(400);
    }
    let mut lengths = headers.get_all(http::header::CONTENT_LENGTH).iter();
    let length = lengths.next();
    if lengths.next().is_some() {
        return Err(400);
    }
    let Some(value) = length else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|_| 400u16)?;
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(400);
    }
    let length = value.parse::<usize>().map_err(|_| 413u16)?;
    if length > MAX_BODY {
        return Err(413);
    }
    Ok(Some(length))
}

pub(super) fn expects_continue(headers: &http::HeaderMap) -> Result<bool, u16> {
    let mut values = headers.get_all(http::header::EXPECT).iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(417);
    }
    match value {
        None => Ok(false),
        Some(value) if value.as_bytes().eq_ignore_ascii_case(b"100-continue") => Ok(true),
        _ => Err(417),
    }
}

async fn read_h2_body(
    request: &mut http::Request<h2::RecvStream>,
    respond: &mut h2::server::SendResponse<Bytes>,
) -> Result<Vec<u8>, u16> {
    let length = match multiplexed_body_length(request.headers()) {
        Err(400) if !request.headers().contains_key(http::header::TRAILER) => {
            respond.send_reset(h2::Reason::PROTOCOL_ERROR);
            return Err(400);
        }
        result => result?,
    };
    if expects_continue(request.headers())? && !request.body().is_end_stream() {
        respond
            .send_informational(http::Response::builder().status(100).body(()).unwrap())
            .map_err(|_| 400u16)?;
    }
    let incoming = request.body_mut();
    let mut body = Vec::new();
    while let Some(data) = incoming.data().await {
        let data = data.map_err(|_| 400u16)?;
        if data.len() > MAX_BODY - body.len() {
            return Err(413);
        }
        body.extend_from_slice(&data);
        incoming
            .flow_control()
            .release_capacity(data.len())
            .map_err(|_| 400u16)?;
    }
    if incoming
        .trailers()
        .await
        .map_err(|_| 400u16)?
        .is_some_and(|trailers| !trailers.is_empty())
    {
        return Err(400);
    }
    if length.is_some_and(|length| length != body.len()) {
        respond.send_reset(h2::Reason::PROTOCOL_ERROR);
        return Err(400);
    }
    Ok(body)
}

async fn serve_h2<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    context: &ServerContext<'_>,
    running: &AtomicBool,
) -> io::Result<()> {
    let mut connection = tokio::time::timeout(
        IO_TIMEOUT,
        h2::server::Builder::new()
            .max_concurrent_streams(100)
            .max_header_list_size(16 * 1024)
            .handshake::<_, Bytes>(stream),
    )
    .await
    .map_err(error)?
    .map_err(error)?;
    let mut stopped = tokio::time::interval(Duration::from_millis(20));
    let mut responses = tokio::task::JoinSet::new();
    let mut requests = futures_util::stream::FuturesUnordered::new();
    use futures_util::StreamExt;
    loop {
        tokio::select! {
            incoming = connection.accept() => match incoming {
                Some(Ok((request, respond))) => {
                    requests.push(async move {
                        let mut request = request;
                        let mut respond = respond;
                        let body = tokio::time::timeout(IO_TIMEOUT, read_h2_body(&mut request, &mut respond))
                            .await.unwrap_or(Err(408));
                        (request, respond, body)
                    });
                }
                Some(Err(failure)) => return Err(error(failure)),
                None => break,
            },
            Some((request, respond, payload)) = requests.next(), if !requests.is_empty() => {
                // Poll bodies concurrently with the connection driver. Generated
                // callbacks must execute here, on the calling server thread.
                let method = request.method().as_str();
                let url = request.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/");
                let head = method == "HEAD";
                let cookies = request.headers().get_all(http::header::COOKIE).iter()
                    .filter_map(|value| value.to_str().ok()).collect::<Vec<_>>().join("; ");
                let headers = request.headers().iter().map(|(name, value)| value.to_str().map(|value| (name.as_str().to_owned(), value.to_owned()))).collect::<Result<Vec<_>, _>>();
                let (status, body, cookie, location) = match (headers, payload) {
                    (Ok(headers), Ok(payload)) => dispatch_request(method, url, "HTTP/2", context, &cookies, headers, Some(payload)),
                    (_, Err(status)) => (status, Vec::new(), None, None),
                    _ => (400, Vec::new(), None, None),
                };
                responses.spawn(send_h2_response(request.into_body(), respond, status, body, head, cookie, location, context.alt_svc.clone()));
            },
            _ = stopped.tick() => if !running.load(Ordering::Acquire) {
                connection.graceful_shutdown();
                requests.clear();
                // Drive queued responses and GOAWAY even if the handler stopped
                // the listener. A peer cannot prevent shutdown indefinitely.
                let _ = tokio::time::timeout(IO_TIMEOUT, futures_util::future::poll_fn(|cx| connection.poll_closed(cx))).await;
                break;
            },
            Some(result) = responses.join_next(), if !responses.is_empty() => {
                match result {
                    Err(failure) => return Err(error(failure)),
                    Ok(Err(failure)) => eprintln!("Ube HTTP/2 stream error: {failure}"),
                    Ok(Ok(())) => {}
                }
            }
        }
    }
    Ok(())
}

async fn send_h2_response(
    _incoming: h2::RecvStream,
    mut respond: h2::server::SendResponse<Bytes>,
    status: u16,
    body: Vec<u8>,
    head: bool,
    cookie: Option<String>,
    location: Option<String>,
    alt_svc: Option<String>,
) -> io::Result<()> {
    let no_body = head || status == 204 || status == 304;
    let mut response = http::Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8");
    if status != 204 {
        response = response.header("content-length", body.len());
    }
    if let Some(cookie) = cookie {
        response = response.header(http::header::SET_COOKIE, cookie);
    }
    if let Some(location) = location {
        response = response.header(http::header::LOCATION, location);
    }
    if let Some(value) = alt_svc {
        response = response.header(http::header::ALT_SVC, value);
    }
    let response = response.body(()).map_err(error)?;
    let mut outgoing = respond
        .send_response(response, no_body || body.is_empty())
        .map_err(error)?;
    let send = async {
        if !no_body {
            let mut body = Bytes::from(body);
            while body.has_remaining() {
                outgoing.reserve_capacity(body.remaining().min(16 * 1024));
                let capacity = futures_util::future::poll_fn(|cx| outgoing.poll_capacity(cx))
                    .await
                    .ok_or_else(|| error("HTTP/2 stream closed"))?
                    .map_err(error)?;
                let count = capacity.min(body.remaining());
                let chunk = body.split_to(count);
                outgoing.send_data(chunk, body.is_empty()).map_err(error)?;
            }
        }
        Ok::<_, io::Error>(())
    };
    tokio::time::timeout(IO_TIMEOUT, send)
        .await
        .map_err(error)??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::{
        ube_request_is_http3, ube_request_protocol, ube_response_create, UbeRequest, UbeResponse,
    };
    use std::{
        ffi::{CStr, CString},
        net::SocketAddr,
        thread,
    };

    struct Server {
        address: SocketAddr,
        running: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<io::Result<()>>>,
    }
    impl Server {
        fn new(http2: bool, tls: Option<Arc<rustls::ServerConfig>>) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let address = listener.local_addr().unwrap();
            listener.set_nonblocking(true).unwrap();
            let running = Arc::new(AtomicBool::new(true));
            let state = running.clone();
            let thread = thread::spawn(move || {
                let routes = [
                    ("GET", "/protocol"),
                    ("GET", "/session"),
                    ("GET", "/large"),
                    ("GET", "/empty"),
                    ("HEAD", "/large"),
                    ("GET", "/stop"),
                ]
                .into_iter()
                .map(|(method, path)| UbeServerRoute {
                    method: method.into(),
                    path: path.into(),
                    context: Arc::as_ptr(&state) as usize,
                    handler: handler as *const () as usize,
                })
                .collect::<Vec<_>>();
                serve_listener(
                    listener,
                    &routes,
                    tls,
                    http2,
                    &state,
                    Default::default(),
                    None,
                )
            });
            Self {
                address,
                running,
                thread: Some(thread),
            }
        }
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.running.store(false, Ordering::Release);
            self.thread.take().unwrap().join().unwrap().unwrap();
        }
    }
    unsafe extern "C" fn handler(
        context: *mut std::ffi::c_void,
        request: *mut UbeRequest,
    ) -> *mut UbeResponse {
        let url = unsafe { CStr::from_ptr((*request).url) }.to_str().unwrap();
        let protocol = unsafe { CString::from_raw(ube_request_protocol(request)) };
        assert!(!ube_request_is_http3(request));
        let (status, body) = match url {
            "/session" => {
                let handle = crate::io::ube_request_session(request);
                crate::sessions::ube_session_set(handle, c"theme".as_ptr(), c"dark".as_ptr());
                (200, "session".into())
            }
            "/large" => (200, "x".repeat(256 * 1024)),
            "/empty" => (204, String::new()),
            "/stop" => {
                unsafe { &*(context as *const AtomicBool) }.store(false, Ordering::Release);
                (200, "stopped".into())
            }
            _ => (200, format!("{} {url}", protocol.to_str().unwrap())),
        };
        let body = CString::new(body).unwrap();
        ube_response_create(status, body.as_ptr())
    }
    async fn response(future: h2::client::ResponseFuture) -> (u16, Vec<u8>) {
        let response = future.await.unwrap();
        assert_eq!(response.version(), http::Version::HTTP_2);
        let status = response.status().as_u16();
        let mut stream = response.into_body();
        let mut bytes = Vec::new();
        while let Some(data) = stream.data().await {
            let data = data.unwrap();
            bytes.extend_from_slice(&data);
            stream.flow_control().release_capacity(data.len()).unwrap();
        }
        (status, bytes)
    }
    async fn h1<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S) -> String {
        stream
            .write_all(b"GET /protocol HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).await.unwrap();
        let response = String::from_utf8(bytes).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
        assert!(response.ends_with("HTTP/1.1 /protocol"), "{response}");
        response
    }
    #[test]
    fn h2c_multiplexing_flow_control_fallback_and_shutdown() {
        let server = Server::new(true, None);
        crate::net::get_runtime().block_on(async {
            tokio::time::timeout(Duration::from_secs(15), async {
                let stream = tokio::net::TcpStream::connect(server.address)
                    .await
                    .unwrap();
                let (mut client, connection) = h2::client::handshake(stream).await.unwrap();
                let driver = tokio::spawn(connection);
                let mut futures = Vec::new();
                for (method, path) in [
                    ("GET", "/large"),
                    ("GET", "/protocol?q=1"),
                    ("GET", "/missing"),
                    ("HEAD", "/large"),
                    ("GET", "/empty"),
                ] {
                    let request = http::Request::builder()
                        .method(method)
                        .uri(format!("http://localhost{path}"))
                        .body(())
                        .unwrap();
                    futures.push(client.send_request(request, true).unwrap().0);
                }
                let results =
                    futures_util::future::join_all(futures.into_iter().map(response)).await;
                assert_eq!(results[0], (200, vec![b'x'; 256 * 1024]));
                assert_eq!(results[1], (200, b"HTTP/2 /protocol?q=1".to_vec()));
                assert_eq!(results[2], (404, b"Not Found".to_vec()));
                assert_eq!(results[3], (200, vec![]));
                assert_eq!(results[4], (204, vec![]));
                // HTTP/1.1 works while the HTTP/2 connection stays open.
                h1(tokio::net::TcpStream::connect(server.address)
                    .await
                    .unwrap())
                .await;
                let request = http::Request::builder()
                    .uri("http://localhost/stop")
                    .body(())
                    .unwrap();
                assert_eq!(
                    response(client.send_request(request, true).unwrap().0).await,
                    (200, b"stopped".to_vec())
                );
                drop(client);
                driver.await.unwrap().unwrap();
            })
            .await
            .unwrap();
        });
    }
    #[test]
    fn tls_alpn_h2_and_http1_fallback() {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let directory = std::env::temp_dir().join(format!("ube-h2-tls-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let cert_path = directory.join("cert.pem");
        let key_path = directory.join("key.pem");
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        let other = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        std::fs::write(&key_path, other.key_pair.serialize_pem()).unwrap();
        assert!(tls_config(cert_path.to_str().unwrap(), key_path.to_str().unwrap()).is_err());
        std::fs::write(&key_path, cert.key_pair.serialize_pem()).unwrap();
        let tls = tls_config(cert_path.to_str().unwrap(), key_path.to_str().unwrap()).unwrap();
        std::fs::remove_dir_all(directory).unwrap();
        let server = Server::new(true, Some(Arc::new(tls)));
        crate::net::get_runtime().block_on(async {
            tokio::time::timeout(Duration::from_secs(15), async {
                for alpn in [Some(b"h2".as_slice()), Some(b"http/1.1".as_slice()), None] {
                    let mut roots = rustls::RootCertStore::empty();
                    roots.add(cert.cert.der().clone()).unwrap();
                    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
                        rustls::crypto::ring::default_provider(),
                    ))
                    .with_safe_default_protocol_versions()
                    .unwrap()
                    .with_root_certificates(roots)
                    .with_no_client_auth();
                    config.alpn_protocols =
                        alpn.into_iter().map(|protocol| protocol.to_vec()).collect();
                    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
                    let stream = tokio::net::TcpStream::connect(server.address)
                        .await
                        .unwrap();
                    let stream = connector
                        .connect("localhost".try_into().unwrap(), stream)
                        .await
                        .unwrap();
                    assert_eq!(stream.get_ref().1.alpn_protocol(), alpn);
                    if alpn == Some(b"h2".as_slice()) {
                        let (mut client, connection) = h2::client::handshake(stream).await.unwrap();
                        let driver = tokio::spawn(connection);
                        let request = http::Request::builder()
                            .uri("https://localhost/protocol")
                            .body(())
                            .unwrap();
                        assert_eq!(
                            response(client.send_request(request, true).unwrap().0).await,
                            (200, b"HTTP/2 /protocol".to_vec())
                        );
                        let request = http::Request::builder()
                            .uri("https://localhost/session")
                            .body(())
                            .unwrap();
                        let response = client.send_request(request, true).unwrap().0.await.unwrap();
                        let cookie = response.headers()[http::header::SET_COOKIE]
                            .to_str()
                            .unwrap();
                        assert!(cookie.contains("; Secure"));
                        assert!(cookie.contains("HttpOnly; SameSite=Lax"));
                        let mut body = response.into_body();
                        while let Some(data) = body.data().await {
                            let data = data.unwrap();
                            body.flow_control().release_capacity(data.len()).unwrap();
                        }
                        drop(body);
                        drop(client);
                        driver.await.unwrap().unwrap();
                    } else {
                        h1(stream).await;
                    }
                }
            })
            .await
            .unwrap();
        });
    }
    #[test]
    fn fragmented_preface_and_disabled_http2() {
        let enabled = Server::new(true, None);
        let disabled = Server::new(false, None);
        crate::net::get_runtime().block_on(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut stream = tokio::net::TcpStream::connect(enabled.address)
                    .await
                    .unwrap();
                for chunk in PREFACE.chunks(3) {
                    stream.write_all(chunk).await.unwrap();
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                // Empty SETTINGS frame completes the client connection preface.
                stream
                    .write_all(&[0, 0, 0, 4, 0, 0, 0, 0, 0])
                    .await
                    .unwrap();
                let mut frame = [0; 9];
                stream.read_exact(&mut frame).await.unwrap();
                assert_eq!(frame[3], 4); // Server SETTINGS, rather than HTTP/1 text.
                drop(stream);
                let mut stream = tokio::net::TcpStream::connect(disabled.address)
                    .await
                    .unwrap();
                stream.write_all(PREFACE).await.unwrap();
                let mut bytes = Vec::new();
                stream.read_to_end(&mut bytes).await.unwrap();
                assert!(bytes.starts_with(b"HTTP/1.1 400"));
                h1(tokio::net::TcpStream::connect(disabled.address)
                    .await
                    .unwrap())
                .await;
            })
            .await
            .unwrap();
        });
    }
}

#[cfg(test)]
mod body_tests {
    use super::*;

    async fn parse(raw: &[u8]) -> Result<H1Request, u16> {
        let (mut client, mut server) = tokio::io::duplex(4096);
        let (result, ()) = tokio::join!(
            async {
                let result = read_h1_request(&mut server).await;
                drop(server);
                result
            },
            async {
                let _ = client.write_all(raw).await;
                let _ = client.shutdown().await;
            }
        );
        result
    }

    #[tokio::test]
    async fn h1_reads_fixed_and_chunked_bodies_and_empty_requests() {
        let request = parse(
            b"POST /echo?q=x HTTP/1.1\r\nHost: localhost\r\nContent-Length: 7\r\n\r\n{\"a\":1}",
        )
        .await
        .unwrap();
        assert_eq!(request.body, br#"{"a":1}"#);
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, "/echo?q=x");
        let request = parse(b"PUT /echo HTTP/1.1\r\nTransfer-Encoding: ChUnKeD\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n").await.unwrap();
        assert_eq!(request.body, b"abcde");
        assert!(parse(b"POST / HTTP/1.1\r\n\r\n")
            .await
            .unwrap()
            .body
            .is_empty());
        assert!(parse(b"POST / HTTP/1.0\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap()
            .body
            .is_empty());
    }

    #[tokio::test]
    async fn h1_rejects_ambiguous_unsupported_and_truncated_framing() {
        for (headers, body, expected) in [
            ("Content-Length: 1\r\nContent-Length: 1", "x", 400),
            ("Content-Length: 1\r\ncontent-length: 2", "xx", 400),
            ("Content-Length: 1, 1", "x", 400),
            ("Content-Length: +1", "x", 400),
            ("Content-Length: -1", "", 400),
            ("Content-Length:", "", 400),
            ("Content-Length: 2", "x", 400),
            ("Content-Length: 1048577", "", 413),
            ("Content-Length: 999999999999999999999999999999999", "", 413),
            ("Content-Length: 0\r\nTransfer-Encoding: chunked", "", 400),
            ("Transfer-Encoding: gzip", "", 400),
            ("Transfer-Encoding: gzip, chunked", "", 400),
            ("Transfer-Encoding: chunked, gzip", "", 400),
            (
                "Transfer-Encoding: chunked\r\nTransfer-Encoding: chunked",
                "",
                400,
            ),
            ("Transfer-Encoding: chunked", "1\r\nx\r\n", 400),
            ("Transfer-Encoding: chunked", "1\r\n", 400),
            ("Transfer-Encoding: chunked", "1\r\nxZZ", 400),
            ("Transfer-Encoding: chunked", "Z\r\n", 400),
            (
                "Transfer-Encoding: chunked",
                "1;foo=bar\r\nx\r\n0\r\n\r\n",
                400,
            ),
            (
                "Transfer-Encoding: chunked",
                "0\r\nX-Trailer: value\r\n\r\n",
                400,
            ),
            (
                "Transfer-Encoding: chunked\r\nTrailer: X-Test",
                "0\r\n\r\n",
                400,
            ),
            ("Transfer-Encoding: chunked", "100001\r\n", 413),
            ("Expect: unsupported", "", 417),
            ("Bad Header: x", "", 400),
            ("X-Test: x\r\n folded", "", 400),
        ] {
            let raw = format!("POST / HTTP/1.1\r\n{headers}\r\n\r\n{body}");
            assert_eq!(
                parse(raw.as_bytes()).await.unwrap_err(),
                expected,
                "{headers}: {body:?}"
            );
        }
        assert_eq!(
            parse(b"POST / HTTP/1.0\r\nTransfer-Encoding: chunked\r\n\r\n")
                .await
                .unwrap_err(),
            400
        );
        assert_eq!(parse(b"POST / HTTP/1.1\n\n").await.unwrap_err(), 400);
        assert_eq!(
            parse(format!("GET /{} HTTP/1.1\r\n\r\n", "x".repeat(MAX_HEAD)).as_bytes())
                .await
                .unwrap_err(),
            431
        );
    }

    #[tokio::test]
    async fn h1_body_and_framing_limits_apply_at_boundaries() {
        let payload = "x".repeat(MAX_BODY);
        let fixed = format!("POST / HTTP/1.1\r\nContent-Length: {MAX_BODY}\r\n\r\n{payload}");
        assert_eq!(parse(fixed.as_bytes()).await.unwrap().body.len(), MAX_BODY);
        let chunked = format!("POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n{MAX_BODY:x}\r\n{payload}\r\n0\r\n\r\n");
        assert_eq!(
            parse(chunked.as_bytes()).await.unwrap().body.len(),
            MAX_BODY
        );
        let oversized = chunked.replace("\r\n0\r\n\r\n", "\r\n1\r\nx\r\n0\r\n\r\n");
        assert_eq!(parse(oversized.as_bytes()).await.unwrap_err(), 413);
        let metadata = format!(
            "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n{}0\r\n\r\n",
            "1\r\nx\r\n".repeat(MAX_CHUNK_METADATA / 3 + 1)
        );
        assert_eq!(parse(metadata.as_bytes()).await.unwrap_err(), 413);
    }

    #[tokio::test]
    async fn h1_continue_allows_fragmented_body_and_checks_limit_first() {
        let (mut client, mut server) = tokio::io::duplex(256);
        let (request, ()) = tokio::join!(read_h1_request(&mut server), async {
            client
                .write_all(b"POST / HTTP/1.1\r\nContent-Length: 4\r\nExpect: 100-continue\r\n\r\n")
                .await
                .unwrap();
            let mut interim = [0; 25];
            client.read_exact(&mut interim).await.unwrap();
            assert_eq!(&interim, b"HTTP/1.1 100 Continue\r\n\r\n");
            client.write_all(b"ab").await.unwrap();
            tokio::task::yield_now().await;
            client.write_all(b"cd").await.unwrap();
        });
        assert_eq!(request.unwrap().body, b"abcd");
        assert_eq!(
            parse(b"POST / HTTP/1.1\r\nContent-Length: 1048577\r\nExpect: 100-continue\r\n\r\n")
                .await
                .unwrap_err(),
            413
        );
    }
}

#[cfg(test)]
mod body_timeout_tests {
    use super::*;

    #[tokio::test]
    async fn incomplete_body_times_out_with_408_before_dispatch() {
        let (mut client, server) = tokio::io::duplex(1024);
        let context = ServerContext {
            routes: &[],
            sessions: Default::default(),
            secure: false,
            alt_svc: None,
        };
        let (result, response) = tokio::join!(serve_h1(server, &context), async {
            client
                .write_all(b"POST / HTTP/1.1\r\nContent-Length: 1\r\n\r\n")
                .await
                .unwrap();
            let mut response = String::new();
            client.read_to_string(&mut response).await.unwrap();
            response
        });
        result.unwrap();
        assert!(response.starts_with("HTTP/1.1 408 "), "{response}");
    }
}

#[cfg(test)]
mod multiplexed_header_tests {
    use super::*;

    #[test]
    fn validates_multiplexed_body_headers_before_allocating_or_continuing() {
        let mut headers = http::HeaderMap::new();
        assert_eq!(multiplexed_body_length(&headers), Ok(None));
        for (value, expected) in [
            ("0", Ok(Some(0))),
            ("1048576", Ok(Some(MAX_BODY))),
            ("1048577", Err(413)),
            ("+1", Err(400)),
            ("1, 1", Err(400)),
            ("", Err(400)),
        ] {
            headers.insert("content-length", value.parse().unwrap());
            assert_eq!(multiplexed_body_length(&headers), expected);
        }
        headers.insert("content-length", "0".parse().unwrap());
        headers.append("content-length", "0".parse().unwrap());
        assert_eq!(multiplexed_body_length(&headers), Err(400));
        headers.clear();
        headers.insert("transfer-encoding", "chunked".parse().unwrap());
        assert_eq!(multiplexed_body_length(&headers), Err(400));
        headers.clear();
        headers.insert("trailer", "x-checksum".parse().unwrap());
        assert_eq!(multiplexed_body_length(&headers), Err(400));
        headers.clear();
        // TE is a distinct, legal negotiation header; it does not frame a body.
        headers.insert("te", "trailers".parse().unwrap());
        assert_eq!(multiplexed_body_length(&headers), Ok(None));
        assert_eq!(expects_continue(&headers), Ok(false));
        headers.insert("expect", "100-CoNtInUe".parse().unwrap());
        assert_eq!(expects_continue(&headers), Ok(true));
        headers.append("expect", "100-continue".parse().unwrap());
        assert_eq!(expects_continue(&headers), Err(417));
        headers.insert("expect", "unsupported".parse().unwrap());
        assert_eq!(expects_continue(&headers), Err(417));
    }
}
