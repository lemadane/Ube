mod support;
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Output, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use ube_compiler::{Lexer, Parser, TypeChecker};

fn execute(source: &str, command: &str) -> Output {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rest_test.ube");
    let path = support::write_source(&path, source);
    Command::new("timeout")
        .arg("90s")
        .arg(env!("CARGO_BIN_EXE_ube-compiler"))
        .arg(command)
        .arg(path)
        .output()
        .unwrap()
}
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
struct HttpFixture {
    base: String,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl HttpFixture {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            let mut connections = vec![];
            while !signal.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => connections.push(thread::spawn(move || serve(stream))),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                }
            }
            for connection in connections {
                connection.join().unwrap();
            }
        });
        Self {
            base,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap();
    }
}
fn serve(mut stream: TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut data = vec![];
    let mut buffer = [0; 4096];
    let header_end = loop {
        let read = match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        data.extend_from_slice(&buffer[..read]);
        if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
        assert!(data.len() < 65536);
    };
    let headers = String::from_utf8_lossy(&data[..header_end]);
    let mut lines = headers.lines();
    let first = lines.next().unwrap().split_whitespace().collect::<Vec<_>>();
    let method = first[0].to_owned();
    let target = first[1].to_owned();
    let headers: HashMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let length = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    while data.len() - header_end < length {
        let read = match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        data.extend_from_slice(&buffer[..read]);
    }
    let path = target.split('?').next().unwrap();
    if path == "/binary-header" {
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nX-Binary: \xff\r\nContent-Length: 2\r\n\r\nok");
        return;
    }
    if path == "/invalid-text" || path == "/nul-text" {
        let _ =
            stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 1\r\n\r\n");
        let _ = stream.write_all(if path == "/invalid-text" {
            b"\xff"
        } else {
            b"\x00"
        });
        return;
    }
    if path == "/chunked" {
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n");
        return;
    }
    if path == "/trickle" {
        let _ =
            stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 6\r\n\r\n");
        for byte in b"abcdef" {
            if stream.write_all(&[*byte]).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        return;
    }
    if path == "/truncated" {
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 20\r\n\r\nno");
        return;
    }
    if path == "/slow" {
        thread::sleep(Duration::from_millis(300));
    }
    let (status, body, extra) = match path {
        "/missing" => (404, "missing".into(), ""),
        "/redirect" => (302, "".into(), "Location: /inspect\r\n"),
        "/large" => (200, "x".repeat(2048), ""),
        "/bad-json" => (200, "not JSON".into(), ""),
        "/values" => (200, r#"{"name":"Ada","n":42,"big":9007199254740993,"ratio":0.25,"flag":false,"nil":null,"items":["first"],"a/b":{"~key":"escaped"}}"#.into(), ""),
        _ => (if method == "POST" { 201 } else { 200 }, serde_json_for_fixture(&method, &target, &headers, &data[header_end..header_end + length]), ""),
    };
    let response = format!("HTTP/1.1 {status} Test\r\nConnection: close\r\nContent-Type: application/json\r\nX-Test: fixture\r\nX-Empty: \r\n{extra}Content-Length: {}\r\n\r\n", body.len());
    if stream.write_all(response.as_bytes()).is_ok() && method != "HEAD" {
        if path == "/slow-body" {
            thread::sleep(Duration::from_millis(300));
        }
        let _ = stream.write_all(body.as_bytes());
    }
}
fn serde_json_for_fixture(
    method: &str,
    target: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
) -> String {
    // Echo the fixture's request fields as JSON for wire-level assertions.
    let escaped = |s: &str| {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    };
    format!(
        r#"{{"method":"{}","target":"{}","authorization":"{}","custom":"{}","contentType":"{}","body":"{}"}}"#,
        escaped(method),
        escaped(target),
        escaped(
            headers
                .get("authorization")
                .map(String::as_str)
                .unwrap_or("")
        ),
        escaped(headers.get("x-custom").map(String::as_str).unwrap_or("")),
        escaped(
            headers
                .get("content-type")
                .map(String::as_str)
                .unwrap_or("")
        ),
        escaped(&String::from_utf8_lossy(body))
    )
}

#[test]
fn fluent_requests_send_real_methods_headers_queries_json_and_inspect_responses() {
    let fixture = HttpFixture::new();
    let source = r#"
        class Application {
            main() {
                final request = RestInPeace.at('BASE').header('X-Custom', 'custom').bearer('token').query('q', 'a & b').accept('application/json')
                request.json(`{"name":"Ada"}`)
                final response = request.post('/inspect').expectStatus(201).expectHeader('x-test', 'fixture').expectJsonString('/method', 'POST')
                Testicle.equal('custom', response.jsonString('/custom'))
                Testicle.equal('Bearer token', response.jsonString('/authorization'))
                Testicle.equal('application/json', response.jsonString('/contentType'))
                Testicle.equal(`{"name":"Ada"}`, response.jsonString('/body'))
                Testicle.equal('/inspect?q=a+%26+b', response.jsonString('/target'))
                Testicle.equal('fixture', response.header('X-Test') ?? 'missing')
                Testicle.isTrue(response.header('unknown') == null)
                Testicle.isTrue(response.hasHeader('x-empty'))
                Testicle.equal('', response.header('x-empty') ?? 'missing')
                Testicle.isTrue(response.elapsedMs() >= 0L)
                Testicle.equal('HTTP/1.1', response.protocol())
                response.close()
                request.close()
                final simple = RestInPeace.given().baseUrl('BASE')
                simple.get('/inspect').expectJsonString('/method', 'GET').close()
                simple.body('payload').contentType('text/plain').put('/inspect').expectJsonString('/method', 'PUT').expectJsonString('/body', 'payload').close()
                simple.patch('/inspect').expectJsonString('/method', 'PATCH').close()
                simple.delete('/inspect').expectJsonString('/method', 'DELETE').close()
                simple.options('/inspect').expectJsonString('/method', 'OPTIONS').close()
                simple.send('PROPFIND', '/inspect').expectJsonString('/method', 'PROPFIND').close()
                simple.head('/inspect').expectStatus(200).expectBody('').close()
                simple.get('/missing').expectStatus(404).expectBody('missing').close()
                simple.get('/redirect').expectStatus(302).expectHeader('location', '/inspect').close()
                simple.get('/chunked').expectBody('abcdef').close()
                final binaryHeader = simple.get('/binary-header')
                Testicle.isTrue(binaryHeader.hasHeader('x-binary'))
                var rejected = false
                try { binaryHeader.header('x-binary') } catch (RestInPeaceError error) { rejected = true }
                Testicle.isTrue(rejected)
                binaryHeader.expectBody('ok').close()
                simple.close()
                final absolute = RestInPeace.given()
                absolute.get('BASE/values').expectStatus(200).close()
                absolute.close()
                print('rest passed')
            }
        }
    "#.replace("BASE", &fixture.base);
    assert_eq!(success(execute(&source, "run")), "rest passed\n");
}

#[test]
fn json_types_assertion_failures_limits_timeouts_and_closed_handles_are_recoverable() {
    let fixture = HttpFixture::new();
    let source = r#"
        class Application {
            main() {
                final request = RestInPeace.at('BASE')
                final response = request.get('/values')
                Testicle.equal('Ada', response.jsonString('/name'))
                Testicle.equal(42, response.jsonInt('/n'))
                Testicle.equal(9007199254740993L, response.jsonLong('/big'))
                Testicle.equal(0.25, response.jsonDecimal('/ratio'))
                Testicle.isFalse(response.jsonBool('/flag'))
                Testicle.isTrue(response.jsonIsNull('/nil'))
                Testicle.equal('first', response.jsonString('/items/0'))
                Testicle.equal('escaped', response.jsonString('/a~1b/~0key'))
                var caught = 0
                try { response.expectStatus(500) } catch (AssertionError error) { caught = caught + 1 }
                try { response.expectBody('wrong') } catch (AssertionError error) { caught = caught + 1 }
                try { response.expectHeader('x-test', 'wrong') } catch (AssertionError error) { caught = caught + 1 }
                try { response.expectJsonString('/name', 'wrong') } catch (AssertionError error) { caught = caught + 1 }
                try { response.jsonIsNull('/absent') } catch (RestInPeaceError error) { caught = caught + 1 }
                try { response.jsonString('/n') } catch (RestInPeaceError error) { caught = caught + 1 }
                try { response.jsonInt('/big') } catch (RestInPeaceError error) { caught = caught + 1 }
                try { response.jsonString('name') } catch (RestInPeaceError error) { caught = caught + 1 }
                try { response.jsonString('/a~2b') } catch (RestInPeaceError error) { caught = caught + 1 }
                response.close()
                try { response.status() } catch (RestInPeaceError error) { caught = caught + 1 }
                try { request.json('invalid JSON') } catch (RestInPeaceError error) { caught = caught + 1 }
                try { request.header('X-Unsafe', 'bad\r\ninjected: yes') } catch (RestInPeaceError error) { caught = caught + 1 }
                try { request.header('Content-Length', '999') } catch (RestInPeaceError error) { caught = caught + 1 }
                try { request.timeout(0) } catch (RestInPeaceError error) { caught = caught + 1 }
                try { request.maxBytes(0) } catch (RestInPeaceError error) { caught = caught + 1 }
                try { request.maxBytes(16).get('/large') } catch (RestInPeaceError error) { caught = caught + 1 }
                request.maxBytes(1048576)
                try { request.timeout(50).get('/slow') } catch (RestInPeaceError error) { caught = caught + 1 }
                try { request.get('/slow-body') } catch (RestInPeaceError error) { caught = caught + 1 }
                try { request.get('/trickle') } catch (RestInPeaceError error) { caught = caught + 1 }
                request.timeout(2000)
                try { request.get('/truncated') } catch (RestInPeaceError error) { caught = caught + 1 }
                try { request.get('/invalid-text') } catch (RestInPeaceError error) { caught = caught + 1 }
                try { request.get('/nul-text') } catch (RestInPeaceError error) { caught = caught + 1 }
                final invalid = request.get('/bad-json')
                try { invalid.jsonString('/name') } catch (RestInPeaceError error) { caught = caught + 1 }
                invalid.close()
                request.get('/missing').expectStatus(404).close()
                request.close()
                try { request.get('/values') } catch (RestInPeaceError error) { caught = caught + 1 }
                try { RestInPeace.at('file:///etc/passwd') } catch (RestInPeaceError error) { caught = caught + 1 }
                Testicle.equal(25, caught)
                print('errors recovered')
            }
        }
    "#.replace("BASE", &fixture.base);
    assert_eq!(success(execute(&source, "run")), "errors recovered\n");
}

#[test]
fn native_test_runner_supports_parallel_async_http_requests() {
    let fixture = HttpFixture::new();
    let source = r#"
        class HttpTests {
            async string fetch(string marker) {
                final request = RestInPeace.at('BASE').header('X-Custom', marker)
                final response = request.get('/inspect').expectStatus(200)
                final value = response.jsonString('/custom')
                response.close()
                request.close()
                return value
            }
            async testParallel() {
                final one = this.fetch('one')
                final two = this.fetch('two')
                Testicle.equal('one', await one)
                Testicle.equal('two', await two)
            }
        }
    "#
    .replace("BASE", &fixture.base);
    assert!(success(execute(&source, "test")).contains("Tests: 1 passed, 0 failed"));
}

#[test]
fn https_verifies_certificates_and_negotiates_http2_with_an_explicit_test_ca() {
    let directory = tempfile::tempdir().unwrap();
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let ca_file = directory.path().join("ca.pem");
    fs::write(&ca_file, certificate.cert.pem()).unwrap();
    let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![certificate.cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.key_pair.serialize_der()).into(),
    )
    .unwrap();
    tls.alpn_protocols = vec![b"h2".to_vec()];
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let worker = thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
                let mut served = 0;
                for _ in 0..2 {
                    let (stream, _) =
                        tokio::time::timeout(Duration::from_secs(20), listener.accept())
                            .await
                            .unwrap()
                            .unwrap();
                    let Ok(stream) = acceptor.accept(stream).await else {
                        continue;
                    };
                    assert_eq!(stream.get_ref().1.alpn_protocol(), Some(b"h2".as_slice()));
                    let mut connection = h2::server::handshake(stream).await.unwrap();
                    while let Some(request) = connection.accept().await {
                        let (_, mut responder) = request.unwrap();
                        let response = http::Response::builder()
                            .status(200)
                            .header("content-type", "application/json")
                            .body(())
                            .unwrap();
                        responder
                            .send_response(response, false)
                            .unwrap()
                            .send_data(bytes::Bytes::from_static(br#"{"secure":true}"#), true)
                            .unwrap();
                        served += 1;
                    }
                }
                served
            })
    });
    let source = format!(
        r#"
        class Application {{ main() {{
            final untrusted = RestInPeace.at('https://localhost:{port}')
            var caught = false
            try {{ untrusted.get('/') }} catch (RestInPeaceError error) {{ caught = true }}
            Testicle.isTrue(caught)
            untrusted.close()
            final request = RestInPeace.at('https://localhost:{port}').caFile('{}')
            final response = request.get('/').expectStatus(200)
            Testicle.equal('HTTP/2', response.protocol())
            Testicle.isTrue(response.jsonBool('/secure'))
            response.close()
            request.close()
            print('https h2 verified')
        }} }}
    "#,
        ca_file.display()
    );
    let output = execute(&source, "run");
    assert_eq!(worker.join().unwrap(), 1);
    assert_eq!(success(output), "https h2 verified\n");
}

#[test]
fn compiler_rejects_invalid_factories_receivers_types_arity_and_nulls() {
    for body in [
        "final request = RestInPeace()",
        "final request = RestInPeaceRequest()",
        "final response = RestInPeaceResponse()",
        "RestInPeace.get('http://localhost')",
        "RestInPeace.at(null)",
        "RestInPeace.given(1)",
        "RestInPeace.given().header('name', null)",
        "RestInPeace.given().timeout('slow')",
        "RestInPeace.given().get()",
        "RestInPeace.given().given()",
        "RestInPeaceResponse.status()",
        "RestInPeace.given().get('http://localhost').expectStatus('ok')",
    ] {
        let source = format!("class Application {{ main() {{ {body} }} }}");
        let mut parser = Parser::new(Lexer::new(&source));
        let program = parser.parse_program();
        assert!(
            parser.diagnostics.is_empty(),
            "{body}: {:?}",
            parser.diagnostics
        );
        assert!(
            !TypeChecker::new().check_program(&program),
            "accepted {body}"
        );
    }
}

struct RunningService(Child);
impl Drop for RunningService {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn rest_in_peace_tests_a_real_compiled_ube_http_service() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let directory = tempfile::tempdir().unwrap();
    let service_source = directory.path().join("service.ube");
    let binary = directory.path().join("service");
    fs::write(
        &service_source,
        format!(
            r#"
        class Application {{ main() {{
            final server = Server()
            server.get('/health', request -> Response.ok('ready'))
            server.post('/echo', request -> Response.ok(request.body()))
            server.start({port})
        }} }}
    "#
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
        .arg("build")
        .arg(service_source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    success(output);
    let mut service = RunningService(
        Command::new(binary)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        assert!(
            service.0.try_wait().unwrap().is_none(),
            "compiled service exited early"
        );
        assert!(Instant::now() < deadline, "service startup timed out");
        thread::sleep(Duration::from_millis(10));
    }
    let source = format!(
        r#"
        class ServiceTests {{ testEndpoints() {{
            final request = RestInPeace.at('http://127.0.0.1:{port}')
            request.get('/health').expectStatus(200).expectBody('ready').close()
            final response = request.json(`{{"name":"Ada"}}`).post('/echo').expectStatus(200)
            Testicle.equal('Ada', response.jsonString('/name'))
            response.close()
            request.get('/not-found').expectStatus(404).close()
            request.close()
        }} }}
    "#
    );
    assert!(success(execute(&source, "test")).contains("Tests: 1 passed, 0 failed"));
}
