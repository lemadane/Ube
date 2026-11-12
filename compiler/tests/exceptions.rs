//! These are native execution tests: parse/typecheck/emit, verify with clang, then run.
use std::{fs, path::PathBuf, process::{Command, Output}, sync::atomic::{AtomicUsize, Ordering}};
use ube_compiler::{IrBuilder, Lexer, Parser, TextualLlvmEmitter, TypeChecker};

fn diagnostics(source: &str) -> Vec<String> {
    let mut parser = Parser::new(Lexer::new(source));
    let program = parser.parse_program();
    if !parser.diagnostics.is_empty() { return parser.diagnostics.iter().map(|d| d.message.clone()).collect(); }
    let mut checker = TypeChecker::new();
    checker.check_program(&program);
    checker.diagnostics.iter().map(|d| d.message.clone()).collect()
}

struct Scratch(PathBuf);
impl Drop for Scratch { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
fn native(source: &str) -> Output { native_with_input(source, b"") }
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
    let dir = Scratch(std::env::temp_dir().join(format!("ube-exceptions-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))));
    fs::create_dir(&dir.0).unwrap();
    let ll = dir.0.join("test.ll");
    let exe = dir.0.join("test");
    fs::write(&ll, llvm).unwrap();
    let runtime = std::env::current_exe().unwrap().parent().unwrap().join("libube_runtime.a");
    assert!(runtime.exists(), "Missing runtime static library: {}", runtime.display());
    let compiled = Command::new("clang").arg("-Wno-override-module").arg("-O1").arg(&ll).arg(runtime)
        .args(["-ldl", "-lpthread", "-lm"]).arg("-o").arg(&exe).output().expect("clang is required for native tests");
    assert!(compiled.status.success(), "{}", String::from_utf8_lossy(&compiled.stderr));
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("timeout").arg("15s").arg(exe)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}
fn runs(source: &str, expected: &str) {
    let output = native(source);
    assert!(output.status.success(), "status {:?}, stderr {}", output.status.code(), String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}
fn rejects(source: &str, expected: &str) {
    let errors = diagnostics(source);
    assert!(errors.iter().any(|e| e.contains(expected)), "Expected {expected:?}, got {errors:?}");
}

#[test]
fn concrete_catch_preserves_object_fields_and_exact_identity() {
    runs(r#"class A(string message, int code) class B(string message)
    class App { start() { try { throw A('bad', 42) }
      catch (B err) { Console.printLine('wrong') } catch (A err) { Console.printLine(err.message()) Console.printLine(err.code()) }
      catch (Error err) { Console.printLine('wrong general') } } }"#, "bad\n42\n");
}
#[test]
fn strings_are_boxed_once_and_error_trait_dispatches() {
    runs(r#"class App { string message() { Console.printLine('called') return 'boom' }
    start() { try { throw this.message() } catch (Error err) { Console.printLine(err.message()) } } }"#, "called\nboom\n");
}
#[test]
fn structural_category_matches_independent_classes() {
    runs(r#"trait Retryable { string message() bool retryable() }
    class A(string message, bool retryable) class B(string message) { bool retryable() { return true } }
    class App { start() {
      try { throw A('a', false) } catch (Retryable err) { Console.printLine(err.message()) Console.printLine(err.retryable()) }
      try { throw B('b') } catch (Retryable err) { Console.printLine(err.message()) Console.printLine(err.retryable()) }
    } }"#, "a\nfalse\nb\ntrue\n");
}
#[test]
fn independent_traits_match_in_source_order() {
    runs(r#"trait Retryable { string message() bool retryable() }
    trait Security { string message() string securityCode() }
    class Both(string message, bool retryable, string securityCode)
    class App { start() { try { throw Both('both', true, 'AUTH') }
      catch (Security err) { Console.printLine(err.securityCode()) }
      catch (Retryable err) { Console.printLine('wrong') } } }"#, "AUTH\n");
}
#[test]
fn unmatched_errors_propagate_through_calls_and_cleanup_only_try() {
    runs(r#"class A(string message) class B(string message)
    class App { fail() { throw A('boom') } relay() { this.fail() }
    start() { try { try { this.relay() } catch (B err) { Console.printLine('wrong') } finally { Console.printLine('inner') } }
      catch (A err) { Console.printLine(err.message()) } finally { Console.printLine('outer') } } }"#, "inner\nboom\nouter\n");
}
#[test]
fn try_finally_does_not_swallow_errors() {
    runs(r#"class App { start() { try { try { throw 'boom' } finally { Console.printLine('cleanup') } }
      catch (Error err) { Console.printLine(err.message()) } } }"#, "cleanup\nboom\n");
}
#[test]
fn accidental_conformance_requires_explicit_throw() {
    runs(r#"class Customer(string message) class App { start() {
      final customer = Customer('hello') Console.printLine('normal')
      try { throw customer } catch (Customer err) { Console.printLine(err.message()) } } }"#, "normal\nhello\n");
}
#[test]
fn finally_runs_on_normal_completion_and_matching_catch() {
    runs(r#"class App { start() {
      try { Console.printLine('normal') } finally { Console.printLine('first') }
      try { throw 'boom' } catch (Error err) { Console.printLine('handled') } finally { Console.printLine('second') }
    } }"#, "normal\nfirst\nhandled\nsecond\n");
}
#[test]
fn nested_finally_runs_once_for_return_and_catch_return() {
    runs(r#"class App {
      int value() { try { try { return 42 } finally { Console.printLine('inner') } } finally { Console.printLine('outer') } }
      int handled() { try { throw 'boom' } catch (Error err) { return 7 } finally { Console.printLine('catch cleanup') } }
      start() { Console.printLine(this.value()) Console.printLine(this.handled()) }
    }"#, "inner\nouter\n42\ncatch cleanup\n7\n");
}
#[test]
fn loop_exits_cleanup_only_regions_being_exited() {
    runs(r#"class App { start() {
      try { each (x from [1,2]) { try { if (x == 1) { continue } break } finally { Console.printLine(x) } } Console.printLine('done') }
      finally { Console.printLine('outer') }
    } }"#, "1\n2\ndone\nouter\n");
}
#[test]
fn exception_from_handler_runs_finally_before_outer_handler() {
    runs(r#"class App { start() { try {
      try { throw 'first' } catch (Error err) { throw 'second' } finally { Console.printLine('inner') }
    } catch (Error err) { Console.printLine(err.message()) } } }"#, "inner\nsecond\n");
}
#[test]
fn cleanup_error_replaces_pending_error_and_preserves_it_as_suppressed() {
    let output = native(r#"class App { start() { try { throw 'original' } finally { throw 'cleanup' } } }"#);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("GenericError: cleanup"), "{stderr}");
    assert!(stderr.contains("suppressed: original"), "{stderr}");
}
#[test]
fn cleanup_return_overrides_pending_return() {
    runs(r#"class App { int value() { try { return 1 } finally { return 2 } }
      start() { Console.printLine(this.value()) } }"#, "2\n");
}
#[test]
fn composition_and_optional_causes_keep_original_concrete_objects() {
    runs(r#"class Details(string message, string code)
      class Root(string message)
      class PaymentError(Details details, Error? cause = null) {
        string message() { return this._details.message() }
        string code() { return this._details.code() }
      }
      class App { start() {
        final root = Root('root')
        try { throw PaymentError(Details('payment', 'PAY'), root) }
        catch (PaymentError err) {
          Console.printLine(err.message()) Console.printLine(err.code())
          try { throw (err.cause() ?? GenericError('missing')) } catch (Root cause) { Console.printLine(cause.message()) }
        }
      } }"#, "payment\nPAY\nroot\n");
}
#[test]
fn guarded_catches_narrow_before_condition_and_continue_on_false() {
    runs(r#"trait Retryable { string message() bool retryable() }
      class A(string message, bool retryable)
      class App { start() { try { throw A('boom', false) }
        catch (Retryable err) if (err.retryable()) { Console.printLine('wrong') }
        catch (A err) { Console.printLine(err.message()) }
      } }"#, "boom\n");
}
#[test]
fn throwing_catch_condition_propagates_after_cleanup() {
    runs(r#"class A(string message) { bool test() { throw 'condition' } }
      class App { start() { try { try { throw A('original') }
        catch (A err) if (err.test()) { Console.printLine('wrong') } finally { Console.printLine('cleanup') }
      } catch (Error err) { Console.printLine(err.message()) } } }"#, "cleanup\ncondition\n");
}
#[test]
fn async_failures_keep_type_fields_causes_and_finally_order() {
    runs(r#"class DbError(string message, int code)
      class App {  int fail() { try { throw DbError('db', 99) } finally { Console.printLine('worker cleanup') } }
        int relay() { return Async.start(() -> this.fail()).await() }
        start() { try { final value = Async.start(() -> this.relay()).await() } catch (DbError err) { Console.printLine(err.message()) Console.printLine(err.code()) } finally { Console.printLine('awaiter cleanup') } }
      }"#, "worker cleanup\ndb\n99\nawaiter cleanup\n");
}
#[test]
fn timeout_and_closed_channel_failures_are_generic_errors() {
    runs(r#"class App { start() {
      final ch = Channel<int>(1)
      try { final value = ch.receive().timeout(5).await() } catch (GenericError err) { Console.printLine(err.message()) }
      ch.close()
      try { final value = ch.receive().await() } catch (Error err) { Console.printLine(err.message()) }
    } }"#, "task timed out\nchannel is closed\n");
}
#[test]
fn cancellation_unblocks_await_and_runs_worker_cleanup() {
    runs(r#"class App {
      wait(Channel<int> ready, Channel<int> blocked) {
        try { final sent = ready.send(1).await() final value = blocked.receive().await() }
        finally { Console.printLine('worker cleanup') }
      }
      start() { final ready = Channel<int>(1) final blocked = Channel<int>(1)
        final task = Async.start(() -> this.wait(ready, blocked)) final started = ready.receive().await()
        final cancelled = task.cancel()
        try { final value = task.await() } catch (Error err) { Console.printLine(err.message()) }
      }
    }"#, "worker cleanup\ntask was cancelled\n");
}

#[test]
fn cancellation_allows_async_cleanup_and_preserves_original_failure() {
    runs(r#"class App {
      work(Channel<int> ready, Channel<int> blocked, Channel<int> completed) {
        try {
          final sent = ready.send(1).await()
          final value = blocked.receive().await()
        } finally {
          Console.printLine('cleanup started')
          try { final value = blocked.receive().timeout(5).await() }
          catch (Error err) { Console.printLine(err.message()) }
          final sent = completed.send(7).await()
          Console.printLine('cleanup completed')
        }
      }
      start() {
        final ready = Channel<int>(1)
        final blocked = Channel<int>(1)
        final completed = Channel<int>(1)
        final task = Async.start(() -> this.work(ready, blocked, completed))
        final started = ready.receive().await()
        final cancelled = task.cancel()
        try { final value = task.await() }
        catch (Error err) { Console.printLine(err.message()) }
        Console.printLine(completed.receive().timeout(1000).await())
      }
    }"#, "cleanup started\ntask timed out\ncleanup completed\ntask was cancelled\n7\n");
}

#[test]
fn unhandled_sync_and_async_errors_exit_unsuccessfully() {
    for source in [r#"class App { start() { throw 'sync' } }"#,
      r#"class App {  fail() { throw 'async' } start() { final value = Async.start(() -> this.fail()).await() } }"#] {
        let output = native(source);
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("unhandled exception GenericError"));
    }
}
#[test]
fn invalid_throw_operands_are_rejected() {
    for value in ["null", "1", "true", "Potato(1)"] {
        rejects(&format!("class Potato(int value) class App {{ start() {{ throw {value} }} }}"), "throw target must");
    }
    rejects(r#"class Wrong { int message() { return 1 } } class App { start() { throw Wrong() } }"#, "throw target must");
}
#[test]
fn invalid_and_unreachable_catch_types_are_rejected() {
    for ty in ["Missing", "Potato", "NotError"] {
        rejects(&format!("trait NotError {{ bool retryable() }} class Potato(int value) class App {{ start() {{ try {{ Console.printLine(1) }} catch ({ty} err) {{}} }} }}"), "catch type");
    }
    rejects(r#"class A(string message) class App { start() { try { throw A('x') } catch (Error err) {} catch (A err) {} } }"#, "Unreachable catch");
    rejects(r#"trait R { string message() bool retryable() } class A(string message, bool retryable)
      class App { start() { try { throw A('x', true) } catch (R err) {} catch (A err) {} } }"#, "Unreachable catch");
}
#[test]
fn catch_scopes_and_static_types_are_checked() {
    rejects(r#"class A(string message, int code) class App { start() { try { throw A('x',1) } catch (A err) { Console.printLine(err.missing()) } } }"#, "Member");
    rejects(r#"class App { start() { try { throw 'x' } catch (Error err) {} Console.printLine(err) } }"#, "err");
    rejects(r#"class App { start() { try { final local = 1 } finally { Console.printLine(local) } } }"#, "local");
    rejects(r#"class App { start() { try { throw 'x' } catch (Error err) if (1) {} } }"#, "catch condition");
}
#[test]
fn incomplete_try_and_legacy_cleanup_syntax_are_rejected() {
    rejects("class App { start() { try {} } }", "try requires");
    for code in ["guard {}", "after file.close()", "try {} exit {}", "try {} catch(err) {}"] {
        assert!(!diagnostics(&format!("class App {{ start() {{ {code} }} }}")).is_empty(), "accepted {code}");
    }
}

#[test]
fn runtime_file_and_directory_failures_are_catchable_and_run_cleanup() {
    let missing = std::env::temp_dir().join(format!("ube-missing-{}", std::process::id())).join("file");
    let path = missing.to_string_lossy();
    runs(&format!(r#"class App {{ start() {{
      try {{ final value = File.read('{path}') Console.printLine('wrong') }}
      catch (FileNotFoundError err) {{ Console.printLine('read') }} finally {{ Console.printLine('cleanup') }}
      try {{ File.write('{path}', 'data') Console.printLine('wrong') }} catch (Error err) {{ Console.printLine('write') }}
      try {{ File.append('{path}', 'data') }} catch (Error err) {{ Console.printLine('append') }}
      try {{ File.delete('{path}') }} catch (FileNotFoundError err) {{ Console.printLine('delete') }}
      try {{ final info = File.info('{path}') }} catch (FileNotFoundError err) {{ Console.printLine('info') }}
      try {{ final entries = Directory.list('{path}') }} catch (FileNotFoundError err) {{ Console.printLine('list') }}
      try {{ Directory.delete('{path}') }} catch (FileNotFoundError err) {{ Console.printLine('directory delete') }}
      Console.printLine(File.exists('{path}'))
    }} }}"#), "read\ncleanup\nwrite\nappend\ndelete\ninfo\nlist\ndirectory delete\nfalse\n");
}

#[test]
fn runtime_file_handles_distinguish_empty_content_closed_handles_and_binary_data() {
    let path = std::env::temp_dir().join(format!("ube-file-handles-{}", std::process::id()));
    let path = path.to_string_lossy();
    runs(&format!(r#"class App {{ start() {{
      File.write('{path}', '')
      Console.printLine(File.read('{path}'))
      File.write('{path}', 'hello')
      final file = File.open('{path}')
      Console.printLine(file.read())
      file.close()
      try {{ Console.printLine(file.read()) }} catch (Error err) {{ Console.printLine('closed') }}
      File.delete('{path}')
    }} }}"#), "\nhello\nclosed\n");
    let binary = std::env::temp_dir().join(format!("ube-binary-{}", std::process::id()));
    fs::write(&binary, b"hello\0world").unwrap();
    runs(&format!(r#"class App {{ start() {{
      try {{ Console.printLine(File.read('{}')) }} catch (Error err) {{ Console.printLine('binary rejected') }}
    }} }}"#, binary.display()), "binary rejected\n");
    fs::remove_file(binary).unwrap();
}

#[test]
fn runtime_server_startup_failures_are_catchable() {
    let reserved = std::net::TcpListener::bind(("127.0.0.1", 0)).ok();
    let port = reserved.as_ref().map(|listener| listener.local_addr().unwrap().port()).unwrap_or(8080);
    runs(&format!(r#"class App {{ start() {{
      final app = Server()
      try {{ app.start(-1) Console.printLine('wrong') }} catch (Error err) {{ Console.printLine('invalid port') }}
      try {{ app.start({port}) Console.printLine('wrong') }} catch (Error err) {{ Console.printLine('bind failed') }}
      finally {{ Console.printLine('cleanup') }}
      Console.printLine('recovered')
    }} }}"#), "invalid port\nbind failed\ncleanup\nrecovered\n");
}

#[test]
fn unhandled_runtime_io_errors_exit_unsuccessfully() {
    let output = native(r#"class App { start() { File.read('/ube-nonexistent-directory/file') Console.printLine('wrong') } }"#);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("FileNotFoundError"));
}

#[test]
fn native_list_indexing_compiles_with_and_without_handlers() {
    runs(r#"class App {
      int first() { final values = [42] return values[0] }
      start() {
        Console.printLine(this.first())
        final values = [10, 20]
        try { Console.printLine(values[0]) Console.printLine(values[1]) }
        catch (Error err) { Console.printLine('unexpected error') }
        finally { Console.printLine('cleanup') }
      }
    }"#, "42\n10\n20\ncleanup\n");
}

#[test]
fn native_list_bounds_errors_are_caught_and_run_cleanup() {
    runs(r#"class App { start() {
      final values = [42]
      try { Console.printLine(values[-1]) Console.printLine('unreachable') }
      catch (Error err) { Console.printLine('negative index') }
      finally { Console.printLine('negative cleanup') }
      try { Console.printLine(values[1]) Console.printLine('unreachable') }
      catch (Error err) { Console.printLine('past end') }
      finally { Console.printLine('past end cleanup') }
      Console.printLine(values[0])
    } }"#, "negative index\nnegative cleanup\npast end\npast end cleanup\n42\n");
}

#[test]
fn native_bare_return_needs_no_semicolon_and_runs_cleanup() {
    runs(r#"class App {
      stop() {
        try {
          Console.printLine('before')
          return
          Console.printLine('unreachable')
        } finally { Console.printLine('cleanup') }
      }
      int value() { return (
        42
      ) }
      start() { this.stop() Console.printLine(this.value()) }
    }"#, "before\ncleanup\n42\n");
}

#[test]
fn backing_fields_are_private_to_the_declaring_class() {
    for statement in ["Console.printLine(user._age)", "user._age = 2", "Console.printLine(user._name)", "user._name = 'B'"] {
        rejects(&format!("class User(string name, var int age) class App {{ start() {{ final user = User('A', 1) {statement} }} }}"), "is private to class 'User'");
    }
}

#[test]
fn public_fields_use_getter_and_setter_calls() {
    runs(r#"class User(string name, var int age) {
      birthday() { this._age = this._age + 1 }
      int read() { return this._age }
    }
    class App { start() {
      final user = User('A', 1)
      final shared = user
      Console.printLine(user.name())
      Console.printLine(user.age())
      user.age(2)
      user.birthday()
      Console.printLine(user.read())
      Console.printLine(shared.age())
    } }"#, "A\n1\n3\n3\n");
}

#[test]
fn public_field_access_and_setter_arguments_are_checked() {
    for (statement, expected) in [
        ("Console.printLine(user.age)", "must be read using 'age()'"),
        ("user.age = 2", "must be written using 'age(value)'"),
        ("user.age('bad')", "setter 'age' expects"),
        ("user.age(1, 2)", "requires exactly one argument"),
        ("user.name('B')", "cannot set immutable field 'name'"),
        ("User.age(2)", "requires an instance of 'User'"),
        ("User.age()", "requires an instance of 'User'"),
    ] {
        rejects(&format!("class User(string name, var int age) class App {{ start() {{ final user = User('A', 1) {statement} }} }}"), expected);
    }
}

#[test]
fn native_declarations_zero_initialize_without_constructor_defaults() {
    runs(r#"class Defaults {
      int load() { Console.printLine('default evaluated') return 99 }
    }
    class Record(int count = Defaults().load(), string label = 'configured')
    class App { start() {
      Record zero
      Console.printLine(zero.count())
      Console.printLine(zero.label())
      final explicit = Record()
      Console.printLine(explicit.count())
      Console.printLine(explicit.label())
      Record? absent
      Console.printLine(absent == null)
    } }"#, "0\n\ndefault evaluated\n99\nconfigured\ntrue\n");
}

#[test]
fn native_zero_initialization_recurses_without_sharing_or_defaults() {
    runs(r#"class Point(var int x = 42)
    class Pair(Point left, Point right)
    class Node(int value = 99, Node? next = null)
    class App { start() {
      Pair pair
      pair.left().x(7)
      Console.printLine(pair.left().x())
      Console.printLine(pair.right().x())
      Pair other
      Console.printLine(other.left().x())
      Node node
      Console.printLine(node.value())
      Console.printLine(node.next() == null)
    } }"#, "7\n0\n0\n0\ntrue\n");
}

#[test]
fn native_primitive_declarations_keep_zero_values() {
    runs(r#"class Settings(int count = 7, long id = 9L, decimal ratio = 1.5,
      bool enabled = true, string text = 'configured')
    class App { start() {
      int count
      long id
      decimal ratio
      bool enabled
      string text
      Console.printLine(count) Console.printLine(id) Console.printLine(ratio) Console.printLine(enabled) Console.printLine(text)
      Settings settings
      Console.printLine(settings.count()) Console.printLine(settings.id()) Console.printLine(settings.ratio())
      Console.printLine(settings.enabled()) Console.printLine(settings.text())
    } }"#, "0\n0\n0\nfalse\n\n0\n0\n0\nfalse\n\n");
}

#[test]
fn unsafe_zero_initialization_requires_nullable_links_or_explicit_initializers() {
    rejects("class Node(Node next) class App { start() { Node node } }", "recursive non-nullable class");
    rejects("class A(B b) class B(A a) class App { start() { A value } }", "recursive non-nullable class");
    rejects("class App { start() { Server app } }", "requires an explicit initializer");
    rejects("class App { start() { Channel<int> values } }", "requires an explicit initializer");
    rejects("trait Named { string name() } class App { start() { Named value } }", "requires an explicit initializer");
}

#[test]
fn native_startup_uses_the_same_zero_values_as_local_declarations() {
    runs(r#"class Defaults {
      int load() { Console.printLine('unexpected default evaluation') return 123 }
    }
    class Child(int count = 77)
    class App(int count = Defaults().load(), long support = 44L,
      string caption = 'configured', Child child = Child(), Child? absent = Child()) {
      start() {
        Console.printLine(this.count())
        Console.printLine(this.support())
        Console.printLine(this.caption())
        Console.printLine(this.child().count())
        Console.printLine(this.absent() == null)
        App other
        Console.printLine(other.child().count())
        final configured = Child()
        Console.printLine(configured.count())
      }
    }"#, "0\n0\n\n0\ntrue\n0\n77\n");
}

#[test]
fn unsafe_startup_layouts_are_rejected_before_codegen() {
    rejects("class App(App next) { start() {} }", "recursive non-nullable class");
    rejects("class App(Server app = Server()) { start() {} }", "requires an explicit initializer");
}

#[test]
fn removed_boolean_operators_are_rejected_in_expressions() {
    for operator in ["nand", "nor", "xor", "xnor"] {
        let source = format!("class App {{ start() {{ Console.printLine(true {operator} false) }} }}");
        let mut parser = Parser::new(Lexer::new(&source));
        parser.parse_program();
        assert!(!parser.diagnostics.is_empty(), "Accepted removed operator {operator}");
    }
}

#[test]
fn native_boolean_logic_keeps_not_and_or_precedence() {
    runs(r#"class App { start() {
      Console.printLine(not true) Console.printLine(not false)
      Console.printLine(true and true) Console.printLine(true and false)
      Console.printLine(false or true) Console.printLine(false or false)
      Console.printLine(true or false and false)
      Console.printLine((true or false) and false)
      Console.printLine(not (true and false))
      Console.printLine(not (true or false))
      final nand = 1
      final nor = 2
      final xor = 3
      final xnor = 4
      Console.printLine(nand + nor + xor + xnor)
    } }"#, "false\ntrue\ntrue\nfalse\ntrue\nfalse\ntrue\nfalse\ntrue\nfalse\n10\n");
}

#[test]
fn native_backtick_text_blocks_preserve_escaped_json_and_literal_markers() {
    runs(r#"class Consumer { string echo(string value) { return value } }
    class App { start() {
      Console.printLine('A')
      Console.printLine('one line')
      Console.printLine(`{"name":"Ube","value":"\${notDeclared}","line":"\\n"}`)
      Console.printLine(`first
  second`)
      Console.printLine(`trailing\\`)
      Console.printLine('after')
      final consumer = Consumer()
      Console.printLine(consumer.echo(`{"message":"hello","active":true}`))
    } }"#, "A\none line\n{\"name\":\"Ube\",\"value\":\"${notDeclared}\",\"line\":\"\\n\"}\nfirst\n  second\ntrailing\\\nafter\n{\"message\":\"hello\",\"active\":true}\n");
}

#[test]
fn double_quotes_are_reserved_and_single_quotes_cannot_span_lines() {
    rejects("class App { start() { Console.printLine(\"reserved\") } }", "Double quotes are reserved for JSON");
    rejects("class App { start() { Console.printLine('first\nsecond') } }", "Unescaped line break");
    rejects("class App { start() { Console.printLine(`unfinished) } }", "Unterminated backtick text block");
}

#[test]
fn nullable_scalars_survive_tasks_channels_and_captures() {
    runs(r#"class App {
      int? score(int? value) { return value }
      start() {
        final ch = Channel<int?>(2)
        int? missing = Async.start(() -> this.score(null)).await()
        int? zero = Async.start(() -> this.score(0)).await()
        Console.printLine(missing == null)
        Console.printLine(zero == null)
        Console.printLine(zero ?? 99)
        ch.send(missing).await()
        ch.send(zero).await()
        int? first = ch.receive().await()
        int? second = ch.receive().await()
        Console.printLine(first == null)
        Console.printLine(second ?? 99)
        final task = Async.start(() -> (missing))
        int? captured = task.await()
        Console.printLine(captured == null)
      }
    }"#, "true\nfalse\n0\ntrue\n0\ntrue\n");
}

#[test]
fn nullable_false_and_decimal_zero_are_present() {
    runs(r#"class App {
      bool? truth(bool? value) { return value }
      decimal? amount(decimal? value) { return value }
      start() {
        final ch = Channel<bool?>(2)
        ch.send(null).await()
        ch.send(Async.start(() -> this.truth(false)).await()).await()
        bool? missing = ch.receive().await()
        bool? present = ch.receive().await()
        Console.printLine(missing == null)
        Console.printLine(present == null)
        Console.printLine(present ?? true)
        final amounts = Channel<decimal?>(2)
        amounts.send(Async.start(() -> this.amount(null)).await()).await()
        amounts.send(Async.start(() -> this.amount(0.0)).await()).await()
        decimal? a = amounts.receive().await()
        decimal? b = amounts.receive().await()
        Console.printLine(a == null)
        Console.printLine(b == null)
        Console.printLine(b ?? 99.0)
      }
    }"#, "true\nfalse\nfalse\ntrue\nfalse\n0\n");
}

#[test]
fn nullable_fields_calls_and_presence_conditions_work_natively() {
    runs(r#"class User(var int? age)
      class App {
        int? echo(int? value) { return value }
        long? wide(long? value) { return value }
        start() {
          final user = User(null)
          Console.printLine(user.age())
          user.age(0)
          Console.printLine(user.age())
          Console.printLine(null == user.age())
          int? empty
          if (empty) { Console.printLine('wrong') } else { Console.printLine('empty') }
          int? zero = this.echo(0)
          if (zero) { Console.printLine('present') }
          Console.printLine(this.echo(null))
          final wide = Channel<long?>(2)
          wide.send(Async.start(() -> this.wide(null)).await()).await()
          wide.send(Async.start(() -> this.wide(9223372036854775807L)).await()).await()
          long? a = wide.receive().await()
          long? b = wide.receive().await()
          Console.printLine(a)
          Console.printLine(b)
          final bytes = Channel<byte?>(2)
          byte value
          bytes.send(null).await()
          bytes.send(value).await()
          byte? c = bytes.receive().await()
          byte? d = bytes.receive().await()
          Console.printLine(c)
          Console.printLine(d)
        }
      }"#, "null\n0\nfalse\nempty\npresent\nnull\nnull\n9223372036854775807\nnull\n0\n");
}

#[test]
fn nullable_payloads_do_not_hide_closure_failure_or_repeat_awaits() {
    runs(r#"class App {
      int? absent() { return null }
      int? fail() { throw 'failed' }
      start() {
        final ch = Channel<int?>(1)
        ch.send(null).await()
        ch.close()
        int? value = ch.receive().await()
        Console.printLine(value == null)
        try { int? closed = ch.receive().await() Console.printLine('wrong') }
        catch (Error err) { Console.printLine('closed') }
        try { int? failed = Async.start(() -> this.fail()).await() Console.printLine('wrong') }
        catch (Error err) { Console.printLine(err.message()) }
        final task = Async.start(() -> this.absent()).timeout(1000)
        int? first = task.await()
        int? second = task.await()
        Console.printLine(first == null)
        Console.printLine(second == null)
      }
    }"#, "true\nclosed\nfailed\ntrue\ntrue\n");
}

#[test]
fn untyped_collections_diagnose_nullable_scalar_storage() {
    rejects("class App { start() { int? value = null final values = [value] } }", "untyped List storage");
    rejects("class App { int? absent() { return null } start() { final tasks = [Async.start(() -> this.absent())] } }", "untyped List storage");
}

#[test]
fn sqlite_native_queries_and_errors() {
    runs(r#"class App { start() {
        final db = SQLite.open(':memory:')
        db.execute('CREATE TABLE items (id INTEGER, name TEXT, price REAL, extra TEXT)')
        Console.printLine(db.execute('INSERT INTO items VALUES (7, \'ube\', 2.5, NULL)'))
        final rows = db.query('SELECT * FROM items')
        Console.printLine(rows.next()) Console.printLine(rows.getLong(0)) Console.printLine(rows.getString(1))
        Console.printLine(rows.getDecimal(2)) Console.printLine(rows.isNull(3)) Console.printLine(rows.next())
        rows.close()
        try { db.execute('invalid SQL') } catch (Error err) { Console.printLine('invalid SQL caught') }
        db.close() db.close()
        try { db.query('SELECT 1') } catch (Error err) { Console.printLine('closed caught') }
    } }"#, "1\ntrue\n7\nube\n2.5\ntrue\nfalse\ninvalid SQL caught\nclosed caught\n");
}

#[test]
fn sqlite_example_and_handle_creation_rules() {
    let output = native(include_str!("../../examples/sqlite/App.ube"));
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(String::from_utf8_lossy(&output.stderr), "[INFO] 1\n[INFO] Ube\n");
    rejects("class App { start() { SQLite.execute('SELECT 1') } }", "require an instance");
    rejects("class App { start() { final db = SQLite() } }", "Use SQLite.open");
    rejects("class App { start() { final rows = SQLiteResult() } }", "Use SQLite.open");
}

#[test]
fn sqlite_prepared_bindings_transactions_and_metadata() {
    runs(r#"class App { start() {
        final db = SQLite.open(':memory:')
        db.busyTimeout(50)
        db.executeBatch('PRAGMA foreign_keys = ON; CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT UNIQUE, price REAL, active INTEGER, payload BLOB, extra TEXT);')
        final insert = db.prepare('INSERT INTO items VALUES (?1, :name, ?3, ?4, ?5, ?6)')
        Console.printLine(insert.parameterCount()) Console.printLine(insert.parameterName(2)) Console.printLine(insert.parameterIndex(':name'))
        insert.bindLong(1, 4294967296L)
        insert.bindString(insert.parameterIndex(':name'), 'hello\' ; DROP TABLE items; --')
        insert.bindDecimal(3, 2.5) insert.bindBool(4, true) insert.bindBlobHex(5, '00ff80') insert.bindNull(6)
        db.begin() Console.printLine(db.inTransaction()) Console.printLine(insert.execute()) Console.printLine(db.changes()) Console.printLine(db.lastInsertRowId())
        db.commit() Console.printLine(db.inTransaction())
        final select = db.prepare('SELECT * FROM items WHERE id = ?')
        select.bindLong(1, 4294967296L)
        final rows = select.query()
        Console.printLine(rows.rowCount()) Console.printLine(rows.columnCount()) Console.printLine(rows.columnName(1)) Console.printLine(rows.columnIndex('NAME'))
        Console.printLine(rows.next()) Console.printLine(rows.getLong(0)) Console.printLine(rows.getString(rows.columnIndex('name')))
        Console.printLine(rows.getDecimal(2)) Console.printLine(rows.getBool(3)) Console.printLine(rows.getBlobHex(4)) Console.printLine(rows.isNull(5)) Console.printLine(rows.columnType(4))
        try { Console.printLine(rows.getInt(0)) } catch (SQLiteError err) { Console.printLine('overflow') }
        rows.reset() Console.printLine(rows.next())
        db.begin() db.execute('DELETE FROM items') db.rollback()
        select.clearBindings() final empty = select.query() Console.printLine(empty.rowCount()) empty.close()
        insert.clearBindings() insert.bindInt(1, 2) insert.bindString(2, 'second') Console.printLine(insert.execute())
        db.begin() db.execute('DELETE FROM items') db.close()
        Console.printLine(rows.getLong(0))
        try { select.execute() } catch (SQLiteError err) { Console.printLine('closed statement') }
        select.close() insert.close() rows.close()
    } }"#, "6\n:name\n2\ntrue\n1\n1\n4294967296\nfalse\n1\n6\nname\n1\ntrue\n4294967296\nhello' ; DROP TABLE items; --\n2.5\ntrue\n00ff80\ntrue\nblob\noverflow\ntrue\n0\n1\n4294967296\nclosed statement\n");
}

#[test]
fn sqlite_returning_nulls_errors_and_parameter_validation() {
    runs(r#"class App { start() {
        final db = SQLite.open(':memory:')
        db.execute('CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT NOT NULL UNIQUE)')
        final returning = db.prepare('INSERT INTO items(value) VALUES (?) RETURNING id, value')
        returning.bindString(1, 'first') final rows = returning.query()
        Console.printLine(rows.next()) Console.printLine(rows.getInt(0)) Console.printLine(rows.getString(1)) rows.close()
        try { returning.query() } catch (SQLiteError err) { Console.printLine('unique') }
        try { returning.bindInt(0, 1) } catch (SQLiteError err) { Console.printLine('index') }
        try { returning.bindBlobHex(1, '0x') } catch (SQLiteError err) { Console.printLine('hex') }
        try { db.execute('DELETE FROM items; DROP TABLE items') } catch (SQLiteError err) { Console.printLine('multiple') }
        try { db.query('SELECT 1; SELECT 2') } catch (SQLiteError err) { Console.printLine('multiple query') }
        final count = db.query('SELECT count(*) FROM items') Console.printLine(count.next()) Console.printLine(count.getInt(0)) count.close()
        try { db.execute('SELECT 1') } catch (SQLiteError err) { Console.printLine('use query') }
        try { db.query('DELETE FROM items') } catch (SQLiteError err) { Console.printLine('use execute') }
        try { db.busyTimeout(-1) } catch (SQLiteError err) { Console.printLine('timeout') }
        final empty = db.query('SELECT id AS number FROM items WHERE 0') Console.printLine(empty.columnName(0)) Console.printLine(empty.next()) empty.close()
        returning.close() db.close()
    } }"#, "true\n1\nfirst\nunique\nindex\nhex\nmultiple\nmultiple query\ntrue\n1\nuse query\nuse execute\ntimeout\nnumber\nfalse\n");
    rejects("class App { start() { final db = SQLite.open() } }", "expects 1 arguments");
    rejects("class App { start() { final db = SQLite.open(':memory:') db.close(1) } }", "expects 0 arguments");
    rejects("class App { start() { final db = SQLite.open(':memory:') final s = db.prepare('SELECT ?') s.bindLong(1, 'bad') } }", "Expected");
    rejects("class App { start() { final s = SQLiteStatement() } }", "Use SQLite.open");
}

#[test]
fn sqlite_native_file_backup_restore_and_read_only() {
    let dir = Scratch(std::env::temp_dir().join(format!("ube-sqlite-native-{}", std::process::id())));
    fs::create_dir_all(&dir.0).unwrap();
    let source = r#"class App { start() {
        Console.printLine(SQLite.version() != '')
        final db = SQLite.open('$ROOT/source.db')
        Console.printLine(db.readOnly())
        db.executeBatch('CREATE TABLE t(id INTEGER); INSERT INTO t VALUES (7);')
        db.backup('$ROOT/backup.db')
        db.execute('UPDATE t SET id = 9')
        db.restore('$ROOT/backup.db')
        db.close()
        final ro = SQLite.openReadOnly('$ROOT/source.db')
        Console.printLine(ro.readOnly())
        final rows = ro.query('SELECT id FROM t') Console.printLine(rows.next()) Console.printLine(rows.getInt(0)) rows.close()
        try { ro.execute('DELETE FROM t') } catch (SQLiteError err) { Console.printLine('read only') }
        ro.close()
    } }"#.replace("$ROOT", dir.0.to_str().unwrap());
    runs(&source, "true\nfalse\ntrue\ntrue\n7\nread only\n");
}


#[test]
fn server_builtin_uses_capitalized_name() {
    runs("class App { start() { final server = Server() Console.printLine(server.port()) server.stop() } }", "8080\n");
    rejects("class App { start() { final app = server() } }", "use Server()");
}

#[test]
fn lowercase_variables_and_console_output_remain_valid() {
    runs(r#"class App { start() {
        final math = 4
        final json = 'data'
        final response = 'ok'
        if (math < 5) { Console.printLine(math) }
        Console.printLine(json) Console.printLine(response)
        final sqlite = SQLite.open(':memory:') sqlite.close()
    } }"#, "4\ndata\nok\n");
}

#[test]
fn log_shorthand_levels_filtering_and_console_streams() {
    let output = native(r#"class App { start() {
        log('hello') Log.info('hello')
        Log.debug('hidden') Log.warn('warning') Log.error('failure')
        Log.setLevel('debug') Log.debug(42) Log.info(true)
        Log.setLevel('warn') log('hidden') Log.warn(2.5) Log.error(false)
        Log.setLevel('off') Log.error('hidden')
        Console.print('plain ') Console.printLine(7)
        Log.setLevel('INFO') log('back')
        try { Log.setLevel('invalid') } catch (Error err) { Console.printLine('invalid level') }
        log('still info')
    } }"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "plain 7\ninvalid level\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "[INFO] hello\n[INFO] hello\n[WARN] warning\n[ERROR] failure\n[DEBUG] 42\n[INFO] true\n[WARN] 2.5\n[ERROR] false\n[INFO] back\n[INFO] still info\n");
}

#[test]
fn log_nullable_values_interpolation_and_error_messages() {
    let output = native(r#"class Failure(string message)
    class App(var int calls = 0) {
        int value() { this._calls = this._calls + 1 return this._calls }
        start() {
            int? number = 0
            bool? flag = false
            decimal? real = 0.0
            string? text = null
            log(number) Log.warn(flag) Log.error(real) Log.info(text) log(null)
            int? missing = null log(missing)
            log('scalars ${4294967296L} ${true} ${2.5} ${missing}')
            log('value ${this.value()}') Console.printLine(this._calls)
            try { throw Failure('boom') } catch (Error err) { Log.error(err) }
            Console.printLine('recovered')
        }
    }"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\nrecovered\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "[INFO] 0\n[WARN] false\n[ERROR] 0\n[INFO] null\n[INFO] null\n[INFO] null\n[INFO] scalars 4294967296 true 2.5 null\n[INFO] value 1\n[ERROR] boom\n");
}

#[test]
fn log_checks_arity_types_and_capitalization() {
    rejects("class App { start() { log() } }", "exactly one value");
    rejects("class App { start() { Log.info('a', 'b') } }", "exactly one value");
    rejects("class App { start() { Log.warn() } }", "exactly one value");
    rejects("class App { start() { Log.setLevel(1) } }", "setLevel requires a string");
    rejects("class App { start() { Log('wrong case') } }", "Use log(value)");
    rejects("class App { start() { log.info('wrong case') } }", "use Log");
    rejects("class Item class App { start() { log(Item()) } }", "Output requires");
    rejects("class App { start() { Console.printLine() } }", "exactly one value");
    rejects("class App { start() { Console() } }", "do not create instances");
}

#[test]
fn log_threshold_is_shared_with_tasks_and_filtering_keeps_argument_evaluation() {
    let output = native(r#"class App(var int calls = 0) {
        int next() { this._calls = this._calls + 1 return this._calls }
        worker() { Log.debug('from worker') }
        start() {
            Log.setLevel('off') log(this.next())
            Console.printLine(this.calls())
            Log.setLevel('debug') Async.start(() -> this.worker()).await()
        }
    }"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "[DEBUG] from worker\n");
}

#[test]
fn log_context_values_records_and_cleanup() {
    let output = native(r#"class App { start() {
        Log.clear()
        Console.printLine(Log.get('absent') == null)
        Log.put('user', 'alice') Log.put('requestId', 'req-42')
        string? original = Log.get('user')
        log('processing')
        Log.put('user', 'bob') Log.warn('retry')
        Console.printLine(Log.get('user'))
        Log.remove('requestId') Log.remove('absent')
        Log.error('failed')
        try { throw 'oops' } catch (Error err) { log(err) } finally { Log.clear() }
        Console.printLine(original)
        Console.printLine(Log.get('user') == null)
        Log.info('clean')
        Log.put('empty', '') Console.printLine(Log.get('empty') == '') Log.clear()
    } }"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "true\nbob\nalice\ntrue\ntrue\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "[INFO] {\"requestId\":\"req-42\",\"user\":\"alice\"} processing\n[WARN] {\"requestId\":\"req-42\",\"user\":\"bob\"} retry\n[ERROR] {\"user\":\"bob\"} failed\n[INFO] {\"user\":\"bob\"} oops\n[INFO] clean\n");
}

#[test]
fn log_context_task_snapshots_and_sibling_isolation() {
    let output = native(r#"class App {
        string child(string name, Channel<int> ready, Channel<int> release) {
            if (Log.get('request') != 'parent') { throw 'missing inherited context' }
            Log.put('request', name)
            ready.send(1).await()
            release.receive().await()
            log('child')
            return Log.get('request') ?? 'missing'
        }
       fail() { Log.put('request', 'failed task') throw 'failure' }
        start() {
            Log.put('request', 'parent')
            final ready = Channel<int>(2) final release = Channel<int>(2)
            final first = Async.start(() -> this.child('A', ready, release))
            final second = Async.start(() -> this.child('B', ready, release))
            ready.receive().await() ready.receive().await()
            Log.put('request', 'changed parent')
            release.send(1).await() release.send(1).await()
            Console.printLine(first.await()) Console.printLine(second.await())
            Console.printLine(Log.get('request'))
            try { Async.start(() -> this.fail()).await() } catch (Error err) { Console.printLine(Log.get('request')) }
            Log.clear()
            final inherited = Async.start(() -> Log.get('request'))
            Console.printLine(inherited.await() == null)
        }
    }"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "A\nB\nchanged parent\nchanged parent\ntrue\n");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut lines = stderr.lines().collect::<Vec<_>>(); lines.sort();
    assert_eq!(lines, vec!["[INFO] {\"request\":\"A\"} child", "[INFO] {\"request\":\"B\"} child"]);
}

#[test]
fn log_context_argument_validation() {
    rejects("class App { start() { Log.put('key') } }", "2 arguments");
    rejects("class App { start() { Log.put('key', 42) } }", "must be non-null strings");
    rejects("class App { start() { Log.put('key', null) } }", "must be non-null strings");
    rejects("class App { start() { Log.get(null) } }", "must be non-null strings");
    rejects("class App { start() { Log.get('key', 'extra') } }", "1 arguments");
    rejects("class App { start() { Log.clear('key') } }", "0 arguments");
    rejects("class App { start() { Log.put('key', log('message')) } }", "must be non-null strings");
    rejects("class App { start() { string value = Log.get('key') } }", "Mismatched initializer type");
}

#[test]
fn log_context_example_runs() {
    let output = native(include_str!("../../examples/log_context/App.ube"));
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "req-42\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "[INFO] {\"requestId\":\"req-42\"} Processing request\n[WARN] {\"requestId\":\"req-42\",\"user\":\"alice\"} Retrying operation\n[INFO] Context cleared\n");
}

#[test]
fn console_reads_unicode_lines_and_eof() {
    let source = r#"class App { start() {
        string? character = Console.read()
        string? line = Console.readLine()
        Console.printLine(character ?? 'EOF')
        Console.printLine(line ?? 'EOF')
        Console.printLine(Console.readLine() ?? 'EOF')
        Console.printLine(Console.readLine() ?? 'EOF')
        Console.printLine(Console.read() ?? 'EOF')
        Console.printLine(Console.readLine() ?? 'EOF')
    } }"#;
    let output = native_with_input(source, "🍠hello\r\n\nlast".as_bytes());
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "🍠\nhello\n\nlast\nEOF\nEOF\n");
    rejects("class App { start() { Console.read(1) } }", "no arguments");
    rejects("class App { start() { Console.readLine('x') } }", "no arguments");
}
#[test]
fn console_input_failures_can_be_caught() {
    let output = native_with_input(r#"class App { start() {
        try { Console.read() } catch (GenericError err) { Console.printLine('caught') }
    } }"#, &[0xff]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "caught\n");
}

#[test]
fn console_print_methods_and_shorthand() {
    let output = native(r#"class App {
        int next() { Console.print('evaluated:') return 42 }
        start() {
            Log.setLevel('off')
            Console.print('hello ')
            Console.printLine('world')
            print(this.next())
            print(true)
            int? missing = null
            print(missing)
            print(GenericError('oops'))
            print('')
        }
    }"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello world\nevaluated:42\ntrue\nnull\noops\n\n");
    assert!(output.stderr.is_empty());
    for call in ["print()", "print(1, 2)", "Console.print()", "Console.printLine(1, 2)"] {
        rejects(&format!("class App {{ start() {{ {call} }} }}"), "exactly one value");
    }
    for call in ["Console.write('old')", "Console.writeLine('old')"] {
        rejects(&format!("class App {{ start() {{ {call} }} }}"), "requires a class call");
    }
    rejects("class Item class App { start() { print(Item()) } }", "Output requires");
}

#[test]
fn invalid_server_tls_configuration_is_catchable() {
    runs(r#"class App { start() {
        final server = Server()
        try { server.tls('/ube-missing-cert.pem', '/ube-missing-key.pem') }
        catch (GenericError err) { print('invalid TLS configuration') }
    } }"#, "invalid TLS configuration\n");
}

#[test]
fn session_api_validates_instances_types_arity_and_construction() {
    rejects(
        "class App { start() { final session = Session() } }",
        "request.session()",
    );
    rejects(
        "class App { start() { Session.get('key') } }",
        "requires an instance",
    );
    rejects(
        "class App { start() { Request.session() } }",
        "requires an instance",
    );
    for call in [
        "get()",
        "get('a', 'b')",
        "set('a')",
        "remove()",
        "regenerate(1)",
        "destroy(1)",
    ] {
        rejects(&format!("class App {{ start() {{ final server = Server() server.get('/', (Request request) -> {{ request.session().{call} return Response.ok('ok') }}) }} }}"), "expects");
    }
    for call in [
        "get(1)",
        "set('key', null)",
        "set('key', 42)",
        "remove(true)",
    ] {
        rejects(&format!("class App {{ start() {{ final server = Server() server.get('/', (Request request) -> {{ request.session().{call} return Response.ok('ok') }}) }} }}"), "non-null strings");
    }
    rejects("class App { start() { final server = Server() server.get('/', (Request request) -> { request.session(1) return Response.ok('ok') }) } }", "expects 0 arguments");
}

#[test]
fn jwt_sign_verify_claims_and_failure_paths_run_natively() {
    runs(
        r#"class App { start() {
        final secret = 'test-only-32-byte-secret-0123456789'
        final token = Jwt.sign('{"sub":"user-123","iss":"ube-test","aud":"ube-api","exp":4102444800}', secret, 'HS256')
        final claims = Jwt.verify(token, secret, 'HS256', 'ube-test', 'ube-api')
        print(claims.get('sub') ?? 'missing')
        print(claims.get('missing'))
        claims.close()
        try { claims.get('sub') } catch (GenericError err) { print('closed') }
        try { Jwt.verify(token, secret, 'HS256', 'wrong', 'ube-api') }
        catch (GenericError err) { print('wrong issuer') }
        try { Jwt.verify(token, secret, 'HS256', 'ube-test', 'wrong') }
        catch (GenericError err) { print('wrong audience') }
        try { Jwt.sign('{"exp":1,"iss":"i","aud":"a"}', secret, 'HS256') }
        catch (GenericError err) { print('expired') }
        try { Jwt.sign('{"exp":4102444800,"iss":"i","aud":"a"}', 'weak', 'HS256') }
        catch (GenericError err) { print('weak secret') }
    } }"#,
        "user-123\nnull\nclosed\nwrong issuer\nwrong audience\nexpired\nweak secret\n",
    );
}
#[test]
fn auth_builtins_validate_methods_types_and_factories() {
    for class in ["Jwt", "JwtClaims", "OAuth2", "OAuth2Token"] {
        rejects(
            &format!("class App {{ start() {{ {class}() }} }}"),
            "cannot be constructed",
        );
    }
    rejects(
        "class App { start() { Jwt.sign('a', 'b') } }",
        "expects 3 arguments",
    );
    rejects(
        "class App { start() { Jwt.verify('a', 'b', 'HS256', 'issuer', null) } }",
        "non-null",
    );
    rejects(
        "class App { start() { OAuth2.create('a','b','c','d',42) } }",
        "argument types",
    );
    rejects(
        "class App { start() { JwtClaims.get('sub') } }",
        "require an instance",
    );
    rejects(
        "class App { start() { OAuth2.authorize('a', 'b') } }",
        "require an instance",
    );
    runs(
        r#"class App { start() {
        try { OAuth2.create('http://provider.example/auth', 'https://provider.example/token', 'id', '', 'https://client.example/callback') }
        catch (GenericError err) { print('requires HTTPS') }
    } }"#,
        "requires HTTPS\n",
    );
}

#[test]
fn http3_configuration_and_startup_errors_are_catchable() {
    runs(
        r#"class App { start() {
        final server = Server()
        print(server.http3Port())
        try { server.enableHttp3(-1) } catch (GenericError err) { print('invalid port') }
        try { server.enableHttp3(65536) } catch (GenericError err) { print('invalid port') }
        server.enableHttp3(0)
        print(server.http3Port())
        try { server.start(0) } catch (GenericError err) { print('TLS required') }
    } }"#,
        "-1\ninvalid port\ninvalid port\n0\nTLS required\n",
    );
    rejects(
        "class App { start() { Server.http3Port() } }",
        "requires an instance",
    );
    rejects(
        "class App { start() { final server = Server() server.enableHttp3() } }",
        "1 arguments",
    );
    rejects(
        "class App { start() { final server = Server() server.http3Port(1) } }",
        "0 arguments",
    );
    rejects(
        "class App { start() { final server = Server() server.enableHttp3('port') } }",
        "port must be an int",
    );
}

#[test]
fn unified_sql_bindings_transactions_and_typed_results_native() {
    runs(r#"
class App {
    insert(SQL db, SQLParams params) { db.execute('INSERT INTO users VALUES (?, ?)', params) }
    start() {
        final db = SQL.open('sqlite::memory:')
        final params = SQLParams.create()
        try {
            print(db.dialect())
            print(db.placeholder(1))
            print(db.quoteIdentifier('user names'))
            db.execute('CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT)', params)
            params.addInt(7)
            params.addString('O\'Brien 🍠')
            this.insert(db, params)
            params.clear()
            final tx = db.begin()
            try {
                tx.execute(`INSERT INTO users VALUES (8, 'rolled back')`, params)
                tx.savepoint('nested')
                tx.execute(`INSERT INTO users VALUES (9, 'nested row')`, params)
                tx.rollbackTo('nested')
                tx.release('nested')
                tx.rollback()
            } finally { tx.close() }
            final rows = db.query('SELECT id, name, NULL AS optional FROM users', params)
            try {
                print(rows.rowCount())
                print(rows.columnName(1))
                for (rows.next()) {
                    print(rows.getInt(0))
                    print(rows.getString(rows.columnIndex('name')))
                    print(rows.isNull(2))
                }
            } finally { rows.close() }
            try { db.execute('INSERT INTO users VALUES (?, ?)', params) }
            catch (SQLError err) { print('binding error caught') }
            final rows2 = db.query('SELECT COUNT(*) FROM users', params)
            rows2.next()
            print(rows2.getLong(0))
            rows2.close()
            try { db.execute('SELECT 1; DROP TABLE users', params) }
            catch (SQLError err) { print('multiple statements rejected') }
        } finally { params.close() db.close() }
    }
}
"#, "sqlite\n?\n\"user names\"\n1\nname\n7\nO'Brien 🍠\ntrue\nbinding error caught\n1\nmultiple statements rejected\n");
}
#[test]
fn unified_sql_enforces_factory_arity_receiver_and_types() {
    for source in [
        "class App { start() { final db = SQL() } }",
        "class App { start() { final p = SQLParams() } }",
        "class App { start() { final r = SQLResult() } }",
        "class App { start() { final t = SQLTransaction() } }",
    ] { rejects(source, "SQL handles cannot be constructed"); }
    rejects("class App { start() { SQL.query('SELECT 1', SQLParams.create()) } }", "require an instance");
    rejects("class App { start() { final db = SQL.open('sqlite::memory:') db.open('sqlite::memory:') } }", "require the class");
    rejects("class App { start() { SQL.open(42) } }", "Invalid SQL method argument types");
    rejects("class App { start() { final p = SQLParams.create() p.addInt('42') } }", "Invalid SQL method argument types");
    rejects("class App { start() { final p = SQLParams.create() p.addString(null) } }", "Invalid SQL method argument types");
    rejects("class App { start() { final db = SQL.open('sqlite::memory:') db.query('SELECT 1') } }", "expects 2 arguments");
    rejects("class App { start() { final db = SQL.open('sqlite::memory:') db.query('SELECT 1', null) } }", "Invalid SQL method argument types");
    rejects("class App { start() { final db = SQL.open('sqlite::memory:') db.missing() } }", "Unknown SQL method");
}

#[test]
fn orm_models_and_fluent_queries_are_checked_before_execution() {
    rejects("class User(string name) class App { start() { Orm.model(SQL.open('sqlite::memory:'), User, 'users') } }", "require a non-null int or long field named id");
    rejects("class Address(string city) class User(int id, Address address) class App { start() { Orm.model(SQL.open('sqlite::memory:'), User, 'users') } }", "must be int, long, decimal, bool, string");
    rejects("class User(int id, string name) class App { start() { final users = Orm.model(SQL.open('sqlite::memory:'), User, 'users') users.query().where('typo', '=', 1) } }", "Unknown ORM column 'typo'");
    rejects("class User(int id, string name) class App { start() { final users = Orm.model(SQL.open('sqlite::memory:'), User, 'users') users.query().where('name', '=', 1) } }", "expects string");
    rejects("class User(int id) class Other(int id) class App { start() { final users = Orm.model(SQL.open('sqlite::memory:'), User, 'users') users.create(Other(1)) } }", "Invalid ORM argument types");
    rejects("class User(int id) class App { start() { final users = Orm.model(SQL.open('sqlite::memory:'), User(1), 'users') } }", "model must be a class name");
    rejects("class User(int id) class App { start() { final users = Orm.model(SQL.open('sqlite::memory:'), User, 'users') users.delete(null) } }", "Invalid ORM argument types");
    rejects("class App { start() { OrmModel() } }", "ORM handles cannot be constructed");
    rejects("class User(int id) class App { start() { final users = Orm.model(SQL.open('sqlite::memory:'), User, 'users') users.query().where('id', '=', null) } }", "where requires a non-null scalar");
}

#[test]
fn orm_runtime_guards_reject_unsafe_queries_and_incompatible_rows() {
    runs(r#"
class User(long id, string name, bool active)
class App {
    start() {
        final db = SQL.open('sqlite::memory:')
        final users = Orm.model(db, User, 'select')
        final query = users.query()
        final params = SQLParams.create()
        try {
            users.migrate()
            users.create(User(1L, 'Ada', false))
            final column = 'id; DROP TABLE users'
            try { query.where(column, '=', 1L) } catch (SQLError err) { print('column rejected') }
            try { query.where('id', '= 1; DELETE FROM users', 1L) } catch (SQLError err) { print('operator rejected') }
            try { query.orderBy('id', 'asc; DROP TABLE users') } catch (SQLError err) { print('direction rejected') }
            try { query.limit(-1) } catch (SQLError err) { print('limit rejected') }
            print(query.count())
            // SQLite's dynamic typing permits an integer outside a boolean's value set.
            db.execute('UPDATE "select" SET active = 7 WHERE id = 1', params)
            try { users.get(1L) } catch (SQLError err) { print('row type rejected') }
            db.execute('UPDATE "select" SET active = 0 WHERE id = 1', params)
            print(users.get(1L).active())
            query.close()
            try { query.count() } catch (SQLError err) { print('closed query rejected') }
        } finally { params.close() query.close() users.close() db.close() }
    }
}
"#, "column rejected\noperator rejected\ndirection rejected\nlimit rejected\n1\nrow type rejected\nfalse\nclosed query rejected\n");
}

#[test]
fn orm_join_columns_aliases_and_related_filters_are_checked() {
    let prefix = "class User(long id, string name) class Order(long id, long userId, bool active) class App { start() { final db = SQL.open('sqlite::memory:') final users = Orm.model(db, User, 'users') final orders = Orm.model(db, Order, 'orders') final q = users.query() ";
    for (body,message) in [
        ("q.join(orders,'o','missing','userId')", "Unknown ORM column 'missing'"),
        ("q.join(orders,'o','id','missing')", "Unknown ORM column 'missing'"),
        ("q.join(orders,'o','name','userId')", "compatible types"),
        ("q.join(orders,'base','id','userId')", "alias must be unique"),
        ("q.join(orders,'o','id','userId').join(orders,'O','id','userId')", "alias must be unique"),
        ("q.join(orders,'o','id','userId').where('o.active','=',1)", "expects bool"),
        ("q.join(orders,'o','id','userId').where('o.missing','=',true)", "Unknown ORM column 'missing'"),
        ("q.join(orders,'o','id','userId').orderBy('unknown.id','asc')", "Unknown ORM join alias"),
        ("q.join(orders,'o','id')", "join expects"),
    ] { rejects(&format!("{prefix}{body} }} }}"),message); }
}

#[test]
fn request_body_requires_instance_and_no_arguments() {
    rejects("class App { start() { Request.body() } }", "requires an instance");
    rejects("class App { start() { final server = Server() server.post('/', request -> Response.ok(request.body('extra'))) } }", "expects 0 arguments");
}

#[test]
fn money_currency_mismatch_is_catchable_and_runs_finally() {
    runs(r#"
        class App {
            Money subtract() { return Money.php(10.0).subtract(Money.usd(5.0)) }
            Money failAsync() { return this.subtract() }
            start() {
                final original = Money.php(10.0)
                try { print(original.add(Money.usd(5.0)).amount()) }
                catch (CurrencyMismatchError error) { print(error.message()) }
                finally { print('add cleanup') }
                try { print(this.subtract().amount()) }
                catch (CurrencyMismatchError error) { print(error.message()) }
                finally { print('subtract cleanup') }
                try { final value = Async.start(() -> this.failAsync()).await() }
                catch (CurrencyMismatchError error) { print('async recovered') }
                try { original.add(Money.eur(5.0)) }
                catch (Error error) { print(error.message()) }
                try { throw CurrencyMismatchError('explicit mismatch') }
                catch (CurrencyMismatchError error) { print(error.message()) }
                print(original.amount() == 10.0)
                print(original.add(Money.php(5.0)).amount() == 15.0)
                print(original.subtract(Money.php(5.0)).amount() == 5.0)
                print('still running')
            }
        }
    "#, "Money currency mismatch: PHP vs USD\nadd cleanup\nMoney currency mismatch: PHP vs USD\nsubtract cleanup\nasync recovered\nMoney currency mismatch: PHP vs EUR\nexplicit mismatch\ntrue\ntrue\ntrue\nstill running\n");
}

#[test]
fn unhandled_money_currency_mismatch_uses_normal_exception_reporting() {
    for operation in ["add", "subtract"] {
        let source = format!("class App {{ start() {{ try {{ Money.php(1.0).{operation}(Money.usd(1.0)) }} finally {{ print('cleanup') }} print('unreachable') }} }}");
        let output = native(&source);
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout, b"cleanup\n");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("unhandled exception CurrencyMismatchError"), "{stderr}");
        assert!(stderr.contains("PHP vs USD"), "{stderr}");
    }
}

#[test]
fn exact_money_api_checks_receiver_arity_and_non_null_text() {
    for (source, expected) in [
        ("Money.parse('1')", "2 arguments"),
        ("Money.parse(1, 'PHP')", "non-null parameter types"),
        ("Money.parse(null, 'PHP')", "non-null parameter types"),
        ("Money.amountText()", "an instance"),
        ("Money.php(1.0).parse('1', 'PHP')", "a class call"),
        ("Money.php(1.0).multiplyPercent(12)", "non-null parameter types"),
    ] {
        rejects(&format!("class App {{ start() {{ {source} }} }}"), expected);
    }
}

#[test]
fn gc_api_requires_static_calls_and_no_arguments() {
    for source in ["Gc()", "Gc.collect(1)", "Gc.objects(1)", "Gc.reclaimed(1)"] {
        let errors = diagnostics(&format!("class App {{ start() {{ {source} }} }}"));
        assert!(!errors.is_empty(), "invalid collector call accepted: {source}");
    }
}

#[test]
fn task_combinators_check_literal_elements_and_outcome_calls() {
    for method in ["all", "any", "race", "allSettled"] {
        rejects(&format!("class App {{ start() {{ final result = Async.{method}([1]).await() }} }}"), "only task elements");
    }
    rejects("class App { start() { final results = Async.allSettled([Async.start(() -> 1)]).await() final result = results['bad'] } }", "index must be int");
    rejects("class App { start() { TaskOutcome() } }", "does not create instances");
    rejects("class App { start() { final results = Async.allSettled([Async.start(() -> 1)]).await() final result = results.get('bad') } }", "Invalid task outcome method call");
}

#[test]
fn stacktrace_records_nested_calls_and_preserves_original_throw() {
    let output = native(r#"class Problem(string message)
class App {
    fail() { throw Problem('boom') }
    relay() { this.fail() }
    finished() { return }
    start() {
        this.finished()
        try { this.relay() } catch (Problem error) {
            final original = error.stacktrace()
            Console.printLine(original)
            try { throw error } catch (Error again) {
                Console.printLine(original == again.stacktrace())
            }
        }
    }
}"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.starts_with("Problem: boom\n  at App.fail (3:"), "{text}");
    let fail = text.find("at App.fail").unwrap();
    let relay = text.find("at App.relay (4:").unwrap();
    let main = text.find("at App.start (8:").unwrap();
    assert!(fail < relay && relay < main, "{text}");
    assert!(!text.contains("App.finished"), "{text}");
    assert!(!text.contains("Problem.message"), "{text}");
    assert!(text.ends_with("true\n"), "{text}");
}

#[test]
fn stacktrace_captures_runtime_errors_and_unhandled_output() {
    let caught = native(r#"class App {
    fail() { final cache=Cache.memory(0) }
    start() { try { this.fail() } catch(Error error) { Console.printLine(error.stacktrace()) } }
}"#);
    assert!(caught.status.success(), "{}", String::from_utf8_lossy(&caught.stderr));
    let text=String::from_utf8_lossy(&caught.stdout);
    assert!(text.contains("at App.fail (2:"), "{text}");
    assert!(text.contains("at App.start (3:"), "{text}");
    let unhandled=native("class App { fail() { throw 'boom' } start() { this.fail() } }");
    assert!(!unhandled.status.success());
    let stderr=String::from_utf8_lossy(&unhandled.stderr);
    assert!(stderr.contains("unhandled exception GenericError: boom"), "{stderr}");
    assert!(stderr.contains("at App.fail (1:"), "{stderr}");
    assert!(stderr.contains("at App.start (1:"), "{stderr}");
}

#[test]
fn async_stacktrace_retains_throwing_task_without_stale_frames() {
    let output=native(r#"class App {
    fail() { throw 'async boom' }
    done() { return }
    start() {
        try { Async.start(() -> this.fail()).await() } catch(Error error) { Console.printLine(error.stacktrace()) }
        this.done()
        try { throw 'later' } catch(Error error) { Console.printLine(error.stacktrace()) }
    }
}"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text=String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("GenericError: async boom\n  at App.fail (2:"), "{text}");
    let later=text.split("GenericError: later").nth(1).unwrap();
    assert!(later.contains("at App.start (7:"), "{text}");
    assert!(!later.contains("App.fail"), "{text}");
    assert!(!later.contains("App.done"), "{text}");
}

#[test]
fn stacktrace_requires_an_error_instance_and_zero_arguments() {
    rejects("class App { start() { GenericError.stacktrace() } }", "error instance");
    rejects("class App { start() { try { throw 'bad' } catch(Error e) { e.stacktrace(1) } } }", "no arguments");
}

#[test]
fn stacktrace_supports_error_compatible_traits_and_unthrown_instances() {
    runs(r#"trait Retryable { string message() bool retryable() }
    class Problem(string message,bool retryable)
    class App { start() {
        Console.printLine(Problem('not thrown',true).stacktrace() == '')
        try { throw Problem('retry',true) } catch(Retryable error) { Console.printLine(error.stacktrace()) }
    }}"#, "true\nProblem: retry\n  at App.start (5:21)\n");
}

#[test]
fn native_math_arguments_match_decimal_and_integer_abis() {
    runs(r#"class App { start() {
        Console.printLine(Math.sqrt(9))
        Console.printLine(Math.pow(2,3))
        Console.printLine(Math.isNaN(Math.sqrt(-1)))
        Console.printLine(Math.gcd(12,8))
        Console.printLine(Math.isEven(4))
        Console.printLine(Math.round(1.234,2))
    }}"#, "3\n8\ntrue\n4\ntrue\n1.23\n");
}

#[test]
fn text_block_templates_interpolate_expressions_escapes_and_nested_templates() {
    runs(r#"class App {
      int next() { print('evaluated') return 4 }
      start() {
        final name = 'Lem'
        final count = 3
        print(`Hello ${name}
Items: ${count * 2}`)
        print(`escaped: \${missing} \` \\ \nend`)
        print(`nested: ${`hi ${name}`} ${'}'} ${true then 'yes' else 'no'}`)
        print(`once: ${this.next()}`)
        print(`unicode: \u03bb \u{1F600} \x41`)
        print(`joined\
line`)
        final raw = `\${name}`
        print(`not recursive: ${raw}`)
      }
    }"#, "Hello Lem\nItems: 6\nescaped: ${missing} ` \\ \nend\nnested: hi Lem } yes\nevaluated\nonce: 4\nunicode: λ 😀 A\njoinedline\nnot recursive: ${name}\n");
}

#[test]
fn text_block_templates_capture_locals_in_closures_and_async_tasks() {
    runs(r#"class App { start() {
      final name = 'Lem'
      final count = 3
      final render = () -> `Hello ${name}: ${count * 2}`
      print(render())
      final task = Async.start(() -> `Async ${name}: ${count}`)
      print(task.await())
    } }"#, "Hello Lem: 6\nAsync Lem: 3\n");
}

#[test]
fn text_block_templates_reject_invalid_expressions_and_escapes() {
    for (source, expected) in [
        ("class App { start() { print(`Hello ${missing}`) } }", "Undeclared identifier"),
        ("class App { start() { print(`Hello ${}`) } }", "expression"),
        ("class App { start() { print(`Hello ${1 2}`) } }", "one expression"),
        ("class App { start() { print(`Hello ${print('x')}`) } }", "void"),
        (r"class App { start() { print(`\uZZZZ`) } }", "hexadecimal"),
        (r"class App { start() { print(`\u{110000}`) } }", "Unicode"),
        (r"class App { start() { print(`\0`) } }", "Numeric escapes"),
    ] { rejects(source, expected); }
}

#[test]
fn text_block_interpolation_diagnostics_keep_source_positions() {
    let source = "class App { start() {\nprint(`first line\n${missing + 1}`)\n} }";
    let mut parser = Parser::new(Lexer::new(source));
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "{:?}", parser.diagnostics);
    let mut checker = TypeChecker::new();
    assert!(!checker.check_program(&program));
    let diagnostic = checker.diagnostics.iter().find(|d| d.message.contains("Undeclared identifier")).unwrap();
    let span = diagnostic.span.unwrap();
    assert_eq!(span.start.index, source.find("missing").unwrap());
    assert_eq!((span.start.line, span.start.column), (3, 3));
}
