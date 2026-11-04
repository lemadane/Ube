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
