use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use ube_compiler::{IrBuilder, Lexer, Parser, TextualLlvmEmitter, TypeChecker};

static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("ube-server-e2e-{}-{id}", std::process::id()));
        fs::create_dir(&path).expect("create isolated server E2E directory");
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct RunningServer(Child);

impl Drop for RunningServer {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn compile_ube_server(source: &str, directory: &std::path::Path) -> std::path::PathBuf {
    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(
        parser.diagnostics.is_empty(),
        "server source did not parse: {:?}",
        parser.diagnostics
    );

    let mut checker = TypeChecker::new();
    let is_valid = checker.check_program(&program);
    assert!(
        is_valid && checker.diagnostics.is_empty(),
        "server source did not type-check: {:?}",
        checker.diagnostics
    );

    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&program);
    assert!(
        builder.diagnostics.is_empty(),
        "server source did not lower to IR: {:?}",
        builder.diagnostics
    );

    let llvm_ir = TextualLlvmEmitter::new().emit_program(&ir_program);
    let ir_path = directory.join("server.ll");
    fs::write(&ir_path, llvm_ir).expect("write generated LLVM IR");

    let executable = std::env::current_exe().expect("locate integration test executable");
    let runtime_dir = executable
        .parent()
        .expect("integration test executable should have a parent directory");
    let runtime_archive = runtime_dir.join("libube_runtime.a");
    assert!(
        runtime_archive.is_file(),
        "static Ube runtime archive not found at {}",
        runtime_archive.display()
    );

    let binary_path = directory.join("server");
    let output = Command::new("clang")
        .arg(&ir_path)
        .arg("-o")
        .arg(&binary_path)
        .arg("-L")
        .arg(runtime_dir)
        .arg("-Wl,-Bstatic")
        .arg("-lube_runtime")
        .arg("-Wl,-Bdynamic")
        .arg("-lm")
        .arg("-Wno-override-module")
        .output()
        .expect("run clang to build the Ube server");
    assert!(
        output.status.success(),
        "clang failed to link the server:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    binary_path
}

fn send_request(address: SocketAddr, request: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .expect("connect to Ube server");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set HTTP response timeout");
    stream
        .write_all(request.as_bytes())
        .expect("write HTTP request");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .expect("read complete HTTP response");
    response
}

#[test]
fn compiled_ube_server_stays_running_and_serves_http_requests() {
    let port = {
        let reservation = TcpListener::bind(("127.0.0.1", 0)).expect("reserve a local port");
        reservation.local_addr().unwrap().port()
    };
    let source = format!(
        r#"
        class Application {{
            main() {{
                final server = Server()
                final res = `Hello from Ube e2e!
This is a captured multiline string.
`
                server.get('/', request -> Response.ok(res))
                server.start({port})
            }}
        }}
        "#
    );
    let directory = TestDirectory::new();
    let binary_path = compile_ube_server(&source, directory.path());
    let child = Command::new(binary_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start compiled Ube server");
    let mut server = RunningServer(child);
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        if let Some(status) = server.0.try_wait().expect("check server process") {
            panic!("server exited before accepting requests: {status}");
        }
        if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "server did not start listening within five seconds"
        );
        thread::sleep(Duration::from_millis(25));
    }

    let response = String::from_utf8(send_request(
        address,
        "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    ))
    .expect("HTTP response should be UTF-8");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    assert!(
        response.ends_with("Hello from Ube e2e!\nThis is a captured multiline string.\n"),
        "{response}"
    );

    let missing_route = String::from_utf8(send_request(
        address,
        "GET /missing HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    ))
    .expect("404 response should be UTF-8");
    assert!(
        missing_route.starts_with("HTTP/1.1 404 Not Found\r\n"),
        "{missing_route}"
    );
    assert!(
        server.0.try_wait().expect("check server process").is_none(),
        "server exited after handling HTTP requests"
    );
}

#[test]
fn compiled_ube_server_serves_http2_and_reports_actual_protocol() {
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let source = format!(
        r#"
        class Application {{
            main() {{
                final server = Server()
                server.enableHttp2()
                server.get('/protocol', request -> Response.ok('${{request.protocol()}} ${{request.isHttp3()}}'))
                server.get('/stop') {{ request ->
                    server.stop()
                    return Response.ok('stopped')
                }}
                server.start({port})
            }}
        }}
    "#
    );
    let directory = TestDirectory::new();
    let binary = compile_ube_server(&source, directory.path());
    let child = Command::new(binary)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut server = RunningServer(child);
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "server exited before listening"
        );
        if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "server startup timed out");
        thread::sleep(Duration::from_millis(25));
    }
    ube_runtime::net::get_runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(10), async {
            let stream = tokio::net::TcpStream::connect(address).await.unwrap();
            let (mut client, connection) = h2::client::handshake(stream).await.unwrap();
            let driver = tokio::spawn(connection);
            for (path, expected) in [("/protocol", "HTTP/2 false"), ("/stop", "stopped")] {
                let request = http::Request::builder()
                    .uri(format!("http://localhost{path}"))
                    .body(())
                    .unwrap();
                let response = client.send_request(request, true).unwrap().0.await.unwrap();
                assert_eq!(response.version(), http::Version::HTTP_2);
                assert_eq!(response.status(), http::StatusCode::OK);
                let mut stream = response.into_body();
                let mut body = Vec::new();
                while let Some(data) = stream.data().await {
                    let data = data.unwrap();
                    body.extend_from_slice(&data);
                    stream.flow_control().release_capacity(data.len()).unwrap();
                }
                assert_eq!(body, expected.as_bytes());
                if path == "/protocol" {
                    let response = String::from_utf8(send_request(
                        address,
                        "GET /protocol HTTP/1.1\r\nHost: localhost\r\n\r\n",
                    ))
                    .unwrap();
                    assert!(response.ends_with("HTTP/1.1 false"), "{response}");
                }
            }
            drop(client);
            driver.await.unwrap().unwrap();
        })
        .await
        .unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "server.stop() did not stop the process"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

struct SessionReply {
    status: u16,
    body: String,
    cookie: Option<String>,
    location: Option<String>,
}
fn session_http1(address: SocketAddr, path: &str, cookie: Option<&str>) -> SessionReply {
    let cookie = cookie
        .map(|value| format!("Cookie: {value}\r\n"))
        .unwrap_or_default();
    let response = String::from_utf8(send_request(
        address,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{cookie}\r\n"),
    ))
    .unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    SessionReply {
        status: head.split_whitespace().nth(1).unwrap().parse().unwrap(),
        body: body.into(),
        location: head.lines().find_map(|line| line.strip_prefix("Location: ").map(str::to_owned)),
        cookie: head
            .lines()
            .find_map(|line| line.strip_prefix("Set-Cookie: ").map(str::to_owned)),
    }
}

#[test]
fn compiled_sessions_work_across_http1_and_http2() {
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let source = format!(
        r#"
        class Application(var Session? saved) {{
            string readUser(Session session) {{ return session.get('userId') ?? 'anonymous' }}
            string checkStale(Request request) {{
                try {{
                    final value = (this._saved ?? request.session()).get('userId')
                    return 'unexpected'
                }} catch (GenericError err) {{ return 'expired handle' }}
            }}
            main() {{
                final server = Server()
                server.enableHttp2()
                server.get('/get', request -> Response.ok(this.readUser(request.session())))
                server.get('/set') {{ request ->
                    final session = request.session()
                    session.set('userId', '123')
                    this._saved = session
                    return Response.ok(request.session().get('userId') ?? 'missing')
                }}
                server.get('/rotate') {{ request ->
                    final session = request.session()
                    session.regenerate()
                    return Response.ok(session.get('userId') ?? 'anonymous')
                }}
                server.get('/remove') {{ request ->
                    request.session().remove('userId')
                    return Response.ok(request.session().get('userId') ?? 'anonymous')
                }}
                server.get('/logout') {{ request ->
                    request.session().destroy()
                    return Response.ok(request.session().get('userId') ?? 'anonymous')
                }}
                server.get('/stale', request -> Response.ok(this.checkStale(request)))
                server.start({port})
            }}
        }}
    "#
    );
    let directory = TestDirectory::new();
    let binary = compile_ube_server(&source, directory.path());
    let mut server = RunningServer(
        Command::new(binary)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(server.0.try_wait().unwrap().is_none());
        if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(25));
    }
    let empty = session_http1(address, "/get", None);
    assert_eq!(empty.status, 200);
    assert_eq!(empty.body, "anonymous");
    assert!(empty.cookie.is_none());
    let forged = format!("ube_session={}", "a".repeat(64));
    let created = session_http1(address, "/set", Some(&forged));
    assert_eq!(created.body, "123");
    let header = created.cookie.unwrap();
    assert!(header.contains("; Path=/; HttpOnly; SameSite=Lax"));
    assert!(!header.contains("; Secure"));
    let original = header.split(';').next().unwrap().to_owned();
    assert_ne!(original, forged);
    assert_eq!(original.len(), "ube_session=".len() + 64);
    assert_eq!(session_http1(address, "/get", None).body, "anonymous");
    assert_eq!(session_http1(address, "/get", Some(&original)).body, "123");
    assert_eq!(
        session_http1(address, "/stale", Some(&original)).body,
        "expired handle"
    );

    ube_runtime::net::get_runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(10), async {
            let stream = tokio::net::TcpStream::connect(address).await.unwrap();
            let (mut client, connection) = h2::client::handshake(stream).await.unwrap();
            let driver = tokio::spawn(connection);
            let mut current = original.clone();
            for (path, expected) in [
                ("/get", "123"),
                ("/rotate", "123"),
                ("/remove", "anonymous"),
                ("/set", "123"),
                ("/logout", "anonymous"),
                ("/get", "anonymous"),
            ] {
                let request = http::Request::builder()
                    .uri(format!("http://localhost{path}"))
                    .header(http::header::COOKIE, &current)
                    .body(())
                    .unwrap();
                let response = client.send_request(request, true).unwrap().0.await.unwrap();
                assert_eq!(response.status().as_u16(), 200, "{path}");
                let cookie = response
                    .headers()
                    .get(http::header::SET_COOKIE)
                    .map(|v| v.to_str().unwrap().to_owned());
                if path == "/rotate" {
                    let rotated = cookie.unwrap();
                    current = rotated.split(';').next().unwrap().to_owned();
                    assert_ne!(current, original);
                    assert_eq!(
                        session_http1(address, "/get", Some(&original)).body,
                        "anonymous"
                    );
                    assert_eq!(session_http1(address, "/get", Some(&current)).body, "123");
                } else if path == "/logout" {
                    let deleted = cookie.unwrap();
                    assert!(deleted.starts_with("ube_session=;"));
                    assert!(deleted.contains("Max-Age=0"));
                } else {
                    assert!(cookie.is_none(), "{path}: {cookie:?}");
                }
                let mut stream = response.into_body();
                let mut body = Vec::new();
                while let Some(data) = stream.data().await {
                    let data = data.unwrap();
                    body.extend_from_slice(&data);
                    stream.flow_control().release_capacity(data.len()).unwrap();
                }
                assert_eq!(String::from_utf8(body).unwrap(), expected, "{path}");
            }
            assert_eq!(
                session_http1(address, "/get", Some(&current)).body,
                "anonymous"
            );
            drop(client);
            driver.await.unwrap().unwrap();
        })
        .await
        .unwrap();
    });
}

#[test]
fn compiled_oauth_redirect_pkce_callback_and_session_work() {
    use std::io::BufRead;
    let provider = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let token_address = provider.local_addr().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let provider_thread = thread::spawn(move || {
        let (mut stream, _) = provider.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = std::io::BufReader::new(&mut stream);
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            headers.push_str(&line);
        }
        let length: usize = headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
            .unwrap()
            .1
            .trim()
            .parse()
            .unwrap();
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        drop(reader);
        let fields = url::form_urlencoded::parse(&body)
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect::<std::collections::HashMap<_, _>>();
        tx.send(fields).unwrap();
        let body = r#"{"access_token":"native-access","token_type":"Bearer","expires_in":3600}"#;
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    });
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let source = format!(
        r#"
        class Application {{
            Response callback(OAuth2 client, Request request) {{
                try {{
                    final token = client.exchange(request.session(), request.query('code'), request.query('state'))
                    try {{
                        request.session().regenerate()
                        request.session().set('accessToken', token.accessToken())
                        return Response.ok('${{token.tokenType()}} ${{token.expiresIn()}}')
                    }} finally {{ token.close() }}
                }} catch (GenericError err) {{ return Response.unauthorized('invalid callback') }}
            }}
            main() {{
                final client = OAuth2.create('https://provider.example/authorize', 'http://{token_address}/token', 'native-client', '', 'http://127.0.0.1:{port}/callback')
                final server = Server()
                server.enableHttp2()
                server.get('/login', request -> Response.redirect(client.authorize(request.session(), 'read')))
                server.get('/callback', request -> this.callback(client, request))
                server.get('/profile', request -> Response.ok(request.session().get('accessToken') ?? 'anonymous'))
                server.get('/headers') {{ request ->
                    final authorization = request.header('Authorization')
                    return Response.ok('${{authorization}} ${{request.hasBearerToken()}}')
                }}
                server.start({port})
            }}
        }}
    "#
    );
    let directory = TestDirectory::new();
    let binary = compile_ube_server(&source, directory.path());
    let mut server = RunningServer(
        Command::new(binary)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(server.0.try_wait().unwrap().is_none());
        if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(25));
    }
    let start = session_http1(address, "/login", None);
    assert_eq!(start.status, 302);
    let cookie = start.cookie.unwrap();
    let cookie = cookie.split(';').next().unwrap().to_owned();
    let authorize = url::Url::parse(start.location.as_ref().unwrap()).unwrap();
    let params = authorize
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(params["code_challenge_method"], "S256");
    assert_eq!(params["scope"], "read");
    let mut callback = url::Url::parse(&format!("http://localhost/callback")).unwrap();
    callback
        .query_pairs_mut()
        .append_pair("code", "native-code+&")
        .append_pair("state", &params["state"]);
    let callback = format!("{}?{}", callback.path(), callback.query().unwrap());
    let wrong_session = session_http1(address, &callback, None);
    assert_eq!(wrong_session.status, 401);
    assert!(wrong_session.cookie.is_none());
    let duplicate = session_http1(
        address,
        &format!("{callback}&state=duplicate"),
        Some(&cookie),
    );
    assert_eq!(duplicate.status, 401);
    ube_runtime::net::get_runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(15), async {
            let stream = tokio::net::TcpStream::connect(address).await.unwrap();
            let (mut client, connection) = h2::client::handshake(stream).await.unwrap();
            let driver = tokio::spawn(connection);
            let request = http::Request::builder()
                .uri(format!("http://localhost{callback}"))
                .header(http::header::COOKIE, &cookie)
                .body(())
                .unwrap();
            let response = client.send_request(request, true).unwrap().0.await.unwrap();
            assert_eq!(response.status().as_u16(), 200);
            let new_cookie = response.headers()[http::header::SET_COOKIE]
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_owned();
            assert_ne!(new_cookie, cookie);
            let mut stream = response.into_body();
            let mut body = Vec::new();
            while let Some(data) = stream.data().await {
                let data = data.unwrap();
                body.extend_from_slice(&data);
                stream.flow_control().release_capacity(data.len()).unwrap();
            }
            assert_eq!(body, b"Bearer 3600");
            drop(stream);
            assert_eq!(
                session_http1(address, "/profile", Some(&new_cookie)).body,
                "native-access"
            );
            assert_eq!(
                session_http1(address, "/profile", Some(&cookie)).body,
                "anonymous"
            );
            assert_eq!(
                session_http1(address, &callback, Some(&new_cookie)).status,
                401
            );
            let request = http::Request::builder()
                .uri("http://localhost/headers")
                .header(http::header::AUTHORIZATION, "Bearer native-access")
                .body(())
                .unwrap();
            let response = client.send_request(request, true).unwrap().0.await.unwrap();
            let mut stream = response.into_body();
            let mut bytes = Vec::new();
            while let Some(data) = stream.data().await {
                let data = data.unwrap();
                bytes.extend_from_slice(&data);
                stream.flow_control().release_capacity(data.len()).unwrap();
            }
            assert_eq!(bytes, b"Bearer native-access true");
            drop(stream);
            let request = http::Request::builder()
                .uri("http://localhost/login")
                .header(http::header::COOKIE, &new_cookie)
                .body(())
                .unwrap();
            let response = client.send_request(request, true).unwrap().0.await.unwrap();
            assert_eq!(response.status().as_u16(), 302);
            assert!(response.headers()[http::header::LOCATION]
                .to_str()
                .unwrap()
                .starts_with("https://provider.example/authorize?"));
            drop(response);
            let request = http::Request::builder()
                .uri("http://localhost/headers")
                .header(
                    http::header::AUTHORIZATION,
                    http::HeaderValue::from_bytes(&[0xff]).unwrap(),
                )
                .body(())
                .unwrap();
            let response = client.send_request(request, true).unwrap().0.await.unwrap();
            assert_eq!(response.status().as_u16(), 400);
            drop(response);
            drop(client);
            driver.await.unwrap().unwrap();
        })
        .await
        .unwrap();
    });
    let fields = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(fields["grant_type"], "authorization_code");
    assert_eq!(fields["client_id"], "native-client");
    assert_eq!(fields["code"], "native-code+&");
    assert_eq!(fields["code_verifier"].len(), 43);
    assert_eq!(
        fields["redirect_uri"],
        format!("http://127.0.0.1:{port}/callback")
    );
    provider_thread.join().unwrap();
}

#[test]
fn compiled_http3_server_serves_quic_with_shared_sessions_and_stops() {
    use bytes::Buf;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let directory = TestDirectory::new();
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert_file = directory.path().join("cert.pem");
    let key_file = directory.path().join("key.pem");
    fs::write(&cert_file, certificate.cert.pem()).unwrap();
    fs::write(&key_file, certificate.key_pair.serialize_pem()).unwrap();
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let source = format!(
        r#"
        class Application {{ main() {{
            final server = Server()
            server.tls('{cert}', '{key}')
            server.enableHttp2()
            server.enableHttp3(0)
            server.get('/ports', request -> Response.ok('${{server.port()}} ${{server.http3Port()}}'))
            server.get('/protocol', request -> Response.ok('${{request.protocol()}} ${{request.isHttp3()}}'))
            server.get('/set') {{ request -> request.session().set('user', '123') return Response.ok('set') }}
            server.get('/get', request -> Response.ok(request.session().get('user') ?? 'anonymous'))
            server.get('/echo', request -> Response.ok(request.query('q')))
            server.get('/redirect', request -> Response.redirect('/protocol'))
            server.get('/stop') {{ request -> server.stop() return Response.ok('stopped') }}
            server.start({port})
        }} }}
    "#,
        cert = cert_file.display(),
        key = key_file.display()
    );
    let binary = compile_ube_server(&source, directory.path());
    let mut server = RunningServer(
        Command::new(binary)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let tcp = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(server.0.try_wait().unwrap().is_none());
        if TcpStream::connect_timeout(&tcp, Duration::from_millis(100)).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(25));
    }
    ube_runtime::net::get_runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(15), async {
            let tls_config = |alpn: &[u8]| {
                let mut roots = rustls::RootCertStore::empty();
                roots.add(certificate.cert.der().clone()).unwrap();
                let mut config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
                    rustls::crypto::ring::default_provider(),
                ))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_root_certificates(roots)
                .with_no_client_auth();
                config.alpn_protocols = vec![alpn.to_vec()];
                config
            };
            let stream = tokio::net::TcpStream::connect(tcp).await.unwrap();
            let mut stream =
                tokio_rustls::TlsConnector::from(std::sync::Arc::new(tls_config(b"http/1.1")))
                    .connect("localhost".try_into().unwrap(), stream)
                    .await
                    .unwrap();
            stream
                .write_all(b"GET /ports HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).await.unwrap();
            let reply = String::from_utf8(bytes).unwrap();
            let body = reply.split_once("\r\n\r\n").unwrap().1;
            let ports = body
                .split_whitespace()
                .map(|p| p.parse::<u16>().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(ports[0], port);
            assert_ne!(ports[1], 0);
            assert!(reply.contains(&format!("Alt-Svc: h3=\":{}\"", ports[1])));
            let quic_config =
                quinn::crypto::rustls::QuicClientConfig::try_from(tls_config(b"h3")).unwrap();
            let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
            endpoint.set_default_client_config(quinn::ClientConfig::new(std::sync::Arc::new(
                quic_config,
            )));
            let quic = endpoint
                .connect(SocketAddr::from(([127, 0, 0, 1], ports[1])), "localhost")
                .unwrap()
                .await
                .unwrap();
            let (mut driver, mut sender) = h3::client::new(h3_quinn::Connection::new(quic.clone()))
                .await
                .unwrap();
            let driver = tokio::spawn(async move { driver.wait_idle().await });
            let mut cookie = None;
            for (path, expected) in [
                ("/protocol", "HTTP/3 true"),
                ("/echo?q=hello%20QUIC", "hello QUIC"),
                ("/set", "set"),
                ("/get", "123"),
                ("/redirect", ""),
                ("/stop", "stopped"),
            ] {
                let mut request = http::Request::builder().uri(format!("https://localhost{path}"));
                if let Some(cookie) = &cookie {
                    request = request.header(http::header::COOKIE, cookie);
                }
                let mut stream = sender
                    .send_request(request.body(()).unwrap())
                    .await
                    .unwrap();
                stream.finish().await.unwrap();
                let response = stream.recv_response().await.unwrap();
                assert_eq!(response.version(), http::Version::HTTP_3);
                if path == "/redirect" {
                    assert_eq!(response.status().as_u16(), 302);
                    assert_eq!(response.headers()[http::header::LOCATION], "/protocol");
                } else {
                    assert_eq!(response.status().as_u16(), 200);
                }
                if let Some(header) = response.headers().get(http::header::SET_COOKIE) {
                    let header = header.to_str().unwrap();
                    assert!(header.contains("; Secure"));
                    cookie = Some(header.split(';').next().unwrap().to_owned());
                }
                let mut body = Vec::new();
                while let Some(mut data) = stream.recv_data().await.unwrap() {
                    while data.has_remaining() {
                        let part = data.chunk();
                        body.extend_from_slice(part);
                        let count = part.len();
                        data.advance(count);
                    }
                }
                assert_eq!(body, expected.as_bytes());
                if path == "/get" {
                    let stream = tokio::net::TcpStream::connect(tcp).await.unwrap();
                    let stream =
                        tokio_rustls::TlsConnector::from(std::sync::Arc::new(tls_config(b"h2")))
                            .connect("localhost".try_into().unwrap(), stream)
                            .await
                            .unwrap();
                    let (mut client, connection) = h2::client::handshake(stream).await.unwrap();
                    let driver = tokio::spawn(connection);
                    let request = http::Request::builder()
                        .uri("https://localhost/get")
                        .header(http::header::COOKIE, cookie.as_ref().unwrap())
                        .body(())
                        .unwrap();
                    let response = client.send_request(request, true).unwrap().0.await.unwrap();
                    assert_eq!(
                        response.headers()[http::header::ALT_SVC].to_str().unwrap(),
                        format!("h3=\":{}\"; ma=3600", ports[1])
                    );
                    let mut stream = response.into_body();
                    let mut bytes = Vec::new();
                    while let Some(data) = stream.data().await {
                        let data = data.unwrap();
                        bytes.extend_from_slice(&data);
                        stream.flow_control().release_capacity(data.len()).unwrap();
                    }
                    assert_eq!(bytes, b"123");
                    drop(stream);
                    drop(client);
                    driver.await.unwrap().unwrap();
                }
            }
            quic.close(quinn::VarInt::from_u32(0x100), b"done");
            drop(sender);
            let _ = driver.await;
            endpoint.close(quinn::VarInt::from_u32(0x100), b"done");
            endpoint.wait_idle().await;
        })
        .await
        .unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "server did not stop");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn compiled_http3_udp_bind_failure_is_catchable() {
    let directory = TestDirectory::new();
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert = directory.path().join("cert.pem");
    let key = directory.path().join("key.pem");
    fs::write(&cert, certificate.cert.pem()).unwrap();
    fs::write(&key, certificate.key_pair.serialize_pem()).unwrap();
    let occupied = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
    let port = occupied.local_addr().unwrap().port();
    let source = format!(
        r#"class Application {{ main() {{
        final server = Server()
        server.tls('{cert}', '{key}')
        server.enableHttp3({port})
        try {{ server.start(0) }} catch (GenericError err) {{ print('UDP occupied') }}
    }} }}"#,
        cert = cert.display(),
        key = key.display()
    );
    let binary = compile_ube_server(&source, directory.path());
    let output = Command::new(binary).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"UDP occupied\n");
}

#[test]
fn compiled_ube_server_receives_post_and_put_bodies() {
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let source = format!(
        r#"
        class Application(var int calls) {{
            Response echo(Request request) {{
                this._calls = this._calls + 1
                return Response.ok(request.body())
            }}
            Response checked(Request request) {{
                try {{ return Response.ok(request.body()) }}
                catch (GenericError error) {{ return Response.ok('invalid text') }}
            }}
            main() {{
                final server = Server()
                server.post('/echo', request -> this.echo(request))
                server.put('/echo', request -> this.echo(request))
                server.get('/calls', request -> Response.ok('${{this._calls}}'))
                server.post('/checked', request -> this.checked(request))
                server.start({port})
            }}
        }}
    "#
    );
    let directory = TestDirectory::new();
    let binary = compile_ube_server(&source, directory.path());
    let mut server = RunningServer(
        Command::new(binary)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + Duration::from_secs(5);
    while TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err() {
        assert!(server.0.try_wait().unwrap().is_none());
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(25));
    }
    let json = r#"{"name":"Ube🦀","enabled":true}"#;
    for method in ["POST", "PUT"] {
        let raw = format!(
            "{method} /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{json}",
            json.len()
        );
        let response = String::from_utf8(send_request(address, &raw)).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
        assert_eq!(response.split_once("\r\n\r\n").unwrap().1, json);
    }
    let response = String::from_utf8(send_request(address, "PUT /echo HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n")).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 "));
    assert!(response.ends_with("\r\n\r\nabcde"));
    let response = String::from_utf8(send_request(
        address,
        "POST /checked HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1\r\n\r\n\0",
    ))
    .unwrap();
    assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
    assert!(response.ends_with("invalid text"), "{response}");
    // Exercise real binary bytes across TCP and the generated catch path.
    let mut binary = TcpStream::connect(address).unwrap();
    binary
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    binary
        .write_all(b"POST /checked HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1\r\n\r\n\xff")
        .unwrap();
    let mut response = String::new();
    binary.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 "));
    assert!(response.ends_with("invalid text"));

    // A client waits for 100 Continue before sending either fixed or chunked data.
    // The server must not dispatch while the final payload bytes are still absent.
    for (framing, first, last) in [
        ("Content-Length: 5", "ab", "cde"),
        (
            "Transfer-Encoding: chunked",
            "3\r\nab",
            "c\r\n2\r\nde\r\n0\r\n\r\n",
        ),
    ] {
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(
            stream,
            "POST /echo HTTP/1.1\r\nHost: localhost\r\n{framing}\r\nExpect: 100-continue\r\n\r\n"
        )
        .unwrap();
        let mut interim = [0; 25];
        stream.read_exact(&mut interim).unwrap();
        assert_eq!(&interim, b"HTTP/1.1 100 Continue\r\n\r\n");
        stream.write_all(first.as_bytes()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let error = stream
            .read(&mut [0; 1])
            .expect_err("handler must await the complete body");
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ));
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream.write_all(last.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
        assert!(response.ends_with("\r\n\r\nabcde"), "{response}");
    }

    // The exact decoded size limit succeeds; one byte above it is rejected below.
    let payload = "x".repeat(1024 * 1024);
    let response = send_request(
        address,
        &format!(
            "PUT /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{payload}",
            payload.len()
        ),
    );
    let response = String::from_utf8(response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 "));
    assert_eq!(response.split_once("\r\n\r\n").unwrap().1, payload);
    let response = send_request(address, "POST /echo HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert!(response.starts_with(b"HTTP/1.1 200 "));
    assert!(response.ends_with(b"\r\n\r\n"));

    // A counter in the compiled handler proves rejected requests never reach it.
    let calls_before = session_http1(address, "/calls", None).body;
    for (headers, status) in [
        ("Content-Length: 1048577", 413),
        ("Content-Length: 1048577\r\nExpect: 100-continue", 413),
        ("Content-Length: 0\r\nTransfer-Encoding: chunked", 400),
        ("Content-Length: 0\r\nContent-Length: 0", 400),
        ("Transfer-Encoding: gzip", 400),
        ("Expect: unsupported", 417),
    ] {
        let response = String::from_utf8(send_request(
            address,
            &format!("POST /echo HTTP/1.1\r\nHost: localhost\r\n{headers}\r\n\r\n"),
        ))
        .unwrap();
        assert!(
            response.starts_with(&format!("HTTP/1.1 {status} ")),
            "{headers}: {response}"
        );
    }
    for (framing, body) in [
        ("Transfer-Encoding: chunked", "Z\r\n"),
        ("Transfer-Encoding: chunked", "100001\r\n"),
        (
            "Transfer-Encoding: chunked",
            "1;unsupported=yes\r\nx\r\n0\r\n\r\n",
        ),
        (
            "Transfer-Encoding: chunked",
            "0\r\nX-Trailer: value\r\n\r\n",
        ),
    ] {
        let status = if body == "100001\r\n" { 413 } else { 400 };
        let response = send_request(
            address,
            &format!("POST /echo HTTP/1.1\r\nHost: localhost\r\n{framing}\r\n\r\n{body}"),
        );
        assert!(
            response.starts_with(format!("HTTP/1.1 {status} ").as_bytes()),
            "{}",
            String::from_utf8_lossy(&response)
        );
    }
    // EOF before Content-Length must reject the request rather than dispatch it.
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .write_all(b"POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\n\r\nx")
        .unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 400 "), "{response}");
    // A stalled sender must receive a 408, with no handler invocation.
    let mut stalled = TcpStream::connect(address).unwrap();
    stalled
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    stalled
        .write_all(b"POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stalled.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 408 "), "{response}");
    assert_eq!(session_http1(address, "/calls", None).body, calls_before);
    // Malformed clients must not prevent subsequent valid requests from succeeding.
    let response = send_request(
        address,
        "POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\n\r\nok",
    );
    assert!(response.starts_with(b"HTTP/1.1 200 "));
    assert!(response.ends_with(b"\r\n\r\nok"));
}

struct BodyCase {
    method: &'static str,
    path: &'static str,
    length: Option<String>,
    data: bytes::Bytes,
    status: u16,
    expected: bytes::Bytes,
}

fn multiplexed_body_cases() -> Vec<BodyCase> {
    use bytes::Bytes;
    let mut cases = Vec::new();
    for method in ["POST", "PUT"] {
        for length in [false, true] {
            let data = Bytes::from_static("{\"name\":\"Ube🦀\"}".as_bytes());
            cases.push(BodyCase {
                method,
                path: "/echo",
                length: length.then(|| data.len().to_string()),
                expected: data.clone(),
                data,
                status: 200,
            });
        }
    }
    for size in [0, 1024 * 1024, 1024 * 1024 + 1] {
        let data = Bytes::from(vec![b'x'; size]);
        cases.push(BodyCase {
            method: "POST",
            path: "/echo",
            length: None,
            expected: if size > 1024 * 1024 {
                Bytes::new()
            } else {
                data.clone()
            },
            data,
            status: if size > 1024 * 1024 { 413 } else { 200 },
        });
    }
    for (length, status) in [("1048577", 413), ("+1", 400), ("1, 1", 400), ("2", 400)] {
        cases.push(BodyCase {
            method: "POST",
            path: "/echo",
            length: Some(length.into()),
            data: Bytes::new(),
            status,
            expected: Bytes::new(),
        });
    }
    for data in [Bytes::from_static(b"\0"), Bytes::from_static(b"\xff")] {
        cases.push(BodyCase {
            method: "POST",
            path: "/checked",
            length: None,
            data,
            status: 200,
            expected: Bytes::from_static(b"invalid text"),
        });
    }
    cases
}

fn compiled_multiplexed_body_server(
    http3: bool,
) -> (
    TestDirectory,
    RunningServer,
    rcgen::CertifiedKey,
    SocketAddr,
) {
    let directory = TestDirectory::new();
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert = directory.path().join("cert.pem");
    let key = directory.path().join("key.pem");
    fs::write(&cert, certificate.cert.pem()).unwrap();
    fs::write(&key, certificate.key_pair.serialize_pem()).unwrap();
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let source = format!(
        r#"
        class Application(var int calls) {{
            Response echo(Request request) {{
                this._calls = this._calls + 1
                return Response.ok(request.body())
            }}
            Response checked(Request request) {{
                try {{ return Response.ok(request.body()) }}
                catch (GenericError error) {{ return Response.ok('invalid text') }}
            }}
            main() {{
                final server = Server()
                server.tls('{cert}', '{key}')
                server.enableHttp2()
                {enable_h3}
                server.get('/ports', request -> Response.ok('${{server.http3Port()}}'))
                server.get('/calls', request -> Response.ok('${{this._calls}}'))
                server.post('/echo', request -> this.echo(request))
                server.put('/echo', request -> this.echo(request))
                server.post('/checked', request -> this.checked(request))
                server.start({port})
            }}
        }}
    "#,
        cert = cert.display(),
        key = key.display(),
        enable_h3 = if http3 { "server.enableHttp3(0)" } else { "" }
    );
    let binary = compile_ube_server(&source, directory.path());
    let mut server = RunningServer(
        Command::new(binary)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + Duration::from_secs(5);
    while TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err() {
        assert!(server.0.try_wait().unwrap().is_none());
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(25));
    }
    (directory, server, certificate, address)
}

fn body_client_tls(certificate: &rcgen::CertifiedKey, alpn: &[u8]) -> rustls::ClientConfig {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate.cert.der().clone()).unwrap();
    let mut config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![alpn.to_vec()];
    config
}

async fn h2_body_reply(future: h2::client::ResponseFuture) -> Result<(u16, Vec<u8>), h2::Error> {
    let response = future.await?;
    assert_eq!(response.version(), http::Version::HTTP_2);
    let status = response.status().as_u16();
    let mut stream = response.into_body();
    let mut body = Vec::new();
    while let Some(data) = stream.data().await {
        let data = data?;
        body.extend_from_slice(&data);
        stream.flow_control().release_capacity(data.len())?;
    }
    Ok((status, body))
}

#[test]
fn compiled_http2_server_buffers_bounded_bodies_without_blocking_other_streams() {
    use bytes::Bytes;
    let (_directory, _server, certificate, address) = compiled_multiplexed_body_server(false);
    ube_runtime::net::get_runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(35), async {
            let tcp = tokio::net::TcpStream::connect(address).await.unwrap();
            let tls = tokio_rustls::TlsConnector::from(std::sync::Arc::new(body_client_tls(
                &certificate,
                b"h2",
            )))
            .connect("localhost".try_into().unwrap(), tcp)
            .await
            .unwrap();
            let (mut client, connection) = h2::client::handshake(tls).await.unwrap();
            let driver = tokio::spawn(connection);
            let mut calls = 0;
            for case in multiplexed_body_cases() {
                let mut request = http::Request::builder()
                    .method(case.method)
                    .uri(format!("https://localhost{}", case.path));
                if let Some(length) = &case.length {
                    request = request.header("content-length", length);
                }
                let end_stream = case.data.is_empty() && case.status != 413;
                let (response, mut send) = client
                    .send_request(request.body(()).unwrap(), end_stream)
                    .unwrap();
                for (index, chunk) in case.data.chunks(16 * 1024).enumerate() {
                    send.reserve_capacity(chunk.len());
                    while send.capacity() < chunk.len() {
                        futures_capacity(&mut send).await;
                    }
                    send.send_data(
                        Bytes::copy_from_slice(chunk),
                        (index + 1) * 16 * 1024 >= case.data.len(),
                    )
                    .unwrap();
                }
                match h2_body_reply(response).await {
                    Ok((status, body)) => {
                        assert_eq!(status, case.status, "{} {:?}", case.path, case.length);
                        assert_eq!(body, case.expected);
                    }
                    Err(error) => assert!(
                        case.status == 400 && error.is_reset(),
                        "{} {:?} expected {}: {error}",
                        case.path,
                        case.length,
                        case.status
                    ),
                }
                if case.path == "/echo" && case.status == 200 {
                    calls += 1;
                }
            }
            let request = http::Request::builder()
                .method("POST")
                .uri("https://localhost/echo")
                .header("expect", "unsupported")
                .body(())
                .unwrap();
            assert_eq!(
                h2_body_reply(client.send_request(request, true).unwrap().0)
                    .await
                    .unwrap()
                    .0,
                417
            );
            let request = http::Request::builder()
                .method("POST")
                .uri("https://localhost/echo")
                .body(())
                .unwrap();
            let (response, mut trailer_send) = client.send_request(request, false).unwrap();
            trailer_send
                .send_data(Bytes::from_static(b"a"), false)
                .unwrap();
            let mut trailers = http::HeaderMap::new();
            trailers.insert("x-test", http::HeaderValue::from_static("unsupported"));
            trailer_send.send_trailers(trailers).unwrap();
            assert_eq!(h2_body_reply(response).await.unwrap().0, 400);
            drop(trailer_send);
            let request = http::Request::builder()
                .method("POST")
                .uri("https://localhost/echo")
                .body(())
                .unwrap();
            let (cancelled, mut cancel_send) = client.send_request(request, false).unwrap();
            cancel_send
                .send_data(Bytes::from_static(b"partial"), false)
                .unwrap();
            cancel_send.send_reset(h2::Reason::CANCEL);
            drop(cancel_send);
            drop(cancelled);
            // Leave an upload unfinished, and prove another stream still completes.
            let request = http::Request::builder()
                .method("POST")
                .uri("https://localhost/echo")
                .header("expect", "100-continue")
                .body(())
                .unwrap();
            let (mut pending, mut send) = client.send_request(request, false).unwrap();
            let interim = tokio::time::timeout(
                Duration::from_secs(2),
                std::future::poll_fn(|cx| pending.poll_informational(cx)),
            )
            .await
            .unwrap()
            .unwrap()
            .unwrap();
            assert_eq!(interim.status(), 100);
            send.send_data(Bytes::from_static(b"ab"), false).unwrap();
            let request = http::Request::builder()
                .uri("https://localhost/calls")
                .body(())
                .unwrap();
            let fast = client.send_request(request, true).unwrap().0;
            let reply = tokio::time::timeout(Duration::from_secs(2), h2_body_reply(fast))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(reply, (200, calls.to_string().into_bytes()));
            send.send_data(Bytes::from_static(b"cde"), true).unwrap();
            assert_eq!(
                h2_body_reply(pending).await.unwrap(),
                (200, b"abcde".to_vec())
            );
            calls += 1;
            let request = http::Request::builder()
                .method("POST")
                .uri("https://localhost/echo")
                .body(())
                .unwrap();
            let (stalled, _send) = client.send_request(request, false).unwrap();
            assert_eq!(h2_body_reply(stalled).await.unwrap().0, 408);
            let request = http::Request::builder()
                .uri("https://localhost/calls")
                .body(())
                .unwrap();
            assert_eq!(
                h2_body_reply(client.send_request(request, true).unwrap().0)
                    .await
                    .unwrap(),
                (200, calls.to_string().into_bytes())
            );
            drop(send);
            drop(_send);
            drop(client);
            driver.await.unwrap().unwrap();
        })
        .await
        .unwrap();
    });
}

async fn futures_capacity(send: &mut h2::SendStream<bytes::Bytes>) {
    std::future::poll_fn(|cx| send.poll_capacity(cx))
        .await
        .unwrap()
        .unwrap();
}

async fn h3_body_bytes(
    stream: &mut h3::client::RequestStream<h3_quinn::BidiStream<bytes::Bytes>, bytes::Bytes>,
) -> Vec<u8> {
    use bytes::Buf;
    let mut body = Vec::new();
    while let Some(mut data) = stream.recv_data().await.unwrap() {
        while data.has_remaining() {
            let chunk = data.chunk();
            body.extend_from_slice(chunk);
            let n = chunk.len();
            data.advance(n);
        }
    }
    body
}

#[test]
fn compiled_http3_server_buffers_bounded_bodies_without_blocking_other_streams() {
    use bytes::Bytes;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (_directory, _server, certificate, address) = compiled_multiplexed_body_server(true);
    ube_runtime::net::get_runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(35), async {
            let tcp = tokio::net::TcpStream::connect(address).await.unwrap();
            let mut tls = tokio_rustls::TlsConnector::from(std::sync::Arc::new(body_client_tls(
                &certificate,
                b"http/1.1",
            )))
            .connect("localhost".try_into().unwrap(), tcp)
            .await
            .unwrap();
            tls.write_all(b"GET /ports HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();
            let mut reply = String::new();
            tls.read_to_string(&mut reply).await.unwrap();
            let port = reply
                .split_once("\r\n\r\n")
                .unwrap()
                .1
                .parse::<u16>()
                .unwrap();
            let config = quinn::crypto::rustls::QuicClientConfig::try_from(body_client_tls(
                &certificate,
                b"h3",
            ))
            .unwrap();
            let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
            endpoint
                .set_default_client_config(quinn::ClientConfig::new(std::sync::Arc::new(config)));
            let quic = endpoint
                .connect(SocketAddr::from(([127, 0, 0, 1], port)), "localhost")
                .unwrap()
                .await
                .unwrap();
            let (mut connection, mut sender) =
                h3::client::new(h3_quinn::Connection::new(quic.clone()))
                    .await
                    .unwrap();
            let driver = tokio::spawn(async move { connection.wait_idle().await });
            let mut calls = 0;
            for case in multiplexed_body_cases() {
                let mut request = http::Request::builder()
                    .method(case.method)
                    .uri(format!("https://localhost{}", case.path));
                if let Some(length) = &case.length {
                    request = request.header("content-length", length);
                }
                let mut stream = sender
                    .send_request(request.body(()).unwrap())
                    .await
                    .unwrap();
                for chunk in case.data.chunks(16 * 1024) {
                    stream
                        .send_data(Bytes::copy_from_slice(chunk))
                        .await
                        .unwrap();
                }
                stream.finish().await.unwrap();
                let response = stream.recv_response().await;
                match response {
                    Ok(response) => {
                        assert_eq!(response.version(), http::Version::HTTP_3);
                        assert_eq!(
                            response.status().as_u16(),
                            case.status,
                            "{} {:?}",
                            case.path,
                            case.length
                        );
                        assert_eq!(h3_body_bytes(&mut stream).await, case.expected);
                    }
                    Err(error) => assert_eq!(case.status, 400, "{error}"),
                }
                if case.path == "/echo" && case.status == 200 {
                    calls += 1;
                }
            }
            let request = http::Request::builder()
                .method("POST")
                .uri("https://localhost/echo")
                .header("expect", "unsupported")
                .body(())
                .unwrap();
            let mut rejected = sender.send_request(request).await.unwrap();
            rejected.finish().await.unwrap();
            assert_eq!(rejected.recv_response().await.unwrap().status(), 417);
            let request = http::Request::builder()
                .method("POST")
                .uri("https://localhost/echo")
                .body(())
                .unwrap();
            let mut trailer_send = sender.send_request(request).await.unwrap();
            trailer_send
                .send_data(Bytes::from_static(b"a"))
                .await
                .unwrap();
            let mut trailers = http::HeaderMap::new();
            trailers.insert("x-test", http::HeaderValue::from_static("unsupported"));
            trailer_send.send_trailers(trailers).await.unwrap();
            trailer_send.finish().await.unwrap();
            assert_eq!(trailer_send.recv_response().await.unwrap().status(), 400);
            let request = http::Request::builder()
                .method("POST")
                .uri("https://localhost/echo")
                .body(())
                .unwrap();
            let mut cancelled = sender.send_request(request).await.unwrap();
            cancelled
                .send_data(Bytes::from_static(b"partial"))
                .await
                .unwrap();
            cancelled.stop_stream(h3::error::Code::H3_REQUEST_CANCELLED);
            cancelled.stop_sending(h3::error::Code::H3_REQUEST_CANCELLED);
            drop(cancelled);
            let request = http::Request::builder()
                .method("POST")
                .uri("https://localhost/echo")
                .header("expect", "100-continue")
                .body(())
                .unwrap();
            let mut pending = sender.send_request(request).await.unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), pending.recv_response())
                    .await
                    .unwrap()
                    .unwrap()
                    .status(),
                100
            );
            pending.send_data(Bytes::from_static(b"ab")).await.unwrap();
            let request = http::Request::builder()
                .uri("https://localhost/calls")
                .body(())
                .unwrap();
            let mut fast = sender.send_request(request).await.unwrap();
            fast.finish().await.unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), fast.recv_response())
                    .await
                    .unwrap()
                    .unwrap()
                    .status(),
                200
            );
            assert_eq!(h3_body_bytes(&mut fast).await, calls.to_string().as_bytes());
            pending.send_data(Bytes::from_static(b"cde")).await.unwrap();
            pending.finish().await.unwrap();
            assert_eq!(pending.recv_response().await.unwrap().status(), 200);
            assert_eq!(h3_body_bytes(&mut pending).await, b"abcde");
            calls += 1;
            let request = http::Request::builder()
                .method("POST")
                .uri("https://localhost/echo")
                .body(())
                .unwrap();
            let mut stalled = sender.send_request(request).await.unwrap();
            assert_eq!(stalled.recv_response().await.unwrap().status(), 408);
            let request = http::Request::builder()
                .uri("https://localhost/calls")
                .body(())
                .unwrap();
            let mut check = sender.send_request(request).await.unwrap();
            check.finish().await.unwrap();
            assert_eq!(check.recv_response().await.unwrap().status(), 200);
            assert_eq!(
                h3_body_bytes(&mut check).await,
                calls.to_string().as_bytes()
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
fn compiled_server_survives_caught_and_uncaught_currency_mismatches() {
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let source = format!(
        r#"
        class Application(var int cleaned, var int continued) {{
            Money fail(bool subtract) {{
                try {{
                    if (subtract) {{ return Money.php(10.0).subtract(Money.usd(1.0)) }}
                    return Money.php(10.0).add(Money.usd(1.0))
                }} finally {{ this._cleaned = this._cleaned + 1 }}
            }}
            Response uncaught(bool subtract) {{
                final value = this.fail(subtract)
                this._continued = this._continued + 1
                return Response.ok('unexpected')
            }}
            Response valid() {{
                final original = Money.php(10.0)
                final sum = original.add(Money.php(5.0))
                final difference = original.subtract(Money.php(5.0))
                return Response.ok('${{original.amount() == 10.0}} ${{sum.amount() == 15.0}} ${{difference.amount() == 5.0}} ${{sum.currency().code()}}')
            }}
            Response recover(bool subtract) {{
                try {{
                    final value = this.fail(subtract)
                    this._continued = this._continued + 1
                    return Response.ok('unexpected')
                }}
                catch (CurrencyMismatchError error) {{ return Response.ok('recovered: ${{error.message()}}') }}
            }}
            main() {{
                final server = Server()
                server.get('/caught-add', request -> this.recover(false))
                server.get('/caught-subtract', request -> this.recover(true))
                server.get('/uncaught-add', request -> this.uncaught(false))
                server.get('/uncaught-subtract', request -> this.uncaught(true))
                server.post('/caught', request -> this.recover(request.body() == 'subtract'))
                server.post('/uncaught', request -> this.uncaught(request.body() == 'subtract'))
                server.get('/valid', request -> this.valid())
                server.get('/stats', request -> Response.ok('${{this._cleaned}} ${{this._continued}}'))
                server.get('/health', request -> Response.ok('alive'))
                server.start({port})
            }}
        }}
    "#
    );
    let directory = TestDirectory::new();
    let binary = compile_ube_server(&source, directory.path());
    let mut server = RunningServer(
        Command::new(binary)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + Duration::from_secs(5);
    while TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err() {
        assert!(server.0.try_wait().unwrap().is_none());
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(25));
    }
    let post = |path: &str, body: &str| {
        let response = send_request(
            address,
            &format!(
                "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        let response = String::from_utf8(response).unwrap();
        let (head, body) = response.split_once("\r\n\r\n").unwrap();
        (
            head.split_whitespace()
                .nth(1)
                .unwrap()
                .parse::<u16>()
                .unwrap(),
            body.to_owned(),
        )
    };
    let mut expected_cleanup = 0;
    for _ in 0..2 {
        for operation in ["add", "subtract"] {
            for caught in [true, false] {
                let mode = if caught { "caught" } else { "uncaught" };
                for method in ["GET", "POST"] {
                    let (status, body) = if method == "GET" {
                        let reply = session_http1(address, &format!("/{mode}-{operation}"), None);
                        (reply.status, reply.body)
                    } else {
                        post(&format!("/{mode}"), operation)
                    };
                    if caught {
                        assert_eq!(status, 200, "{method} {mode} {operation}");
                        assert_eq!(body, "recovered: Money currency mismatch: PHP vs USD");
                    } else {
                        assert_eq!(status, 500, "{method} {mode} {operation}");
                        assert_eq!(body, "Internal Server Error");
                    }
                    expected_cleanup += 1;
                    // Exactly one finally runs per operation. No statement
                    // after the failed arithmetic may execute in either path.
                    let stats = session_http1(address, "/stats", None);
                    assert_eq!(stats.status, 200);
                    assert_eq!(stats.body, format!("{expected_cleanup} 0"));
                    // A successful money operation after every failure proves
                    // pending exceptions were consumed and arithmetic still works.
                    let valid = session_http1(address, "/valid", None);
                    assert_eq!(valid.status, 200);
                    assert_eq!(valid.body, "true true true PHP");
                    let health = session_http1(address, "/health", None);
                    assert_eq!(health.status, 200);
                    assert_eq!(health.body, "alive");
                    assert!(server.0.try_wait().unwrap().is_none());
                }
            }
        }
    }
}
