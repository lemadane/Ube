//! First-class Func values through the real compiler and native runtime.
mod support;
use std::{fs, process::Command};
use ube_compiler::{Lexer, Parser, TypeChecker};

fn runs(source: &str, expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    let entry = support::write_source(&dir.path().join("App.ube"), source);
    fs::write(dir.path().join(".gitignore"), ".build/\n").unwrap();
    let executable = dir.path().join("program");
    let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
        .arg("build")
        .arg(entry)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new("timeout")
        .arg("30s")
        .arg(executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn returned_functions_capture_values_and_support_higher_order_calls() {
    runs(
        r#"
class Boxed((Func(int) -> int) transform)
class App {
    (Func(int) -> int) makeAdder(int amount) {
        return (int value) -> value + amount
    }
    int apply((Func(int) -> int) operation, int value) { return operation(value) }
    (Func(int) -> (Func(int) -> int)) curried() {
        return (int first) -> (int second) -> first + second
    }
    static Func(decimal) -> decimal scale(decimal factor) {
        return (decimal value) -> factor * value
    }
    start() {
        final addFive = this.makeAdder(5)
        Console.printLine(addFive(10))
        Console.printLine(this.apply(addFive, 20))
        Console.printLine(this.makeAdder(7)(3))
        Console.printLine(this.curried()(8)(9))
        (Func(int) -> Func(int) -> int) groupedCurried = this.curried()
        Console.printLine(groupedCurried(3)(4))
        Console.printLine(static.scale(0.5)(8))
        var amount = 2
        final snapshot = () -> amount
        amount = 99
        Gc.collect()
        Console.printLine(snapshot())
        (Func(int) -> int) typed = (int value) -> value * 2
        final boxed = Boxed(typed)
        Gc.collect()
        Console.printLine(boxed.transform()(6))
        Func(string) -> void output = (string text) -> Console.printLine(text)
        output('done')
        final wide = (long value) -> value
        Console.printLine(wide(9007199254740993L))
        final inverse = (bool value) -> not value
        Console.printLine(inverse(false))
        final nullable = (int? value) -> value
        Console.printLine(nullable(null) == null)
        Console.printLine(nullable(0) ?? 9)
        final formatted = () -> 'amount=${amount}'
        Console.printLine(formatted())
        (
            Func(bool) -> int
        ) choose = (bool flag) -> if (flag) { yield 1 } else { yield 2 }
        Console.printLine(choose(false))
    }
}
"#,
        "15\n25\n10\n17\n7\n4\n2\n12\ndone\n9007199254740993\ntrue\ntrue\n0\namount=99\n2\n",
    );
}

#[test]
fn returned_functions_defer_tasks_and_preserve_errors_and_captures() {
    runs(
        r#"
class App(var int calls) {
    async int incrementMany(Cache cache, int count) {
        cache.increment('count', count)
        return count
    }
    Func() -> Async<int> makeIncrement(Cache cache, int count) {
        return () -> this.incrementMany(cache, count)
    }
    int fail() { throw 'closure failure' }
    Func() -> int failure() { return () -> this.fail() }
    Func() -> int capturedObject() {
        final value = Holder(42)
        return () -> value.value()
    }
    start() {
        final cache = Cache.memory(4096)
        cache.set('count', '0', 0)
        final first = this.makeIncrement(cache, 100)
        final second = this.makeIncrement(cache, 100)
        Console.printLine(cache.get('count') ?? 'missing')
        Console.printLine(await first())
        Console.printLine(await second())
        Console.printLine(await first())
        Console.printLine(cache.get('count') ?? 'missing')
        final read = this.capturedObject()
        Gc.collect()
        Console.printLine(read())
        final fail = this.failure()
        try { fail() } catch (Error error) { Console.printLine(error.message()) }
        final deferred = async (() -> 7)
        final seven = await deferred
        Console.printLine(seven())
        Func() -> Async<void> deferredOutput = () -> async Console.printLine('async void')
        await deferredOutput()
        cache.close()
    }
}

class Holder(int value)
"#,
        "0\n100\n100\n100\n300\n42\nclosure failure\n7\nasync void\n",
    );
}

#[test]
fn async_start_runs_an_ordinary_method_and_await_returns_its_value() {
    runs(
        r#"
class App {
    int incrementMany(Cache cache, int count) {
        cache.increment('count', count)
        return count
    }
    int fail() { throw 'started failure' }
    start() {
        final cache = Cache.memory(4096)
        cache.set('count', '0', 0)
        final first = Async.start(() -> this.incrementMany(cache, 100))
        final operation = () -> this.incrementMany(cache, 200)
        final second = Async.start(operation)
        Console.printLine(first.await())
        Console.printLine(second.await())
        Console.printLine(cache.get('count') ?? 'missing')
        try {
            Async.start(() -> this.fail()).await()
        } catch (Error error) {
            Console.printLine(error.message())
        }
        cache.close()
    }
}
"#,
        "100\n200\n300\nstarted failure\n",
    );
}

#[test]
fn async_construction_batch_and_status_apis_run() {
    runs(
        r#"
class App {
    start() {
        final completed = Async.completed('ready')
        Console.printLine(completed.isCompleted())
        Console.printLine(completed.isCancelled())
        Console.printLine(completed.await())

        final batch = Async.startAll([() -> 10, () -> 20])
        Console.printLine(batch.await().len())

        Async.delay(1).await()
        Console.printLine('delayed')

        final pending = Async.delay(10000)
        Console.printLine(pending.isCompleted())
        Console.printLine(pending.cancel())
        Console.printLine(pending.isCancelled())

        try {
            Async.failed(GenericError('expected failure')).await()
        } catch (Error error) {
            Console.printLine(error.message())
        }
    }
}
"#,
        "true\nfalse\nready\n2\ndelayed\nfalse\ntrue\ntrue\nexpected failure\n",
    );
}
#[test]
fn invalid_function_signatures_and_calls_are_diagnosed() {
    for (source, expected) in [
        ("class App { start() { final f = (int n) -> n f() } }", "Func expects 1 arguments"),
        ("class App { start() { Async.start(1) } }", "zero-argument Func"),
        ("class App { start() { Async.start((int value) -> value) } }", "zero-argument Func"),
        ("class App { start() { Async.startAll([() -> 1, 2]) } }", "zero-argument Func values"),
        ("class App { start() { Async.completed() } }", "expects one value"),
        ("class App { start() { Async.failed(1) } }", "expects one Error"),
        ("class App { start() { Async.delay('later') } }", "milliseconds"),
        ("class App { start() { Async.delay(1).isCompleted(1) } }", "expects no arguments"),
        ("class App { start() { (Func() -> int) = () -> 1 } }", "variable name after parenthesized Func type"),
        ("class App { start() { final f = () -> 1 f(2) } }", "Func expects 0 arguments"),
        ("class App { start() { final f = (int n) -> n f('wrong') } }", "Func argument 1"),
        ("class App { start() { final value = 1 value() } }", "expected a Func value"),
        ("class App { start() { Func() -> int missing } }", "requires an explicit initializer"),
        ("class App { Func() -> int wrong() { return () -> 'wrong' } }", "Mismatched return type"),
        ("class App { start() { var n = 1 final f = () -> n = 2 } }", "immutable variable"),
        ("class App { start() { final f = (int n, int n) -> n } }", "Duplicate Func parameter"),
        ("class App { start() { final f = (void n) -> 1 } }", "cannot have type void"),
        ("class App { use(Func(void) -> int f) {} }", "cannot have type void"),
        ("class Func", "reserved for function types"),
        ("class App { Func() -> int wrong() { return Console.printLine('wrong') } }", "Mismatched return type"),
        ("class App { start() { thisMissing() } }", "Unqualified method calls require"),
        ("class App { async int work() { return 1 } start() { final pending = this.work() pending() } }", "expected a Func value"),
    ] {
        let mut parser = Parser::new(Lexer::new(source));
        let program = parser.parse_program();
        let mut errors = parser.diagnostics;
        if errors.is_empty() {
            let mut checker = TypeChecker::new();
            checker.check_program(&program);
            errors.extend(checker.diagnostics);
        }
        assert!(errors.iter().any(|error| error.message.contains(expected)), "{source}: expected {expected}, got {errors:?}");
    }
}

#[test]
fn closures_do_not_retain_unrelated_locals_or_previous_functions() {
    runs(
        r#"
class Holder(int value)
class App {
    start() {
        var operation = () -> 0
        var index = 0
        for (index < 1000) {
            final unrelated = Holder(index)
            operation = () -> 7
            index = index + 1
        }
        Gc.collect()
        if (Gc.objects() > 50) { throw 'closures retained unused locals' }
        Console.printLine(operation())
    }
}
"#,
        "7\n",
    );
}

#[test]
fn documented_function_example_runs() {
    runs(
        include_str!("../../examples/functions/App.ube"),
        "15\n100\n100\n200\n",
    );
}

#[test]
fn async_await_methods_preserve_eager_execution_types_and_failures() {
    runs(
        r#"
class App {
    async int exchange(Channel<int> outgoing, Channel<int> incoming, int value) {
        outgoing.send(value).await()
        return incoming.receive().await()
    }
    async int fail() { throw 'async failed' }
    start() {
        final left = Channel<int>(1)
        final right = Channel<int>(1)
        Async<int> first = this.exchange(left, right, 10)
        Async<int> second = this.exchange(right, left, 20)
        Console.printLine(first.await())
        Console.printLine(second.await())
        Console.printLine(first.await())
        final deferred = () -> this.exchange(left, right, 30)
        final third = deferred()
        final fourth = this.exchange(right, left, 40)
        Console.printLine(Async.all([third, fourth]).await().len())
        try { this.fail().await() } catch (Error error) { Console.printLine(error.message()) }
        left.close()
        right.close()
    }
}
"#,
        "20\n10\n20\n2\nasync failed\n",
    );
}

#[test]
fn async_await_methods_reject_invalid_receivers_and_arguments() {
    for (source, expected) in [
        (
            "class App { start() { 1.await() } }",
            ".await() requires an Async",
        ),
        (
            "class App { start() { final f = () -> 1 f.await() } }",
            ".await() requires an Async",
        ),
        ("class App { start() { 1.await(2) } }", "takes no arguments"),
        ("class App { start() { 1.await } }", "after .await"),
        (
            "class App(Task<int> result)",
            "Task has been renamed to Async",
        ),
    ] {
        let mut parser = Parser::new(Lexer::new(source));
        let program = parser.parse_program();
        let mut errors = parser.diagnostics;
        if errors.is_empty() {
            let mut checker = TypeChecker::new();
            checker.check_program(&program);
            errors.extend(checker.diagnostics);
        }
        assert!(
            errors.iter().any(|error| error.message.contains(expected)),
            "{source}: {errors:?}"
        );
    }
}

#[test]
fn java_style_block_lambdas_capture_return_and_run_as_tasks() {
    runs(r#"
class App {
    (Func(int) -> int) makeAdder(int amount) {
        return (int value) -> {
            final result = value + amount
            return result
        }
    }
    start() {
        var offset = 3
        final calculate = (int value) -> {
            var result = value + offset
            if (value < 0) { return 0 }
            result = result * 2
            return result
        }
        offset = 100
        Gc.collect()
        Console.printLine(calculate(4))
        Console.printLine(calculate(-1))
        Console.printLine(this.makeAdder(5)(10))
        final nested = (int first) -> {
            return (int second) -> { return first + second }
        }
        Console.printLine(nested(7)(8))
        final output = (string text) -> { Console.printLine(text) return }
        output('void')
        final cleanup = () -> {
            try { return 42 } finally { Console.printLine('cleanup') }
        }
        Console.printLine(cleanup())
        final failure = () -> { throw 'block failure' }
        try { failure() } catch (Error error) { Console.printLine(error.message()) }
        Console.printLine(Async.start(() -> { final value = 6 return value * 7 }).await())
        final choose = (bool enabled) -> {
            if (enabled) { return 'yes' } else { return 'no' }
        }
        Console.printLine(choose(false))
    }
}
"#, "14\n0\n15\n15\nvoid\ncleanup\n42\nblock failure\n42\nno\n");
}

#[test]
fn java_style_block_lambdas_reject_invalid_returns_and_scope_escapes() {
    for (body, expected) in [
        ("final f = (bool flag) -> { if (flag) { return 1 } }", "every path"),
        ("final f = (bool flag) -> { if (flag) { return 1 } return 'no' }", "same type"),
        ("final f = (bool flag) -> { if (flag) { return 1 } return }", "same type"),
        ("final f = (int x, int x) -> { return x }", "Duplicate Func parameter"),
        ("var x = 1 final f = () -> { x = 2 }", "immutable"),
        ("final f = () -> { break }", "inside a loop"),
        ("final f = () -> { return unknown }", "Undeclared"),
        ("final f = () -> { return Console.printLine('no') }", "bare return"),
        ("final f = (int x) -> { x = 2 return x }", "immutable"),
    ] {
        let source = format!("class App {{ start() {{ {body} }} }}");
        let mut parser = Parser::new(Lexer::new(&source));
        let program = parser.parse_program();
        assert!(parser.diagnostics.is_empty(), "{:?}", parser.diagnostics);
        let mut checker = TypeChecker::new();
        checker.check_program(&program);
        assert!(checker.diagnostics.iter().any(|error| error.message.contains(expected)),
            "{source}: {:?}", checker.diagnostics);
    }
}

#[test]
fn kotlin_style_lambdas_are_rejected_with_migration_guidance() {
    for statement in [
        "server.get('/') { request -> return Response.ok('old') }",
        "server.get('/')\n{ request -> return Response.ok('old') }",
        "server.get { request -> return Response.ok('old') }",
        "server.use('/api') { request -> Console.printLine('old') }",
        "server.websocket('/chat') { socket -> socket.close() }",
        "socket.onMessage { message -> Console.printLine(message) }",
        "callback() { value -> Console.printLine(value) }",
        "server.get('/', { request -> return Response.ok('old') })",
    ] {
        let source = format!("class App {{ start() {{ {statement} }} }}");
        let mut parser = Parser::new(Lexer::new(&source));
        parser.parse_program();
        assert!(parser.diagnostics.iter().any(|error|
            error.message.contains("Kotlin-style lambdas are not supported")
                && error.message.contains("(Request request) ->")),
            "{statement}: {:?}", parser.diagnostics);
    }
}
