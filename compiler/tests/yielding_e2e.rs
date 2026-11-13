//! Backend regression fixtures intentionally retain legacy async method declarations.
//! They exercise resumable method frames; ordinary Async.start method calls use the blocking pool.
//! Real native continuations, including thread-count checks above the old pool limit.
#![cfg(unix)]
mod support;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    time::Duration,
};

struct Program {
    _temp: tempfile::TempDir,
    executable: PathBuf,
    llvm: String,
}
impl Program {
    fn new(source: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let source = support::write_source(&temp.path().join("App.ube"), source);
        fs::write(temp.path().join(".gitignore"), ".build/\n").unwrap();
        let executable = temp.path().join("program");
        let runtime = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("libube_runtime.a");
        assert!(runtime.exists());
        let compiler = PathBuf::from(env!("CARGO_BIN_EXE_ube-compiler"));
        let entry = source;
        let output = Command::new(&compiler)
            .args([
                "build",
                entry.to_str().unwrap(),
                "-o",
                executable.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let llvm = fs::read_to_string(entry.with_extension("ll")).unwrap();
        Self {
            _temp: temp,
            executable,
            llvm,
        }
    }
    fn run(&self) -> Output {
        Command::new("timeout")
            .arg("20s")
            .arg(&self.executable)
            .env("TOKIO_WORKER_THREADS", "1")
            .output()
            .unwrap()
    }
}
fn expect(program: &Program, expected: &str) {
    let output = program.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn native_frame_keeps_locals_loops_branches_and_multiple_awaits() {
    let program = Program::new(
        r#"
        class App {
            async int receive(Channel<int> inbox, int offset) {
                var sum = offset var index = 0
                while (index < 3) {
                    final value = inbox.receive().await()
                    if (value > 0) { sum = sum + value } else { sum = sum - value }
                    index = index + 1
                }
                return sum
            }
            start() {
                final inbox = Channel<int>(3) final task = this.receive(inbox, 10)
                final a = inbox.send(1).await() final b = inbox.send(-2).await() final c = inbox.send(3).await()
                print(task.await())
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_App_receive"));
    assert!(program.llvm.contains("call i8* @ube_task_spawn_poll("));
    expect(&program, "16\n");
}

#[test]
fn native_frame_cancellation_runs_yielding_finally_before_completion() {
    let program = Program::new(
        r#"
        class App {
            async int work(Channel<int> ready, Channel<int> blocked, Channel<int> done) {
                try { final sent = ready.send(1).await() return blocked.receive().await() }
                finally { final sent = done.send(7).await() }
            }
            start() {
                final ready = Channel<int>(1) final blocked = Channel<int>(1) final done = Channel<int>(1)
                final task = this.work(ready, blocked, done) final started = ready.receive().await()
                final cancelled = task.cancel()
                try { final value = task.await() } catch (Error error) { print(error.message()) }
                print(done.receive().timeout(1000).await())
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_App_work"));
    expect(&program, "task was cancelled\n7\n");
}

#[test]
fn blocking_calls_keep_the_legacy_backend() {
    let program = Program::new(
        r#"
        class App {
            async int work(Channel<int> inbox) { final value = inbox.receive().await() print(value) return value + 1 }
            start() { final inbox = Channel<int>(1) final task = this.work(inbox) final sent = inbox.send(41).await() print(task.await()) }
        }
    "#,
    );
    assert!(!program
        .llvm
        .contains("define internal i1 @__ube_poll_App_work"));
    expect(&program, "41\n42\n");
}

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[cfg(target_os = "linux")]
fn threads(pid: u32) -> usize {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("Threads:"))
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

#[cfg(target_os = "linux")]
#[test]
fn thousands_of_suspended_methods_use_bounded_worker_threads() {
    let program = Program::new(
        r#"
        class App {
            async int work(Channel<int> ready, Channel<int> gate) {
                final sent = ready.send(1).await()
                final value = gate.receive().await()
                return value + 1
            }
            start() {
                final ready = Channel<int>(2048) final gate = Channel<int>(2048)
                final warm = this.work(ready, gate) final started = ready.receive().await()
                final sent = gate.send(1).await() final warmed = warm.await()
                print('warm') final begin = Console.readLine()
                final tasks = [] var index = 0
                while (index < 2048) { tasks.add(this.work(ready, gate)) index = index + 1 }
                index = 0
                while (index < 2048) { final started = ready.receive().await() index = index + 1 }
                print('parked') final release = Console.readLine()
                index = 0
                while (index < 2048) { final sent = gate.send(1).await() index = index + 1 }
                final results = Async.all(tasks).await()
                var total = 0 index = 0
                while (index < 2048) { total = total + results[index] index = index + 1 }
                print(total)
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_App_work"));
    let mut child = Running(
        Command::new(&program.executable)
            .env("TOKIO_WORKER_THREADS", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (send, recv) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if send.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    assert_eq!(recv.recv_timeout(Duration::from_secs(20)).unwrap(), "warm");
    let baseline = threads(child.0.id());
    writeln!(child.0.stdin.as_mut().unwrap()).unwrap();
    assert_eq!(
        recv.recv_timeout(Duration::from_secs(20)).unwrap(),
        "parked"
    );
    let suspended = threads(child.0.id());
    assert!(
        suspended <= baseline + 2,
        "worker growth: {baseline} -> {suspended}"
    );
    writeln!(child.0.stdin.as_mut().unwrap()).unwrap();
    assert_eq!(recv.recv_timeout(Duration::from_secs(20)).unwrap(), "4096");
    let status = child.0.wait().unwrap();
    reader.join().unwrap();
    assert!(status.success());
    println!("2048 suspended methods: {baseline} baseline threads, {suspended} suspended threads");
}

#[test]
fn suspended_references_remain_rooted_and_are_released_on_completion() {
    let program = Program::new(
        r#"
        class Payload(int value)
        class App(var Payload? current = null) {
            App setup() { return App(Payload(7)) }
            async int work(Channel<int> ready, Channel<int> gate) {
                final saved = this._current
                final sent = ready.send(1).await()
                final released = gate.receive().await()
                if (saved != null) { return 1 } return 0
            }
            start() {
                final holder = this.setup()
                final ready = Channel<int>(1) final gate = Channel<int>(1)
                final task = holder.work(ready, gate) final started = ready.receive().await()
                final before = Gc.objects()
                holder.current(null) Gc.collect()
                print(Gc.objects() == before)
                final released = gate.send(1).await() print(task.await())
                Gc.collect() print(Gc.objects() == before - 1)
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_App_work"));
    expect(&program, "true\n1\ntrue\n");
}

#[test]
fn instruction_budget_allows_another_task_to_win_on_one_worker() {
    let program = Program::new(
        r#"
        class App {
            async int spin(Channel<int> ready, Channel<int> gate) {
                final sent = ready.send(1).await() final started = gate.receive().await()
                var index = 0 while (index < 100000) { index = index + 1 }
                return index
            }
            async int quick(Channel<int> gate) { final started = gate.receive().await() return 7 }
            start() {
                final ready = Channel<int>(1) final slowGate = Channel<int>(1) final fastGate = Channel<int>(1)
                final slow = this.spin(ready, slowGate) final started = ready.receive().await()
                final quick = this.quick(fastGate) final winner = Async.race([slow, quick])
                final startSlow = slowGate.send(1).await() final startFast = fastGate.send(1).await()
                print(winner.timeout(2000).await())
                try { final value = slow.await() } catch (Error error) { print(error.message()) }
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_App_spin"));
    expect(&program, "7\ntask was cancelled\n");
}

#[test]
fn nullable_results_and_closed_channel_handlers_resume_correctly() {
    let program = Program::new(
        r#"
        class App {
            async int? work(Channel<int?> inbox) { return inbox.receive().await() }
            async int closed(Channel<int> inbox) {
                try { return inbox.receive().await() } catch (Error error) { return -1 }
            }
            start() {
                final inbox = Channel<int?>(1)
                final one = this.work(inbox) final sent = inbox.send(null).await() print(one.await())
                final two = this.work(inbox) final zero = inbox.send(0).await() print(two.await())
                final closed = Channel<int>(1) closed.close() print(this.closed(closed).await())
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_App_work"));
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_App_closed"));
    expect(&program, "null\n0\n-1\n");
}

#[test]
fn logging_context_survives_suspension_without_leaking_to_main() {
    let program = Program::new(
        r#"
        class App {
            async string? work(Channel<int> ready, Channel<int> gate) {
                Log.put('trace', 'child') final sent = ready.send(1).await()
                final released = gate.receive().await() return Log.get('trace')
            }
            start() {
                Log.put('trace', 'parent')
                final ready = Channel<int>(1) final gate = Channel<int>(1)
                final task = this.work(ready, gate) final started = ready.receive().await()
                Log.put('trace', 'parent-changed') final released = gate.send(1).await()
                print(task.await()) print(Log.get('trace'))
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_App_work"));
    expect(&program, "child\nparent-changed\n");
}

#[test]
fn async_expression_uses_the_same_resumable_frame_for_captures() {
    let program = Program::new(
        r#"
        class App {
            start() {
                final inbox = Channel<int>(1)
                final task = Async.start(() -> (inbox.receive().await()))
                final sent = inbox.send(42).await()
                print(task.await())
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_UbeAsyncExpr"));
    expect(&program, "42\n");
}

#[test]
fn async_trace_links_nested_submissions_awaits_and_legacy_callbacks() {
    let program = Program::new(r#"
    class App {
        async leaf(Channel<int> input) { final result=input.receive().await() }
        async middle(Channel<int> input) { this.leaf(input).await() }
        async legacyLeaf() { final exists=File.exists('/tmp') throw 'legacy failure' }
        async legacyMiddle() { this.legacyLeaf().await() }
        start() {
            final input=Channel<int>(1)
            final task=this.middle(input)
            input.close()
            try { task.await() } catch(Error error) { print(error.stacktrace()) }
            print('NEXT')
            try { this.legacyMiddle().await() } catch(Error error) { print(error.stacktrace()) }
        }
    }"#);
    assert!(program.llvm.contains("define internal i1 @__ube_poll_App_leaf"));
    assert!(!program.llvm.contains("define internal i1 @__ube_poll_App_legacyLeaf"));
    let output=program.run();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text=String::from_utf8_lossy(&output.stdout);
    let (first,second)=text.split_once("NEXT\n").unwrap();
    for (trace,leaf,parent) in [(first,"App.leaf","App.middle"),(second,"App.legacyLeaf","App.legacyMiddle")] {
        assert!(trace.contains(leaf) && trace.contains(parent) && trace.contains("App.start"), "{trace}");
        assert!(trace.contains("--- spawned at ---"), "{trace}");
        assert!(trace.matches("--- awaited at ---").count()>=2, "{trace}");
    }
    assert!(!second.contains("App.middle"), "{second}");
    assert!(!second.contains("App.leaf"), "{second}");
}

#[test]
fn async_trace_repeated_awaits_keep_separate_locations_and_rethrow_identity() {
    let program=Program::new(r#"class App {
        async leaf() { throw 'shared failure' }
        start() {
            final task=this.leaf()
            try { task.await() } catch(Error error) { print(error.stacktrace()) }
            print('NEXT')
            try { task.await() } catch(Error error) {
                final trace=error.stacktrace()
                print(trace)
                try { throw error } catch(Error again) { print(trace==again.stacktrace()) }
            }
        }
    }"#);
    let output=program.run();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text=String::from_utf8_lossy(&output.stdout);
    let (first,second)=text.split_once("NEXT\n").unwrap();
    assert!(first.contains("App.start (7:"), "{first}");
    assert!(second.contains("App.start (9:"), "{second}");
    assert!(!second.contains("App.start (7:"), "{second}");
    assert_eq!(first.matches("--- awaited at ---").count(),1,"{first}");
    assert_eq!(second.matches("--- awaited at ---").count(),1,"{second}");
    assert!(second.ends_with("true\n"),"{second}");
}

#[test]
fn unhandled_async_chain_reports_leaf_submission_and_each_awaiter_in_order() {
    let program = Program::new(r#"
    class App {
        finished() { return }
        async leaf(Channel<int> started, Channel<int> blocked) {
            started.send(1).await()
            blocked.receive().await()
        }
        async relay(Channel<int> started, Channel<int> blocked) {
            this.leaf(started, blocked).await()
        }
        start() {
            this.finished()
            final started=Channel<int>(1)
            final blocked=Channel<int>(1)
            final task=this.relay(started, blocked)
            started.receive().await()
            blocked.close()
            task.await()
        }
    }"#);
    assert!(program.llvm.contains("define internal i1 @__ube_poll_App_leaf"));
    let output = program.run();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unhandled exception GenericError: channel is closed"), "{stderr}");
    let sections: Vec<_> = stderr.split("--- awaited at ---").collect();
    assert_eq!(sections.len(), 3, "{stderr}");
    let origin = sections[0];
    let submitted: Vec<_> = origin.split("--- spawned at ---").collect();
    assert_eq!(submitted.len(), 3, "{stderr}");
    assert!(submitted[0].contains("at App.leaf ("), "{stderr}");
    assert!(submitted[1].contains("at App.relay ("), "{stderr}");
    assert!(submitted[2].contains("at App.start ("), "{stderr}");
    assert!(sections[1].contains("at App.relay ("), "{stderr}");
    assert!(sections[2].contains("at App.start ("), "{stderr}");
    assert!(!sections[2].contains("at App.relay"), "{stderr}");
    assert!(!stderr.contains("App.finished"), "{stderr}");
    for line in stderr.lines().filter(|line| line.trim_start().starts_with("at App.")) {
        let location = line.split_once('(').unwrap().1.trim_end_matches(')');
        let (line, column) = location.split_once(':').unwrap();
        assert!(line.parse::<usize>().unwrap() > 0);
        assert!(column.parse::<usize>().unwrap() > 0);
    }
}
