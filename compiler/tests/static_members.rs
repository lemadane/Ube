//! These are native execution tests: parse/typecheck/emit, verify with clang, then run.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
use ube_compiler::{IrBuilder, Lexer, Parser, TextualLlvmEmitter, TypeChecker};

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

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn native(source: &str) -> Output {
    native_with_input(source, b"")
}
fn native_with_input(source: &str, input: &[u8]) -> Output {
    let mut parser = Parser::new(Lexer::new(source));
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "{:?}", parser.diagnostics);
    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program), "{:?}", checker.diagnostics);
    let mut builder = IrBuilder::new();
    let program = builder.build_program(&program);
    assert!(builder.diagnostics.is_empty(), "{:?}", builder.diagnostics);
    let llvm = TextualLlvmEmitter::new().emit_program(&program);
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = Scratch(std::env::temp_dir().join(format!(
        "ube-statics-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&dir.0).unwrap();
    let ll = dir.0.join("test.ll");
    let exe = dir.0.join("test");
    fs::write(&ll, llvm).unwrap();
    let runtime = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("libube_runtime.a");
    assert!(
        runtime.exists(),
        "Missing runtime static library: {}",
        runtime.display()
    );
    let compiled = Command::new("clang")
        .arg("-Wno-override-module")
        .arg("-O1")
        .arg(&ll)
        .arg(runtime)
        .args(["-ldl", "-lpthread", "-lm"])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("clang is required for native tests");
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("timeout")
        .arg("15s")
        .arg(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}
fn runs(source: &str, expected: &str) {
    let output = native(source);
    assert!(
        output.status.success(),
        "status {:?}, stderr {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}
fn rejects(source: &str, expected: &str) {
    let errors = diagnostics(source);
    assert!(
        errors.iter().any(|e| e.contains(expected)),
        "Expected {expected:?}, got {errors:?}"
    );
}

#[test]
fn shared_fields_and_explicit_receivers() {
    runs(
        r#"
    class Counter {
        static var int total = 0
        static final int limit = 100
        static int current() { return static.total }
        static increment() { static.total = static.current() + 1 }
        tick() { static.increment() }
    }
    class Other { static var int total = 50 }
    class App { start() {
        final a = Counter() final b = Counter()
        a.tick() b.tick()
        print(Counter.current()) print(Counter.limit)
        Counter.total = 10
        print(Counter.current()) print(Other.total)
    } }
    "#,
        "2\n100\n10\n50\n",
    );
}

#[test]
fn initialization_once_defaults_and_factories() {
    runs(
        r#"
    class Item(int value)
    class State {
        static var int count
        static final Item item = static.make()
        static final int next = static.item.value() + 1
        static Item make() { static.count = static.count + 1 return Item(41) }
    }
    class App { start() {
        final a = State() final b = State()
        print(State.count) print(State.item.value()) print(State.next)
    } }
    "#,
        "1\n41\n42\n",
    );
}

#[test]
fn reference_slots_survive_collection_and_replacement() {
    runs(
        r#"
    class Item(string value)
    class State {
        static var Item? item = Item('first')
        static set() { static.item = Item('second') }
        static string read() { return (static.item ?? Item('missing')).value() }
    }
    class App { start() {
        Gc.collect() print(State.read())
        State.set() Gc.collect() print(State.read())
        State.item = null Gc.collect() print(State.read())
    } }
    "#,
        "first\nsecond\nmissing\n",
    );
}

#[test]
fn nullable_and_numeric_static_storage() {
    runs(
        r#"
    class State {
        static var int? maybe = null
        static var decimal value = 1.5
        static var long wide = 5000000000L
        static final bool enabled = true
        static var string text = 'hello'
        static fill() { static.maybe ??= 7 static.maybe ??= 9 }
    }
    class App { start() {
        print(State.maybe) State.fill() print(State.maybe)
        print(State.value) print(State.wide) print(State.enabled) print(State.text)
        State.value = 2.5 State.text = 'world' print(State.value) print(State.text)
    } }
    "#,
        "null\n7\n1.5\n5000000000\ntrue\nhello\n2.5\nworld\n",
    );
}

#[test]
fn static_async_methods_and_await() {
    runs(
        r#"
    class Work {
        static final int answer = 42
        static async int first() { return static.answer }
        static async int second() { final result = await static.first() return result + 1 }
    }
    class App { start() { print(await Work.second()) } }
    "#,
        "43\n",
    );
}

#[test]
fn static_start_needs_no_app_instance() {
    runs("class App(App recursive) { static final int answer = 42 static start() { print(static.answer) } }", "42\n");
}

#[test]
fn static_exceptions_propagate() {
    runs(
        r#"
    class Failure { static fail() { throw 'boom' } }
    class App { start() { try { Failure.fail() } catch (Error e) { print(e.message()) } } }
    "#,
        "boom\n",
    );
    let output = native("class Failure { static final int value = static.fail() static int fail() { throw 'init failed' } } class App { start() { print('should not run') } }");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("init failed"));
}

#[test]
fn reject_invalid_static_access() {
    rejects(
        "class C { static var int n = 0 int bad() { return n } }",
        "must be accessed through 'static.'",
    );
    rejects(
        "class C { static var int n = 0 int bad() { return C.n } }",
        "must be accessed through 'static.'",
    );
    rejects(
        "class C { static var int n = 0 int bad() { return this.n } }",
        "static member",
    );
    rejects("class C { static var int n = 0 } class App { start() { final c = C() print(c.n) } }", "static member");
    rejects(
        "class C { static final int n = 0 change() { static.n = 1 } }",
        "immutable static field",
    );
    rejects(
        "class C { static final int? n = null change() { static.n ??= 1 } }",
        "immutable static field",
    );
    rejects(
        "class C { static int f() { return this.value() } int value() { return 1 } }",
        "'this' is unavailable",
    );
    rejects(
        "class C(int n) { static int f() { return static.n } }",
        "not a static member",
    );
    rejects(
        "class C { int f() { return 1 } } class App { start() { print(C.f()) } }",
        "not a static member",
    );
    rejects(
        "class C { static int f(int n) { return n } int bad() { return f(1) } }",
        "Unqualified method calls require",
    );
    rejects("class C { static int f(int n) { return n } } class App { start() { print(C.f()) } }", "Invalid arguments");
    rejects("class C { static int f(int n) { return n } } class App { start() { print(C.f('bad')) } }", "Invalid arguments");
    rejects(
        "class C { static final int n = 'wrong' }",
        "Static field 'n' expects",
    );
    rejects(
        "class C { static final int n = this.value() int value() { return 1 } }",
        "'this' is unavailable",
    );
    rejects("class C { static final int _secret = 1 } class App { start() { print(C._secret) } }", "private to class");
    rejects("class C { static int _secret() { return 1 } } class App { start() { print(C._secret()) } }", "private to class");
    rejects(
        "class C { static var int n = 0 static var int n = 1 }",
        "Duplicate member",
    );
    rejects(
        "trait T { static int f() }",
        "Traits declare instance methods",
    );
}

#[test]
fn static_methods_do_not_satisfy_instance_traits() {
    rejects("trait Named { string name() } class C { static string name() { return 'class' } } class App { use(Named value) {} start() { this.use(C()) } }", "Expected");
}

#[test]
fn static_fields_work_across_native_async_suspension() {
    runs(
        r#"
    class Work {
        static var int total = 0
        static async int receive(Channel<int> inbox) {
            final value = await inbox.receive()
            static.total = value
            return static.total
        }
    }
    class App { start() {
        final inbox = Channel<int>(1)
        final task = Work.receive(inbox)
        final sent = await inbox.send(42)
        print(await task) print(Work.total)
    } }
    "#,
        "42\n42\n",
    );
}

#[test]
fn concurrent_static_reference_replacement_and_collection() {
    runs(
        r#"
    class Item(int value)
    class State {
        static var Item item = Item(0)
        static async int write() {
            var index = 0
            for (index < 100) {
                static.item = Item(index)
                Gc.collect()
                index = index + 1
            }
            return index
        }
        static async int read() {
            var index = 0
            for (index < 100) {
                final item = static.item
                Gc.collect()
                if (item.value() < 0 or item.value() > 99) { throw 'invalid static object' }
                index = index + 1
            }
            return index
        }
    }
    class App { start() {
        final writer = State.write() final reader = State.read()
        print(await writer) print(await reader)
    } }
    "#,
        "100\n100\n",
    );
}

#[test]
fn replacing_a_static_reference_releases_its_gc_root() {
    runs(
        r#"
    class Item(int value)
    class State {
        static var Item item = Item(1)
        static replace() { static.item = Item(2) }
    }
    class App { start() {
        Gc.collect() final before = Gc.reclaimed()
        State.replace() Gc.collect()
        print(Gc.reclaimed() > before)
        print(State.item.value())
    } }
    "#,
        "true\n2\n",
    );
}

#[test]
fn forward_and_cyclic_initializers_fail_before_application_code() {
    for fields in [
        "static final int first = static.second static final int second = 42",
        "static final int first = static.read() static final int second = static.first static int read() { return static.second }",
    ] {
        let output = native(&format!("class State {{ {fields} }} class App {{ start() {{ print('should not run') }} }}"));
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("before its initializer completed"));
    }
}

#[test]
fn static_fields_can_initialize_generated_mocks_and_trait_values() {
    runs(r#"
    trait Answer { int get() }
    class State {
        static final Answer answer = Mockery.mock(Answer)
        static int read(Answer answer) { return answer.get() }
    }
    class App { start() {
        try { print(State.read(State.answer)) }
        catch (MockeryError e) { print('mock initialized') }
    } }
    "#, "mock initialized\n");
}
