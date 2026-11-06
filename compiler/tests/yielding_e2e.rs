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
        let source = support::write_source(&temp.path().join("Main.ube"), source);
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
        class Main {
            async int receive(Channel<int> inbox, int offset) {
                var sum = offset var index = 0
                for (index < 3) {
                    final value = await inbox.receive()
                    if (value > 0) { sum = sum + value } else { sum = sum - value }
                    index = index + 1
                }
                return sum
            }
            main() {
                final inbox = Channel<int>(3) final task = this.receive(inbox, 10)
                final a = await inbox.send(1) final b = await inbox.send(-2) final c = await inbox.send(3)
                print(await task)
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_Main_receive"));
    assert!(program.llvm.contains("call i8* @ube_task_spawn_poll("));
    expect(&program, "16\n");
}

#[test]
fn native_frame_cancellation_runs_yielding_finally_before_completion() {
    let program = Program::new(
        r#"
        class Main {
            async int work(Channel<int> ready, Channel<int> blocked, Channel<int> done) {
                try { final sent = await ready.send(1) return await blocked.receive() }
                finally { final sent = await done.send(7) }
            }
            main() {
                final ready = Channel<int>(1) final blocked = Channel<int>(1) final done = Channel<int>(1)
                final task = this.work(ready, blocked, done) final started = await ready.receive()
                final cancelled = task.cancel()
                try { final value = await task } catch (Error error) { print(error.message()) }
                print(await done.receive().timeout(1000))
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_Main_work"));
    expect(&program, "task was cancelled\n7\n");
}

#[test]
fn blocking_calls_keep_the_legacy_backend() {
    let program = Program::new(
        r#"
        class Main {
            async int work(Channel<int> inbox) { final value = await inbox.receive() print(value) return value + 1 }
            main() { final inbox = Channel<int>(1) final task = this.work(inbox) final sent = await inbox.send(41) print(await task) }
        }
    "#,
    );
    assert!(!program
        .llvm
        .contains("define internal i1 @__ube_poll_Main_work"));
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
        class Main {
            async int work(Channel<int> ready, Channel<int> gate) {
                final sent = await ready.send(1)
                final value = await gate.receive()
                return value + 1
            }
            main() {
                final ready = Channel<int>(2048) final gate = Channel<int>(2048)
                final warm = this.work(ready, gate) final started = await ready.receive()
                final sent = await gate.send(1) final warmed = await warm
                print('warm') final begin = Console.readLine()
                final tasks = [] var index = 0
                for (index < 2048) { tasks.add(this.work(ready, gate)) index = index + 1 }
                index = 0
                for (index < 2048) { final started = await ready.receive() index = index + 1 }
                print('parked') final release = Console.readLine()
                index = 0
                for (index < 2048) { final sent = await gate.send(1) index = index + 1 }
                final results = await Task.all(tasks)
                var total = 0 index = 0
                for (index < 2048) { total = total + results[index] index = index + 1 }
                print(total)
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_Main_work"));
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
        class Payload(int value) {}
        class Main(var Payload? current = null) {
            Main setup() { return Main(Payload(7)) }
            async int work(Channel<int> ready, Channel<int> gate) {
                final saved = this._current
                final sent = await ready.send(1)
                final released = await gate.receive()
                if (saved != null) { return 1 } return 0
            }
            main() {
                final holder = this.setup()
                final ready = Channel<int>(1) final gate = Channel<int>(1)
                final task = holder.work(ready, gate) final started = await ready.receive()
                final before = Gc.objects()
                holder.current(null) Gc.collect()
                print(Gc.objects() == before)
                final released = await gate.send(1) print(await task)
                Gc.collect() print(Gc.objects() == before - 1)
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_Main_work"));
    expect(&program, "true\n1\ntrue\n");
}

#[test]
fn instruction_budget_allows_another_task_to_win_on_one_worker() {
    let program = Program::new(
        r#"
        class Main {
            async int spin(Channel<int> ready, Channel<int> gate) {
                final sent = await ready.send(1) final started = await gate.receive()
                var index = 0 for (index < 100000) { index = index + 1 }
                return index
            }
            async int quick(Channel<int> gate) { final started = await gate.receive() return 7 }
            main() {
                final ready = Channel<int>(1) final slowGate = Channel<int>(1) final fastGate = Channel<int>(1)
                final slow = this.spin(ready, slowGate) final started = await ready.receive()
                final quick = this.quick(fastGate) final winner = Task.race([slow, quick])
                final startSlow = await slowGate.send(1) final startFast = await fastGate.send(1)
                print(await winner.timeout(2000))
                try { final value = await slow } catch (Error error) { print(error.message()) }
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_Main_spin"));
    expect(&program, "7\ntask was cancelled\n");
}

#[test]
fn nullable_results_and_closed_channel_handlers_resume_correctly() {
    let program = Program::new(
        r#"
        class Main {
            async int? work(Channel<int?> inbox) { return await inbox.receive() }
            async int closed(Channel<int> inbox) {
                try { return await inbox.receive() } catch (Error error) { return -1 }
            }
            main() {
                final inbox = Channel<int?>(1)
                final one = this.work(inbox) final sent = await inbox.send(null) print(await one)
                final two = this.work(inbox) final zero = await inbox.send(0) print(await two)
                final closed = Channel<int>(1) closed.close() print(await this.closed(closed))
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_Main_work"));
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_Main_closed"));
    expect(&program, "null\n0\n-1\n");
}

#[test]
fn logging_context_survives_suspension_without_leaking_to_main() {
    let program = Program::new(
        r#"
        class Main {
            async string? work(Channel<int> ready, Channel<int> gate) {
                Log.put('trace', 'child') final sent = await ready.send(1)
                final released = await gate.receive() return Log.get('trace')
            }
            main() {
                Log.put('trace', 'parent')
                final ready = Channel<int>(1) final gate = Channel<int>(1)
                final task = this.work(ready, gate) final started = await ready.receive()
                Log.put('trace', 'parent-changed') final released = await gate.send(1)
                print(await task) print(Log.get('trace'))
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_Main_work"));
    expect(&program, "child\nparent-changed\n");
}

#[test]
fn async_expression_uses_the_same_resumable_frame_for_captures() {
    let program = Program::new(
        r#"
        class Main {
            main() {
                final inbox = Channel<int>(1)
                final task = async (await inbox.receive())
                final sent = await inbox.send(42)
                print(await task)
            }
        }
    "#,
    );
    assert!(program
        .llvm
        .contains("define internal i1 @__ube_poll_UbeAsyncExpr"));
    expect(&program, "42\n");
}
