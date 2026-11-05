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
