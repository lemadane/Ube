//! Compile Ube, link the current runtime, then exercise its management listener.
mod support;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};
use ube_compiler::{Lexer, Parser, TypeChecker};

fn diagnostics(source: &str) -> Vec<String> {
    let mut parser = Parser::new(Lexer::new(source));
    let program = parser.parse_program();
    if !parser.diagnostics.is_empty() {
        return parser
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
    }
    let mut checker = TypeChecker::new();
    checker.check_program(&program);
    checker
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect()
}
#[test]
fn api_rejects_invalid_calls() {
    for statement in [
        "Metrics()",
        "Health()",
        "Observability()",
        "Metrics.increment(42)",
        "Metrics.record('latency', 'bad')",
        "Health.ready(1)",
        "Observability.start()",
        "Metrics.unknown()",
    ] {
        assert!(
            !diagnostics(&format!("class Main {{ main() {{ {statement} }} }}")).is_empty(),
            "accepted {statement}"
        );
    }
    assert!(diagnostics("class Main { main() { Metrics.increment('count') Metrics.add('count', 2) Metrics.gauge('size', 12) Metrics.record('seconds', 0.25) Health.ready(true) Observability.start(9090) Observability.stop() } }").is_empty());
}
struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn http(port: u16, method: &str, path: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut output = String::new();
    stream.read_to_string(&mut output).unwrap();
    output
}
#[test]
fn native_listener_metrics_health_restart_and_cleanup() {
    // Hold the port while compiling to avoid collisions with parallel tests.
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    let temp = tempfile::tempdir().unwrap();
    let source = format!(
        r#"class Main {{ main() {{
        Metrics.increment('orders_created_total')
        Metrics.add('orders_created_total', 2)
        Metrics.gauge('orders_pending', 12)
        Metrics.record('order_processing_seconds', 0.025)
        Metrics.record('order_processing_seconds', 0.075)
        try {{ Metrics.add('orders_created_total', -1) }} catch (Error err) {{ Console.printLine('negative rejected') }}
        try {{ Metrics.gauge('orders_created_total', 2) }} catch (Error err) {{ Console.printLine('type rejected') }}
        try {{ Metrics.increment('ube_reserved') }} catch (Error err) {{ Console.printLine('reserved rejected') }}
        try {{ Observability.start(0) }} catch (Error err) {{ Console.printLine('port rejected') }}
        Observability.start({port})
        try {{ Observability.start({port}) }} catch (Error err) {{ Console.printLine('duplicate rejected') }}
        Console.printLine('listening')
        final first = Console.readLine()
        Health.ready(true)
        Console.printLine('ready')
        final second = Console.readLine()
        Health.ready(false)
        Console.printLine('unready')
        final third = Console.readLine()
        Observability.stop()
        Observability.stop()
        Observability.start({port})
        Console.printLine('restarted')
        final fourth = Console.readLine()
    }} }}"#
    );
    let entry = support::write_source(&temp.path().join("Main.ube"), &source);
    fs::write(temp.path().join(".gitignore"), ".build/\n").unwrap();
    let executable = temp.path().join("program");
    let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
        .arg("build")
        .arg(entry)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    drop(reservation);
    let mut process = Running(
        Command::new(&executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let stdout = process.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let next = || rx.recv_timeout(Duration::from_secs(10)).unwrap();
    for expected in [
        "negative rejected",
        "type rejected",
        "reserved rejected",
        "port rejected",
        "duplicate rejected",
        "listening",
    ] {
        assert_eq!(next(), expected);
    }
    assert!(http(port, "GET", "/health/live").starts_with("HTTP/1.1 200"));
    assert!(http(port, "GET", "/health/ready").starts_with("HTTP/1.1 503"));
    let metrics = http(port, "GET", "/metrics");
    for expected in [
        "orders_created_total 3\n",
        "orders_pending 12\n",
        "order_processing_seconds_count 2\n",
        "order_processing_seconds_sum 0.1\n",
        "ube_uptime_seconds",
        "ube_tasks_active",
        "ube_gc_objects",
    ] {
        assert!(metrics.contains(expected), "missing {expected}: {metrics}");
    }
    for path in ["/env", "/config", "/shutdown", "/"] {
        assert!(http(port, "GET", path).starts_with("HTTP/1.1 404"));
    }
    assert!(http(port, "POST", "/metrics").starts_with("HTTP/1.1 405"));
    writeln!(process.0.stdin.as_mut().unwrap()).unwrap();
    assert_eq!(next(), "ready");
    let ready = http(port, "GET", "/health/ready");
    assert!(ready.starts_with("HTTP/1.1 200") && ready.contains("\"UP\""));
    writeln!(process.0.stdin.as_mut().unwrap()).unwrap();
    assert_eq!(next(), "unready");
    assert!(http(port, "GET", "/health/ready").starts_with("HTTP/1.1 503"));
    writeln!(process.0.stdin.as_mut().unwrap()).unwrap();
    assert_eq!(next(), "restarted");
    assert!(http(port, "GET", "/health/live").starts_with("HTTP/1.1 200"));
    // A slow/incomplete client must not prevent bounded shutdown.
    let mut slow = TcpStream::connect(("127.0.0.1", port)).unwrap();
    slow.write_all(b"GET /metrics HTTP/1.1\r\n").unwrap();
    writeln!(process.0.stdin.as_mut().unwrap()).unwrap();
    process.0.stdin.take();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = process.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "runtime cleanup stalled"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "runtime cleanup releases listener"
    );
}
