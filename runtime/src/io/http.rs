//! HTTP transports for the shared Ube route callback ABI.
use super::{dispatch_request, UbeServerRoute};
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
) -> io::Result<()> {
    use futures_util::{stream::FuturesUnordered, StreamExt};
    crate::net::get_runtime().block_on(async {
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let mut connections = FuturesUnordered::new();
        let mut stopped = tokio::time::interval(Duration::from_millis(20));
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (stream, _) = accepted?;
                    connections.push(serve_connection(stream, routes, tls.clone(), http2, running));
                },
                Some(result) = connections.next(), if !connections.is_empty() => {
                    if let Err(error) = result { eprintln!("Ube server request error: {error}"); }
                },
                _ = stopped.tick() => if !running.load(Ordering::Acquire) { break; }
            }
        }
        // Allow queued responses to finish after a handler calls server.stop().
        let _ = tokio::time::timeout(IO_TIMEOUT, async {
            while connections.next().await.is_some() {}
        })
        .await;
        Ok(())
    })
}

async fn serve_connection(
    stream: tokio::net::TcpStream,
    routes: &[UbeServerRoute],
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
            serve_h2(stream, routes, running).await
        } else {
            tokio::time::timeout(IO_TIMEOUT, serve_h1(stream, routes))
                .await
                .map_err(error)?
        }
    } else if http2
        && tokio::time::timeout(IO_TIMEOUT, is_h2(&stream))
            .await
            .map_err(error)??
    {
        serve_h2(stream, routes, running).await
    } else {
        tokio::time::timeout(IO_TIMEOUT, serve_h1(stream, routes))
            .await
            .map_err(error)?
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

async fn serve_h1<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    routes: &[UbeServerRoute],
) -> io::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut head = Vec::new();
    // A hard cap applies before allocating an arbitrarily long request line.
    while head.len() < 16 * 1024 {
        let byte = reader.read_u8().await?;
        head.push(byte);
        if head.ends_with(b"\r\n\r\n") || head.ends_with(b"\n\n") {
            break;
        }
    }
    let (status, body) = if head.len() == 16 * 1024 {
        (431, b"Request header too large".to_vec())
    } else if let Ok(head) = std::str::from_utf8(&head) {
        let mut parts = head.lines().next().unwrap_or("").split_whitespace();
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(method), Some(url), Some(version @ ("HTTP/1.0" | "HTTP/1.1")), None) => {
                dispatch_request(
                    method,
                    url,
                    if version == "HTTP/1.0" {
                        "HTTP/1.0"
                    } else {
                        "HTTP/1.1"
                    },
                    routes,
                )
            }
            _ => (400, b"Bad Request".to_vec()),
        }
    } else {
        (400, b"Bad Request".to_vec())
    };
    let stream = reader.get_mut();
    let reason = http::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or("Response");
    stream.write_all(format!("HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.shutdown().await
}

async fn serve_h2<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    routes: &[UbeServerRoute],
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
    loop {
        tokio::select! {
            incoming = connection.accept() => match incoming {
                Some(Ok((request, respond))) => {
                    let method = request.method().as_str();
                    let url = request.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/");
                    let head = method == "HEAD";
                    let (status, body) = dispatch_request(method, url, "HTTP/2", routes);
                    responses.spawn(send_h2_response(request.into_body(), respond, status, body, head));
                }
                Some(Err(failure)) => return Err(error(failure)),
                None => break,
            },
            _ = stopped.tick() => if !running.load(Ordering::Acquire) {
                connection.graceful_shutdown();
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
    mut incoming: h2::RecvStream,
    mut respond: h2::server::SendResponse<Bytes>,
    status: u16,
    body: Vec<u8>,
    head: bool,
) -> io::Result<()> {
    let no_body = head || status == 204 || status == 304;
    let mut response = http::Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8");
    if status != 204 {
        response = response.header("content-length", body.len());
    }
    let response = response.body(()).map_err(error)?;
    let mut outgoing = respond
        .send_response(response, no_body || body.is_empty())
        .map_err(error)?;
    // Drain unused request bodies while sending to release the receive window.
    let receive = async {
        while let Some(data) = incoming.data().await {
            let data = data.map_err(error)?;
            incoming
                .flow_control()
                .release_capacity(data.len())
                .map_err(error)?;
        }
        Ok::<_, io::Error>(())
    };
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
    tokio::try_join!(receive, send)?;
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
                serve_listener(listener, &routes, tls, http2, &state)
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
