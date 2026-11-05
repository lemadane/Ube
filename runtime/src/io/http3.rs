//! HTTP/3 over QUIC, sharing the TCP transports' route and session context.
use super::{dispatch_request, ServerContext};
use bytes::{Buf, Bytes};
use futures_util::{stream::FuturesUnordered, StreamExt};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
const TIMEOUT: Duration = Duration::from_secs(10);
fn error(value: impl std::fmt::Display) -> io::Error {
    io::Error::other(value.to_string())
}

pub(super) fn endpoint(
    socket: std::net::UdpSocket,
    tls: Arc<rustls::ServerConfig>,
) -> io::Result<quinn::Endpoint> {
    let mut tls = (*tls).clone();
    tls.alpn_protocols = vec![b"h3".to_vec()];
    // No replayable early data is delivered to application callbacks.
    tls.max_early_data_size = 0;
    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls).map_err(error)?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(100u32.into());
    transport.datagram_receive_buffer_size(None);
    transport.datagram_send_buffer_size(0);
    transport.max_concurrent_uni_streams(16u32.into());
    transport.max_idle_timeout(Some(Duration::from_secs(30).try_into().map_err(error)?));
    config.transport_config(Arc::new(transport));
    quinn::Endpoint::new(
        Default::default(),
        Some(config),
        socket,
        Arc::new(quinn::TokioRuntime),
    )
}

pub(super) async fn serve_connection(
    incoming: quinn::Incoming,
    context: &ServerContext<'_>,
    running: &AtomicBool,
) -> io::Result<()> {
    let quic = tokio::time::timeout(TIMEOUT, incoming)
        .await
        .map_err(error)?
        .map_err(error)?;
    let mut connection = h3::server::builder()
        .max_field_section_size(16 * 1024)
        .build(h3_quinn::Connection::new(quic.clone()))
        .await
        .map_err(error)?;
    let mut requests = FuturesUnordered::new();
    let mut stopped = tokio::time::interval(Duration::from_millis(20));
    loop {
        tokio::select! {
            accepted = connection.accept() => match accepted {
                Ok(Some(resolver)) => requests.push(serve_request(resolver, context)),
                Ok(None) => break,
                Err(failure) if failure.is_h3_no_error() => break,
                Err(failure) => return Err(error(failure)),
            },
            Some(result) = requests.next(), if !requests.is_empty() => {
                if let Err(failure) = result { eprintln!("Ube HTTP/3 request error: {failure}"); }
            },
            _ = stopped.tick() => if !running.load(Ordering::Acquire) {
                let _ = connection.shutdown(0).await;
                let _ = tokio::time::timeout(TIMEOUT, async {
                    while !requests.is_empty() {
                        tokio::select! {
                            _ = requests.next() => {},
                            _ = connection.accept() => {},
                        }
                    }
                    // FIN is queued, not necessarily acknowledged. Keep the
                    // transport alive so the final response reaches the peer.
                    quic.closed().await;
                }).await;
                break;
            }
        }
    }
    Ok(())
}

async fn serve_request(
    resolver: h3::server::RequestResolver<h3_quinn::Connection, Bytes>,
    context: &ServerContext<'_>,
) -> io::Result<()> {
    let (request, stream) = tokio::time::timeout(TIMEOUT, resolver.resolve_request())
        .await
        .map_err(error)?
        .map_err(error)?;
    let (mut send, mut recv) = stream.split();
    let payload = tokio::time::timeout(TIMEOUT, async {
        let length = match super::http::multiplexed_body_length(request.headers()) {
            Err(400) if !request.headers().contains_key(http::header::TRAILER) => {
                send.stop_stream(h3::error::Code::H3_MESSAGE_ERROR);
                recv.stop_sending(h3::error::Code::H3_MESSAGE_ERROR);
                return Err(400);
            }
            result => result?,
        };
        if super::http::expects_continue(request.headers())? {
            send.send_response(http::Response::builder().status(100).body(()).unwrap())
                .await
                .map_err(|_| 400u16)?;
        }
        let mut body = Vec::new();
        while let Some(mut data) = recv.recv_data().await.map_err(|_| 400u16)? {
            if data.remaining() > super::http::MAX_BODY - body.len() {
                return Err(413);
            }
            while data.has_remaining() {
                let chunk = data.chunk();
                body.extend_from_slice(chunk);
                let count = chunk.len();
                data.advance(count);
            }
        }
        if recv
            .recv_trailers()
            .await
            .map_err(|_| 400u16)?
            .is_some_and(|trailers| !trailers.is_empty())
        {
            return Err(400);
        }
        if length.is_some_and(|length| length != body.len()) {
            send.stop_stream(h3::error::Code::H3_MESSAGE_ERROR);
            recv.stop_sending(h3::error::Code::H3_MESSAGE_ERROR);
            return Err(400);
        }
        Ok(body)
    })
    .await
    .unwrap_or(Err(408));
    tokio::time::timeout(TIMEOUT, async {
        let head = request.method() == http::Method::HEAD;
        let cookies = request
            .headers()
            .get_all(http::header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect::<Vec<_>>()
            .join("; ");
        let headers = request
            .headers()
            .iter()
            .map(|(name, value)| {
                value
                    .to_str()
                    .map(|value| (name.as_str().to_owned(), value.to_owned()))
            })
            .collect::<Result<Vec<_>, _>>();
        let (status, body, cookie, location) = match (headers, payload) {
            (Ok(headers), Ok(payload)) => dispatch_request(
                request.method().as_str(),
                request
                    .uri()
                    .path_and_query()
                    .map(|p| p.as_str())
                    .unwrap_or("/"),
                "HTTP/3",
                context,
                &cookies,
                headers,
                Some(payload),
            ),
            (_, Err(status)) => (status, Vec::new(), None, None),
            _ => (400, b"Bad Request".to_vec(), None, None),
        };
        let no_body = head || status == 204 || status == 304;
        let mut response = http::Response::builder()
            .status(status)
            .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8");
        if status != 204 {
            response = response.header(http::header::CONTENT_LENGTH, body.len());
        }
        if let Some(value) = cookie {
            response = response.header(http::header::SET_COOKIE, value);
        }
        if let Some(value) = location {
            response = response.header(http::header::LOCATION, value);
        }
        if let Some(value) = &context.alt_svc {
            response = response.header(http::header::ALT_SVC, value);
        }
        let response = response.body(()).map_err(error)?;
        // Generated callbacks and this future remain on the server thread.
        {
            send.send_response(response).await.map_err(error)?;
            if !no_body {
                let mut body = Bytes::from(body);
                while body.has_remaining() {
                    let count = body.remaining().min(16 * 1024);
                    send.send_data(body.split_to(count)).await.map_err(error)?;
                }
            }
            send.finish().await.map_err(error)?;
        }
        Ok::<_, io::Error>(())
    })
    .await
    .map_err(error)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::{UbeRequest, UbeResponse, UbeServerRoute};
    use std::{
        ffi::{CStr, CString},
        net::{SocketAddr, TcpListener, UdpSocket},
        thread,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    type Sender = h3::client::SendRequest<h3_quinn::OpenStreams, Bytes>;
    struct Server {
        tcp: SocketAddr,
        udp: SocketAddr,
        cert: rustls::pki_types::CertificateDer<'static>,
        running: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<io::Result<()>>>,
    }
    impl Server {
        fn new() -> Self {
            let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
            let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.cert.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()).into(),
            )
            .unwrap();
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            listener.set_nonblocking(true).unwrap();
            let udp = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
            udp.set_nonblocking(true).unwrap();
            let tcp = listener.local_addr().unwrap();
            let address = udp.local_addr().unwrap();
            let running = Arc::new(AtomicBool::new(true));
            let state = running.clone();
            let worker = thread::spawn(move || {
                let routes = [
                    ("GET", "/protocol"),
                    ("GET", "/large"),
                    ("POST", "/large"),
                    ("HEAD", "/large"),
                    ("GET", "/set"),
                    ("GET", "/get"),
                    ("GET", "/redirect"),
                    ("GET", "/empty"),
                    ("GET", "/stop"),
                ]
                .into_iter()
                .map(|(method, path)| UbeServerRoute {
                    method: method.into(),
                    path: path.into(),
                    context: Arc::as_ptr(&state) as usize,
                    handler: callback as *const () as usize,
                })
                .collect::<Vec<_>>();
                crate::io::http::serve_listener(
                    listener,
                    &routes,
                    Some(Arc::new(tls)),
                    true,
                    &state,
                    Default::default(),
                    Some(udp),
                )
            });
            Self {
                tcp,
                udp: address,
                cert: cert.cert.der().clone(),
                running,
                worker: Some(worker),
            }
        }
        fn tls(&self, alpn: &[u8], trusted: bool) -> rustls::ClientConfig {
            let mut roots = rustls::RootCertStore::empty();
            if trusted {
                roots.add(self.cert.clone()).unwrap();
            }
            let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
            config.alpn_protocols = vec![alpn.to_vec()];
            config
        }
        fn endpoint(&self, alpn: &[u8], trusted: bool) -> quinn::Endpoint {
            let config =
                quinn::crypto::rustls::QuicClientConfig::try_from(self.tls(alpn, trusted)).unwrap();
            let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
            endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(config)));
            endpoint
        }
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.running.store(false, Ordering::Release);
            self.worker.take().unwrap().join().unwrap().unwrap();
        }
    }
    unsafe extern "C" fn callback(
        context: *mut std::ffi::c_void,
        request: *mut UbeRequest,
    ) -> *mut UbeResponse {
        let path = unsafe { CStr::from_ptr((*request).url) }.to_str().unwrap();
        let (status, body) = match path {
            "/large" => (200, "x".repeat(256 * 1024)),
            "/set" => {
                let session = crate::io::ube_request_session(request);
                crate::sessions::ube_session_set(session, c"user".as_ptr(), c"123".as_ptr());
                (200, "set".into())
            }
            "/get" => {
                let session = crate::io::ube_request_session(request);
                let value = crate::sessions::ube_session_get(session, c"user".as_ptr());
                (
                    200,
                    if value.is_null() {
                        "anonymous".into()
                    } else {
                        unsafe { CStr::from_ptr(value) }.to_str().unwrap().into()
                    },
                )
            }
            "/redirect" => return crate::io::ube_response_redirect(c"/protocol".as_ptr()),
            "/empty" => (204, String::new()),
            "/stop" => {
                unsafe { &*(context as *const AtomicBool) }.store(false, Ordering::Release);
                (200, "stopped".into())
            }
            _ => {
                let protocol =
                    unsafe { CString::from_raw(crate::io::ube_request_protocol(request)) };
                (
                    200,
                    format!(
                        "{} {}",
                        protocol.to_str().unwrap(),
                        crate::io::ube_request_is_http3(request)
                    ),
                )
            }
        };
        crate::io::ube_response_create(status, CString::new(body).unwrap().as_ptr())
    }
    async fn request(
        mut sender: Sender,
        method: &str,
        path: &str,
        cookie: Option<&str>,
    ) -> (http::Response<()>, Vec<u8>) {
        let mut request = http::Request::builder()
            .method(method)
            .uri(format!("https://localhost{path}"));
        if let Some(cookie) = cookie {
            request = request.header(http::header::COOKIE, cookie);
        }
        let stream = sender
            .send_request(request.body(()).unwrap())
            .await
            .unwrap();
        let (mut send, mut recv) = stream.split();
        let (_, reply) = tokio::join!(
            async {
                if method == "POST" {
                    send.send_data(Bytes::from(vec![b'y'; 256 * 1024]))
                        .await
                        .unwrap();
                }
                send.finish().await.unwrap();
            },
            async {
                let response = recv.recv_response().await.unwrap();
                let mut bytes = Vec::new();
                while let Some(mut data) = recv.recv_data().await.unwrap() {
                    while data.has_remaining() {
                        let part = data.chunk();
                        bytes.extend_from_slice(part);
                        let count = part.len();
                        data.advance(count);
                    }
                }
                (response, bytes)
            }
        );
        reply
    }
    #[test]
    fn quic_routes_streams_sessions_fallback_and_shutdown() {
        let server = Server::new();
        crate::net::get_runtime().block_on(async {
            tokio::time::timeout(Duration::from_secs(15), async {
                let endpoint = server.endpoint(b"h3", true);
                let quic = endpoint
                    .connect(server.udp, "localhost")
                    .unwrap()
                    .await
                    .unwrap();
                let (mut driver, sender) = h3::client::new(h3_quinn::Connection::new(quic.clone()))
                    .await
                    .unwrap();
                let driver = tokio::spawn(async move { driver.wait_idle().await });
                let replies = futures_util::future::join_all(
                    [
                        ("GET", "/large"),
                        ("GET", "/protocol"),
                        ("POST", "/large"),
                        ("HEAD", "/large"),
                        ("GET", "/missing"),
                        ("GET", "/empty"),
                    ]
                    .into_iter()
                    .map(|(method, path)| request(sender.clone(), method, path, None)),
                )
                .await;
                assert_eq!(replies[0].1, vec![b'x'; 256 * 1024]);
                assert_eq!(replies[1].1, b"HTTP/3 true");
                assert_eq!(replies[1].0.version(), http::Version::HTTP_3);
                assert_eq!(replies[2].1, vec![b'x'; 256 * 1024]);
                assert!(replies[3].1.is_empty());
                assert_eq!(replies[4].0.status().as_u16(), 404);
                assert_eq!(replies[5].0.status().as_u16(), 204);
                assert!(replies[5].1.is_empty());
                let (response, _) = request(sender.clone(), "GET", "/set", None).await;
                let cookie = response.headers()[http::header::SET_COOKIE]
                    .to_str()
                    .unwrap();
                assert!(cookie.contains("; Secure"));
                let cookie = cookie.split(';').next().unwrap();
                assert_eq!(
                    request(sender.clone(), "GET", "/get", Some(cookie)).await.1,
                    b"123"
                );
                let response = request(sender.clone(), "GET", "/redirect", None).await.0;
                assert_eq!(response.status().as_u16(), 302);
                assert_eq!(response.headers()[http::header::LOCATION], "/protocol");
                // HTTP/1.1 over TLS remains usable while QUIC stays open, and
                // shares the same session store and advertised UDP port.
                let stream = tokio::net::TcpStream::connect(server.tcp).await.unwrap();
                let mut tls =
                    tokio_rustls::TlsConnector::from(Arc::new(server.tls(b"http/1.1", true)))
                        .connect("localhost".try_into().unwrap(), stream)
                        .await
                        .unwrap();
                tls.write_all(
                    format!("GET /get HTTP/1.1\r\nHost: localhost\r\nCookie: {cookie}\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
                let mut bytes = Vec::new();
                tls.read_to_end(&mut bytes).await.unwrap();
                let text = String::from_utf8(bytes).unwrap();
                assert!(text.ends_with("123"));
                assert!(text.contains(&format!("Alt-Svc: h3=\":{}\"", server.udp.port())));
                assert_eq!(
                    request(sender.clone(), "GET", "/stop", None).await.1,
                    b"stopped"
                );
                quic.close(quinn::VarInt::from_u32(0x100), b"done");
                drop(sender);
                let _ = driver.await;
                endpoint.close(quinn::VarInt::from_u32(0x100), b"done");
                endpoint.wait_idle().await;
            })
            .await
            .unwrap();
        });
    }
    #[test]
    fn quic_rejects_untrusted_certificates_and_wrong_alpn() {
        let server = Server::new();
        crate::net::get_runtime().block_on(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                for (alpn, trusted) in [(b"h3".as_slice(), false), (b"h2".as_slice(), true)] {
                    let endpoint = server.endpoint(alpn, trusted);
                    assert!(endpoint
                        .connect(server.udp, "localhost")
                        .unwrap()
                        .await
                        .is_err());
                    endpoint.close(0u32.into(), b"done");
                    endpoint.wait_idle().await;
                }
            })
            .await
            .unwrap();
        });
    }
}
