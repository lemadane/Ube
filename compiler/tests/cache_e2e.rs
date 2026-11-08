//! Real Ube binaries, independent TCP clients, forced server death, and WAL recovery.
mod support;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use ube_compiler::{Lexer, Parser, TypeChecker};
const TOKEN: &str = "cache-integration-test-token";
struct Program {
    _temp: tempfile::TempDir,
    executable: PathBuf,
}
impl Program {
    fn new(source: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let entry = support::write_source(&temp.path().join("App.ube"), source);
        fs::write(temp.path().join(".gitignore"), ".build/\n").unwrap();
        let executable = temp.path().join("program");
        let result = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
            .arg("build")
            .arg(entry)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        Self {
            _temp: temp,
            executable,
        }
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("timeout");
        command
            .arg("30s")
            .arg(&self.executable)
            .args(args)
            .env("CACHE_TEST_TOKEN", TOKEN);
        command
    }
    fn server_command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.executable);
        command.args(args).env("CACHE_TEST_TOKEN", TOKEN);
        command
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
}
fn expect(output: Output, expected: &str) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}
struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn started(mut command: Command, prefix: &str) -> Running {
    let mut child = Running(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(line);
    });
    let line = rx
        .recv_timeout(Duration::from_secs(15))
        .expect("server startup deadline");
    assert!(line.starts_with(prefix), "server startup: {line}");
    child
}
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
fn http(port: u16) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut result = String::new();
    stream.read_to_string(&mut result).unwrap();
    result
}
fn standalone(address: &str, db: &Path, metrics: u16) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ube-compiler"));
    command
        .args(["cache", "--bind", address, "--disk"])
        .arg(db)
        .args([
            "--memory",
            "4096",
            "--metrics",
            &metrics.to_string(),
            "--token-env",
            "CACHE_TEST_TOKEN",
        ])
        .env("CACHE_TEST_TOKEN", TOKEN);
    command
}
const CLIENT: &str = r#"class App { start() {
    final args = Console.args()
    final cache = Cache.connect(args[0], Env.require('CACHE_TEST_TOKEN'))
    if (args[1] == 'seed') {
        cache.set('keep', 'hello 世界', 0)
        cache.set('counter', '0', 0)
        cache.set('deleted', 'gone', 0)
        Console.printLine(cache.delete('deleted'))
        cache.set('expired', 'gone', 100)
        Console.printLine(cache.get('keep') ?? 'missing')
    }
    if (args[1] == 'worker') {
        Console.printLine(cache.add('winner', 'claimed', 0))
        var i = 0
        for (i < 25) { cache.increment('counter', 1) i = i + 1 }
    }
    if (args[1] == 'offline') { cache.set('offline', 'gone', 400) }
    if (args[1] == 'report') {
        Console.printLine(cache.get('keep') ?? 'missing')
        Console.printLine(cache.get('deleted') ?? 'missing')
        Console.printLine(cache.get('expired') ?? 'missing')
        Console.printLine(cache.get('counter') ?? 'missing')
        Console.printLine(cache.get('winner') ?? 'missing')
        Console.printLine(cache.get('offline') ?? 'missing')
    }
    if (args[1] == 'unauthorized') {
        try { final value = cache.get('keep') } catch (Error err) { Console.printLine('auth rejected') }
    }
    cache.close()
} }"#;
#[test]
fn standalone_shares_atomic_values_and_recovers_after_sigkill() {
    let client = Program::new(CLIENT);
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("cache.db");
    let port = free_port();
    let address = format!("127.0.0.1:{port}");
    let metrics = loop {
        let candidate = free_port();
        if candidate != port {
            break candidate;
        }
    };
    let mut server = started(standalone(&address, &db, metrics), "Ube cache listening");
    expect(client.run(&[&address, "seed"]), "true\nhello 世界\n");
    let workers: Vec<_> = (0..4)
        .map(|_| {
            client
                .command(&[&address, "worker"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let mut winners = 0;
    for worker in workers {
        let output = worker.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        match output.stdout.as_slice() {
            b"true\n" => winners += 1,
            b"false\n" => {}
            _ => panic!("unexpected worker output"),
        }
    }
    assert_eq!(winners, 1);
    std::thread::sleep(Duration::from_millis(150));
    expect(
        client.run(&[&address, "report"]),
        "hello 世界\nmissing\nmissing\n100\nclaimed\nmissing\n",
    );
    let mut unauthorized = client.command(&[&address, "unauthorized"]);
    unauthorized.env("CACHE_TEST_TOKEN", "a-different-test-token");
    expect(unauthorized.output().unwrap(), "auth rejected\n");
    let output = http(metrics);
    for name in [
        "ube_cache_hits_total",
        "ube_cache_misses_total",
        "ube_cache_writes_total",
        "ube_cache_items 3\n",
        "ube_cache_accounted_bytes",
    ] {
        assert!(output.contains(name), "missing {name}: {output}");
    }
    assert!(!output.contains(TOKEN));
    assert!(!output.contains("hello 世界"));
    // Malformed clients must receive an error without breaking subsequent clients.
    for bytes in [&[255, 255, 255, 255][..], &[0, 0, 0, 1, 255][..]] {
        let mut stream = TcpStream::connect(&address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream.write_all(bytes).unwrap();
        let mut header = [0; 4];
        stream.read_exact(&mut header).unwrap();
        let mut body = vec![0; u32::from_be_bytes(header) as usize];
        stream.read_exact(&mut body).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(value.get("error").is_some());
    }
    expect(client.run(&[&address, "offline"]), "");
    // Kill without calling close or checkpoint. All acknowledged writes must recover.
    server.0.kill().unwrap();
    server.0.wait().unwrap();
    drop(server);
    std::thread::sleep(Duration::from_millis(500));
    let _restarted = started(standalone(&address, &db, metrics), "Ube cache listening");
    expect(
        client.run(&[&address, "report"]),
        "hello 世界\nmissing\nmissing\n100\nclaimed\nmissing\n",
    );
}
#[test]
fn embedded_listener_shares_values_and_runtime_cleanup_releases_disk_lock() {
    let server = Program::new(
        r#"class App { start() {
        final args = Console.args()
        final cache = Cache.open(args[0], 4096)
        cache.listen(args[1], Env.require('CACHE_TEST_TOKEN'))
        Console.printLine('listening')
        final stop = Console.readLine()
    } }"#,
    );
    let client = Program::new(CLIENT);
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("embedded.db");
    let address = format!("127.0.0.1:{}", free_port());
    let mut running = started(
        server.server_command(&[db.to_str().unwrap(), &address]),
        "listening",
    );
    expect(client.run(&[&address, "seed"]), "true\nhello 世界\n");
    let mut slow_client = TcpStream::connect(&address).unwrap();
    slow_client.write_all(&[0, 0]).unwrap();
    writeln!(running.0.stdin.as_mut().unwrap()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = running.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "cleanup stalled");
        std::thread::sleep(Duration::from_millis(10));
    }
    let _reopened = started(
        server.server_command(&[db.to_str().unwrap(), &address]),
        "listening",
    );
    std::thread::sleep(Duration::from_millis(150));
    expect(
        client.run(&[&address, "report"]),
        "hello 世界\nmissing\nmissing\n0\nmissing\nmissing\n",
    );
}
#[test]
fn native_memory_cache_eviction_errors_and_buffered_persistence() {
    let program = Program::new(
        r#"class App { start() {
        final cache = Cache.memory(132)
        Console.printLine(cache.add('a','1',0))
        Console.printLine(cache.add('a','9',0))
        cache.set('b','2',0)
        Console.printLine(cache.get('a') ?? 'missing')
        cache.set('c','3',0)
        Console.printLine(cache.get('b') ?? 'missing')
        Console.printLine(cache.increment('a',2))
        Console.printLine(cache.delete('a'))
        Console.printLine(cache.delete('a'))
        try { cache.set('c','3',-1) } catch (Error err) { Console.printLine('ttl rejected') }
        try { cache.increment('missing',1) } catch (Error err) { Console.printLine('missing rejected') }
        cache.close()
        cache.close()
        try { final value = cache.get('c') } catch (Error err) { Console.printLine('closed rejected') }
        final disk = Cache.openBuffered(Console.args()[0],4096)
        disk.set('empty','',0)
        disk.set('json','{"ok":true}',0)
        disk.close()
        final reopened = Cache.open(Console.args()[0],4096)
        Console.printLine(reopened.get('empty') ?? 'missing')
        Console.printLine(reopened.get('json') ?? 'missing')
        reopened.close()
    } }"#,
    );
    let temp = tempfile::tempdir().unwrap();
    expect(program.run(&[temp.path().join("buffered.db").to_str().unwrap()]), "true\nfalse\n1\nmissing\n3\ntrue\nfalse\nttl rejected\nmissing rejected\nclosed rejected\n\n{\"ok\":true}\n");
}
#[test]
fn cache_signatures_reject_invalid_calls() {
    for statement in [
        "Cache()",
        "Cache.memory('bad')",
        "Cache.get('key')",
        "Cache.open('path')",
        "Cache.connect('addr',1)",
        "Cache.memory(1024).memory(1024)",
        "Cache.memory(1024).set('key',1,0)",
        "Cache.memory(1024).get(1)",
        "Cache.memory(1024).unknown()",
    ] {
        let source = format!("class App {{ start() {{ {statement} }} }}");
        let mut parser = Parser::new(Lexer::new(&source));
        let program = parser.parse_program();
        let mut checker = TypeChecker::new();
        assert!(
            !parser.diagnostics.is_empty() || !checker.check_program(&program),
            "accepted {statement}"
        );
    }
}

#[test]
fn standalone_cli_reports_configuration_errors() {
    let help = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
        .args(["cache", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--disk"));
    for args in [
        vec!["cache", "--buffered"],
        vec!["cache", "--memory"],
        vec!["cache", "--memory", "bad"],
        vec!["cache", "--metrics", "0"],
        vec!["cache"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
            .args(args)
            .env_remove("UBE_CACHE_TOKEN")
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("Error:"));
    }
}
