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
fn native(source: &str) -> Output {
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
    Command::new("timeout").arg("15s").arg(exe).output().unwrap()
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
    runs(r#"class A(string message, int code) {} class B(string message) {}
    class Application { main() { try { throw A('bad', 42) }
      catch (B err) { print('wrong') } catch (A err) { print(err.message()) print(err.code()) }
      catch (Error err) { print('wrong general') } } }"#, "bad\n42\n");
}
#[test]
fn strings_are_boxed_once_and_error_trait_dispatches() {
    runs(r#"class Application { string message() { print('called') return 'boom' }
    main() { try { throw this.message() } catch (Error err) { print(err.message()) } } }"#, "called\nboom\n");
}
#[test]
fn structural_category_matches_independent_classes() {
    runs(r#"trait Retryable { string message() bool retryable() }
    class A(string message, bool retryable) {} class B(string message) { bool retryable() { return true } }
    class Application { main() {
      try { throw A('a', false) } catch (Retryable err) { print(err.message()) print(err.retryable()) }
      try { throw B('b') } catch (Retryable err) { print(err.message()) print(err.retryable()) }
    } }"#, "a\nfalse\nb\ntrue\n");
}
#[test]
fn independent_traits_match_in_source_order() {
    runs(r#"trait Retryable { string message() bool retryable() }
    trait Security { string message() string securityCode() }
    class Both(string message, bool retryable, string securityCode) {}
    class Application { main() { try { throw Both('both', true, 'AUTH') }
      catch (Security err) { print(err.securityCode()) }
      catch (Retryable err) { print('wrong') } } }"#, "AUTH\n");
}
#[test]
fn unmatched_errors_propagate_through_calls_and_cleanup_only_try() {
    runs(r#"class A(string message) {} class B(string message) {}
    class Application { fail() { throw A('boom') } relay() { this.fail() }
    main() { try { try { this.relay() } catch (B err) { print('wrong') } finally { print('inner') } }
      catch (A err) { print(err.message()) } finally { print('outer') } } }"#, "inner\nboom\nouter\n");
}
#[test]
fn try_finally_does_not_swallow_errors() {
    runs(r#"class Application { main() { try { try { throw 'boom' } finally { print('cleanup') } }
      catch (Error err) { print(err.message()) } } }"#, "cleanup\nboom\n");
}
#[test]
fn accidental_conformance_requires_explicit_throw() {
    runs(r#"class Customer(string message) {} class Application { main() {
      final customer = Customer('hello') print('normal')
      try { throw customer } catch (Customer err) { print(err.message()) } } }"#, "normal\nhello\n");
}
#[test]
fn finally_runs_on_normal_completion_and_matching_catch() {
    runs(r#"class Application { main() {
      try { print('normal') } finally { print('first') }
      try { throw 'boom' } catch (Error err) { print('handled') } finally { print('second') }
    } }"#, "normal\nfirst\nhandled\nsecond\n");
}
#[test]
fn nested_finally_runs_once_for_return_and_catch_return() {
    runs(r#"class Application {
      int value() { try { try { return 42 } finally { print('inner') } } finally { print('outer') } }
      int handled() { try { throw 'boom' } catch (Error err) { return 7 } finally { print('catch cleanup') } }
      main() { print(this.value()) print(this.handled()) }
    }"#, "inner\nouter\n42\ncatch cleanup\n7\n");
}
#[test]
fn loop_exits_cleanup_only_regions_being_exited() {
    runs(r#"class Application { main() {
      try { for (x in [1,2]) { try { if (x == 1) { continue } break } finally { print(x) } } print('done') }
      finally { print('outer') }
    } }"#, "1\n2\ndone\nouter\n");
}
#[test]
fn exception_from_handler_runs_finally_before_outer_handler() {
    runs(r#"class Application { main() { try {
      try { throw 'first' } catch (Error err) { throw 'second' } finally { print('inner') }
    } catch (Error err) { print(err.message()) } } }"#, "inner\nsecond\n");
}
#[test]
fn cleanup_error_replaces_pending_error_and_preserves_it_as_suppressed() {
    let output = native(r#"class Application { main() { try { throw 'original' } finally { throw 'cleanup' } } }"#);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("GenericError: cleanup"), "{stderr}");
    assert!(stderr.contains("suppressed: original"), "{stderr}");
}
#[test]
fn cleanup_return_overrides_pending_return() {
    runs(r#"class Application { int value() { try { return 1 } finally { return 2 } }
      main() { print(this.value()) } }"#, "2\n");
}
#[test]
fn composition_and_optional_causes_keep_original_concrete_objects() {
    runs(r#"class Details(string message, string code) {}
      class Root(string message) {}
      class PaymentError(Details details, Error? cause = null) {
        string message() { return this._details.message() }
        string code() { return this._details.code() }
      }
      class Application { main() {
        final root = Root('root')
        try { throw PaymentError(Details('payment', 'PAY'), root) }
        catch (PaymentError err) {
          print(err.message()) print(err.code())
          try { throw (err.cause() ?? GenericError('missing')) } catch (Root cause) { print(cause.message()) }
        }
      } }"#, "payment\nPAY\nroot\n");
}
#[test]
fn guarded_catches_narrow_before_condition_and_continue_on_false() {
    runs(r#"trait Retryable { string message() bool retryable() }
      class A(string message, bool retryable) {}
      class Application { main() { try { throw A('boom', false) }
        catch (Retryable err) if (err.retryable()) { print('wrong') }
        catch (A err) { print(err.message()) }
      } }"#, "boom\n");
}
#[test]
fn throwing_catch_condition_propagates_after_cleanup() {
    runs(r#"class A(string message) { bool test() { throw 'condition' } }
      class Application { main() { try { try { throw A('original') }
        catch (A err) if (err.test()) { print('wrong') } finally { print('cleanup') }
      } catch (Error err) { print(err.message()) } } }"#, "cleanup\ncondition\n");
}
#[test]
fn async_failures_keep_type_fields_causes_and_finally_order() {
    runs(r#"class DbError(string message, int code) {}
      class Application { async int fail() { try { throw DbError('db', 99) } finally { print('worker cleanup') } }
        async int relay() { return await this.fail() }
        main() { try { final value = await this.relay() } catch (DbError err) { print(err.message()) print(err.code()) } finally { print('awaiter cleanup') } }
      }"#, "worker cleanup\ndb\n99\nawaiter cleanup\n");
}
#[test]
fn timeout_and_closed_channel_failures_are_generic_errors() {
    runs(r#"class Application { main() {
      final ch = Channel<int>(1)
      try { final value = await ch.receive().timeout(5) } catch (GenericError err) { print(err.message()) }
      ch.close()
      try { final value = await ch.receive() } catch (Error err) { print(err.message()) }
    } }"#, "task timed out\nchannel is closed\n");
}
#[test]
fn cancellation_unblocks_await_and_runs_worker_cleanup() {
    runs(r#"class Application {
      async wait(Channel<int> ready, Channel<int> blocked) {
        try { final sent = await ready.send(1) final value = await blocked.receive() }
        finally { print('worker cleanup') }
      }
      main() { final ready = Channel<int>(1) final blocked = Channel<int>(1)
        final task = this.wait(ready, blocked) final started = await ready.receive()
        final cancelled = task.cancel()
        try { final value = await task } catch (Error err) { print(err.message()) }
      }
    }"#, "worker cleanup\ntask was cancelled\n");
}

#[test]
fn cancellation_allows_async_cleanup_and_preserves_original_failure() {
    runs(r#"class Application {
      async work(Channel<int> ready, Channel<int> blocked, Channel<int> completed) {
        try {
          final sent = await ready.send(1)
          final value = await blocked.receive()
        } finally {
          print('cleanup started')
          try { final value = await blocked.receive().timeout(5) }
          catch (Error err) { print(err.message()) }
          final sent = await completed.send(7)
          print('cleanup completed')
        }
      }
      main() {
        final ready = Channel<int>(1)
        final blocked = Channel<int>(1)
        final completed = Channel<int>(1)
        final task = this.work(ready, blocked, completed)
        final started = await ready.receive()
        final cancelled = task.cancel()
        try { final value = await task }
        catch (Error err) { print(err.message()) }
        print(await completed.receive().timeout(1000))
      }
    }"#, "cleanup started\ntask timed out\ncleanup completed\ntask was cancelled\n7\n");
}

#[test]
fn unhandled_sync_and_async_errors_exit_unsuccessfully() {
    for source in [r#"class Application { main() { throw 'sync' } }"#,
      r#"class Application { async fail() { throw 'async' } main() { final value = await this.fail() } }"#] {
        let output = native(source);
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("unhandled exception GenericError"));
    }
}
#[test]
fn invalid_throw_operands_are_rejected() {
    for value in ["null", "1", "true", "Potato(1)"] {
        rejects(&format!("class Potato(int value) {{}} class Application {{ main() {{ throw {value} }} }}"), "throw target must");
    }
    rejects(r#"class Wrong { int message() { return 1 } } class Application { main() { throw Wrong() } }"#, "throw target must");
}
#[test]
fn invalid_and_unreachable_catch_types_are_rejected() {
    for ty in ["Missing", "Potato", "NotError"] {
        rejects(&format!("trait NotError {{ bool retryable() }} class Potato(int value) {{}} class Application {{ main() {{ try {{ print(1) }} catch ({ty} err) {{}} }} }}"), "catch type");
    }
    rejects(r#"class A(string message) {} class Application { main() { try { throw A('x') } catch (Error err) {} catch (A err) {} } }"#, "Unreachable catch");
    rejects(r#"trait R { string message() bool retryable() } class A(string message, bool retryable) {}
      class Application { main() { try { throw A('x', true) } catch (R err) {} catch (A err) {} } }"#, "Unreachable catch");
}
#[test]
fn catch_scopes_and_static_types_are_checked() {
    rejects(r#"class A(string message, int code) {} class Application { main() { try { throw A('x',1) } catch (A err) { print(err.missing()) } } }"#, "Member");
    rejects(r#"class Application { main() { try { throw 'x' } catch (Error err) {} print(err) } }"#, "err");
    rejects(r#"class Application { main() { try { final local = 1 } finally { print(local) } } }"#, "local");
    rejects(r#"class Application { main() { try { throw 'x' } catch (Error err) if (1) {} } }"#, "catch condition");
}
#[test]
fn incomplete_try_and_legacy_cleanup_syntax_are_rejected() {
    rejects("class Application { main() { try {} } }", "try requires");
    for code in ["guard {}", "after file.close()", "try {} exit {}", "try {} catch(err) {}"] {
        assert!(!diagnostics(&format!("class Application {{ main() {{ {code} }} }}")).is_empty(), "accepted {code}");
    }
}

#[test]
fn runtime_file_and_directory_failures_are_catchable_and_run_cleanup() {
    let missing = std::env::temp_dir().join(format!("ube-missing-{}", std::process::id())).join("file");
    let path = missing.to_string_lossy();
    runs(&format!(r#"class Application {{ main() {{
      try {{ final value = File.read('{path}') print('wrong') }}
      catch (FileNotFoundError err) {{ print('read') }} finally {{ print('cleanup') }}
      try {{ File.write('{path}', 'data') print('wrong') }} catch (Error err) {{ print('write') }}
      try {{ File.append('{path}', 'data') }} catch (Error err) {{ print('append') }}
      try {{ File.delete('{path}') }} catch (FileNotFoundError err) {{ print('delete') }}
      try {{ final info = File.info('{path}') }} catch (FileNotFoundError err) {{ print('info') }}
      try {{ final entries = Directory.list('{path}') }} catch (FileNotFoundError err) {{ print('list') }}
      try {{ Directory.delete('{path}') }} catch (FileNotFoundError err) {{ print('directory delete') }}
      print(File.exists('{path}'))
    }} }}"#), "read\ncleanup\nwrite\nappend\ndelete\ninfo\nlist\ndirectory delete\nfalse\n");
}

#[test]
fn runtime_file_handles_distinguish_empty_content_closed_handles_and_binary_data() {
    let path = std::env::temp_dir().join(format!("ube-file-handles-{}", std::process::id()));
    let path = path.to_string_lossy();
    runs(&format!(r#"class Application {{ main() {{
      File.write('{path}', '')
      print(File.read('{path}'))
      File.write('{path}', 'hello')
      final file = File.open('{path}')
      print(file.read())
      file.close()
      try {{ print(file.read()) }} catch (Error err) {{ print('closed') }}
      File.delete('{path}')
    }} }}"#), "\nhello\nclosed\n");
    let binary = std::env::temp_dir().join(format!("ube-binary-{}", std::process::id()));
    fs::write(&binary, b"hello\0world").unwrap();
    runs(&format!(r#"class Application {{ main() {{
      try {{ print(File.read('{}')) }} catch (Error err) {{ print('binary rejected') }}
    }} }}"#, binary.display()), "binary rejected\n");
    fs::remove_file(binary).unwrap();
}

#[test]
fn runtime_server_startup_failures_are_catchable() {
    let reserved = std::net::TcpListener::bind(("127.0.0.1", 0)).ok();
    let port = reserved.as_ref().map(|listener| listener.local_addr().unwrap().port()).unwrap_or(8080);
    runs(&format!(r#"class Application {{ main() {{
      final app = server()
      try {{ app.start(-1) print('wrong') }} catch (Error err) {{ print('invalid port') }}
      try {{ app.start({port}) print('wrong') }} catch (Error err) {{ print('bind failed') }}
      finally {{ print('cleanup') }}
      print('recovered')
    }} }}"#), "invalid port\nbind failed\ncleanup\nrecovered\n");
}

#[test]
fn unhandled_runtime_io_errors_exit_unsuccessfully() {
    let output = native(r#"class Application { main() { File.read('/ube-nonexistent-directory/file') print('wrong') } }"#);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("FileNotFoundError"));
}

#[test]
fn native_list_indexing_compiles_with_and_without_handlers() {
    runs(r#"class Application {
      int first() { final values = [42] return values[0] }
      main() {
        print(this.first())
        final values = [10, 20]
        try { print(values[0]) print(values[1]) }
        catch (Error err) { print('unexpected error') }
        finally { print('cleanup') }
      }
    }"#, "42\n10\n20\ncleanup\n");
}

#[test]
fn native_list_bounds_errors_are_caught_and_run_cleanup() {
    runs(r#"class Application { main() {
      final values = [42]
      try { print(values[-1]) print('unreachable') }
      catch (Error err) { print('negative index') }
      finally { print('negative cleanup') }
      try { print(values[1]) print('unreachable') }
      catch (Error err) { print('past end') }
      finally { print('past end cleanup') }
      print(values[0])
    } }"#, "negative index\nnegative cleanup\npast end\npast end cleanup\n42\n");
}

#[test]
fn native_bare_return_needs_no_semicolon_and_runs_cleanup() {
    runs(r#"class Application {
      stop() {
        try {
          print('before')
          return
          print('unreachable')
        } finally { print('cleanup') }
      }
      int value() { return (
        42
      ) }
      main() { this.stop() print(this.value()) }
    }"#, "before\ncleanup\n42\n");
}

#[test]
fn backing_fields_are_private_to_the_declaring_class() {
    for statement in ["print(user._age)", "user._age = 2", "print(user._name)", "user._name = 'B'"] {
        rejects(&format!("class User(string name, var int age) {{}} class Application {{ main() {{ final user = User('A', 1) {statement} }} }}"), "is private to class 'User'");
    }
}

#[test]
fn public_fields_use_getter_and_setter_calls() {
    runs(r#"class User(string name, var int age) {
      birthday() { this._age = this._age + 1 }
      int read() { return this._age }
    }
    class Application { main() {
      final user = User('A', 1)
      final shared = user
      print(user.name())
      print(user.age())
      user.age(2)
      user.birthday()
      print(user.read())
      print(shared.age())
    } }"#, "A\n1\n3\n3\n");
}

#[test]
fn public_field_access_and_setter_arguments_are_checked() {
    for (statement, expected) in [
        ("print(user.age)", "must be read using 'age()'"),
        ("user.age = 2", "must be written using 'age(value)'"),
        ("user.age('bad')", "setter 'age' expects"),
        ("user.age(1, 2)", "requires exactly one argument"),
        ("user.name('B')", "cannot set immutable field 'name'"),
        ("User.age(2)", "requires an instance of 'User'"),
        ("User.age()", "requires an instance of 'User'"),
    ] {
        rejects(&format!("class User(string name, var int age) {{}} class Application {{ main() {{ final user = User('A', 1) {statement} }} }}"), expected);
    }
}

#[test]
fn native_declarations_zero_initialize_without_constructor_defaults() {
    runs(r#"class Defaults {
      int load() { print('default evaluated') return 99 }
    }
    class Record(int count = Defaults().load(), string label = 'configured') {}
    class Application { main() {
      Record zero
      print(zero.count())
      print(zero.label())
      final explicit = Record()
      print(explicit.count())
      print(explicit.label())
      Record? absent
      print(absent == null)
    } }"#, "0\n\ndefault evaluated\n99\nconfigured\ntrue\n");
}

#[test]
fn native_zero_initialization_recurses_without_sharing_or_defaults() {
    runs(r#"class Point(var int x = 42) {}
    class Pair(Point left, Point right) {}
    class Node(int value = 99, Node? next = null) {}
    class Application { main() {
      Pair pair
      pair.left().x(7)
      print(pair.left().x())
      print(pair.right().x())
      Pair other
      print(other.left().x())
      Node node
      print(node.value())
      print(node.next() == null)
    } }"#, "7\n0\n0\n0\ntrue\n");
}

#[test]
fn native_primitive_declarations_keep_zero_values() {
    runs(r#"class Settings(int count = 7, long id = 9L, decimal ratio = 1.5,
      bool enabled = true, string text = 'configured') {}
    class Application { main() {
      int count
      long id
      decimal ratio
      bool enabled
      string text
      print(count) print(id) print(ratio) print(enabled) print(text)
      Settings settings
      print(settings.count()) print(settings.id()) print(settings.ratio())
      print(settings.enabled()) print(settings.text())
    } }"#, "0\n0\n0\nfalse\n\n0\n0\n0\nfalse\n\n");
}

#[test]
fn unsafe_zero_initialization_requires_nullable_links_or_explicit_initializers() {
    rejects("class Node(Node next) {} class Application { main() { Node node } }", "recursive non-nullable class");
    rejects("class A(B b) {} class B(A a) {} class Application { main() { A value } }", "recursive non-nullable class");
    rejects("class Application { main() { Server app } }", "requires an explicit initializer");
    rejects("class Application { main() { Channel<int> values } }", "requires an explicit initializer");
    rejects("trait Named { string name() } class Application { main() { Named value } }", "requires an explicit initializer");
}

#[test]
fn native_startup_uses_the_same_zero_values_as_local_declarations() {
    runs(r#"class Defaults {
      int load() { print('unexpected default evaluation') return 123 }
    }
    class Child(int count = 77) {}
    class Application(int count = Defaults().load(), long support = 44L,
      string caption = 'configured', Child child = Child(), Child? absent = Child()) {
      main() {
        print(this.count())
        print(this.support())
        print(this.caption())
        print(this.child().count())
        print(this.absent() == null)
        Application other
        print(other.child().count())
        final configured = Child()
        print(configured.count())
      }
    }"#, "0\n0\n\n0\ntrue\n0\n77\n");
}

#[test]
fn unsafe_startup_layouts_are_rejected_before_codegen() {
    rejects("class Application(Application next) { main() {} }", "recursive non-nullable class");
    rejects("class Application(Server app = Server()) { main() {} }", "requires an explicit initializer");
}

#[test]
fn removed_boolean_operators_are_rejected_in_expressions() {
    for operator in ["nand", "nor", "xor", "xnor"] {
        let source = format!("class Application {{ main() {{ print(true {operator} false) }} }}");
        let mut parser = Parser::new(Lexer::new(&source));
        parser.parse_program();
        assert!(!parser.diagnostics.is_empty(), "Accepted removed operator {operator}");
    }
}

#[test]
fn native_boolean_logic_keeps_not_and_or_precedence() {
    runs(r#"class Application { main() {
      print(not true) print(not false)
      print(true and true) print(true and false)
      print(false or true) print(false or false)
      print(true or false and false)
      print((true or false) and false)
      print(not (true and false))
      print(not (true or false))
      final nand = 1
      final nor = 2
      final xor = 3
      final xnor = 4
      print(nand + nor + xor + xnor)
    } }"#, "false\ntrue\ntrue\nfalse\ntrue\nfalse\ntrue\nfalse\ntrue\nfalse\n10\n");
}

#[test]
fn native_backtick_text_blocks_preserve_json_and_do_not_interpolate() {
    runs(r#"class Consumer { string echo(string value) { return value } }
    class Application { main() {
      print('A')
      print('one line')
      print(`{"name":"Ube","value":"${notDeclared}","line":"\n"}`)
      print(`first
  second`)
      print(`trailing\`)
      print('after')
      final consumer = Consumer()
      print(consumer.echo(`{"message":"hello","active":true}`))
    } }"#, "A\none line\n{\"name\":\"Ube\",\"value\":\"${notDeclared}\",\"line\":\"\\n\"}\nfirst\n  second\ntrailing\\\nafter\n{\"message\":\"hello\",\"active\":true}\n");
}

#[test]
fn double_quotes_are_reserved_and_single_quotes_cannot_span_lines() {
    rejects("class Application { main() { print(\"reserved\") } }", "Double quotes are reserved for JSON");
    rejects("class Application { main() { print('first\nsecond') } }", "Unescaped line break");
    rejects("class Application { main() { print(`unfinished) } }", "Unterminated backtick text block");
}

#[test]
fn nullable_scalars_survive_tasks_channels_and_captures() {
    runs(r#"class Application {
      async int? score(int? value) { return value }
      main() {
        final ch = Channel<int?>(2)
        int? missing = await this.score(null)
        int? zero = await this.score(0)
        print(missing == null)
        print(zero == null)
        print(zero ?? 99)
        await ch.send(missing)
        await ch.send(zero)
        int? first = await ch.receive()
        int? second = await ch.receive()
        print(first == null)
        print(second ?? 99)
        final task = async (missing)
        int? captured = await task
        print(captured == null)
      }
    }"#, "true\nfalse\n0\ntrue\n0\ntrue\n");
}

#[test]
fn nullable_false_and_decimal_zero_are_present() {
    runs(r#"class Application {
      async bool? truth(bool? value) { return value }
      async decimal? amount(decimal? value) { return value }
      main() {
        final ch = Channel<bool?>(2)
        await ch.send(null)
        await ch.send(await this.truth(false))
        bool? missing = await ch.receive()
        bool? present = await ch.receive()
        print(missing == null)
        print(present == null)
        print(present ?? true)
        final amounts = Channel<decimal?>(2)
        await amounts.send(await this.amount(null))
        await amounts.send(await this.amount(0.0))
        decimal? a = await amounts.receive()
        decimal? b = await amounts.receive()
        print(a == null)
        print(b == null)
        print(b ?? 99.0)
      }
    }"#, "true\nfalse\nfalse\ntrue\nfalse\n0\n");
}

#[test]
fn nullable_fields_calls_and_presence_conditions_work_natively() {
    runs(r#"class User(var int? age) {}
      class Application {
        int? echo(int? value) { return value }
        async long? wide(long? value) { return value }
        main() {
          final user = User(null)
          print(user.age())
          user.age(0)
          print(user.age())
          print(null == user.age())
          int? empty
          if (empty) { print('wrong') } else { print('empty') }
          int? zero = this.echo(0)
          if (zero) { print('present') }
          print(this.echo(null))
          final wide = Channel<long?>(2)
          await wide.send(await this.wide(null))
          await wide.send(await this.wide(9223372036854775807L))
          long? a = await wide.receive()
          long? b = await wide.receive()
          print(a)
          print(b)
          final bytes = Channel<byte?>(2)
          byte value
          await bytes.send(null)
          await bytes.send(value)
          byte? c = await bytes.receive()
          byte? d = await bytes.receive()
          print(c)
          print(d)
        }
      }"#, "null\n0\nfalse\nempty\npresent\nnull\nnull\n9223372036854775807\nnull\n0\n");
}

#[test]
fn nullable_payloads_do_not_hide_closure_failure_or_repeat_awaits() {
    runs(r#"class Application {
      async int? absent() { return null }
      async int? fail() { throw 'failed' }
      main() {
        final ch = Channel<int?>(1)
        await ch.send(null)
        ch.close()
        int? value = await ch.receive()
        print(value == null)
        try { int? closed = await ch.receive() print('wrong') }
        catch (Error err) { print('closed') }
        try { int? failed = await this.fail() print('wrong') }
        catch (Error err) { print(err.message()) }
        final task = this.absent().timeout(1000)
        int? first = await task
        int? second = await task
        print(first == null)
        print(second == null)
      }
    }"#, "true\nclosed\nfailed\ntrue\ntrue\n");
}

#[test]
fn untyped_collections_diagnose_nullable_scalar_storage() {
    rejects("class Application { main() { int? value = null final values = [value] } }", "untyped List storage");
    rejects("class Application { async int? absent() { return null } main() { final tasks = [this.absent()] } }", "untyped List storage");
}

#[test]
fn sqlite_native_queries_and_errors() {
    runs(r#"class Application { main() {
        final db = SQLite.open(':memory:')
        db.execute('CREATE TABLE items (id INTEGER, name TEXT, price REAL, extra TEXT)')
        print(db.execute('INSERT INTO items VALUES (7, \'ube\', 2.5, NULL)'))
        final rows = db.query('SELECT * FROM items')
        print(rows.next()) print(rows.getLong(0)) print(rows.getString(1))
        print(rows.getDecimal(2)) print(rows.isNull(3)) print(rows.next())
        rows.close()
        try { db.execute('invalid SQL') } catch (Error err) { print('invalid SQL caught') }
        db.close() db.close()
        try { db.query('SELECT 1') } catch (Error err) { print('closed caught') }
    } }"#, "1\ntrue\n7\nube\n2.5\ntrue\nfalse\ninvalid SQL caught\nclosed caught\n");
}

#[test]
fn sqlite_example_and_handle_creation_rules() {
    runs(include_str!("../../examples/sqlite.ube"), "1\nUbe\n");
    rejects("class Application { main() { SQLite.execute('SELECT 1') } }", "require an instance");
    rejects("class Application { main() { final db = SQLite() } }", "Use SQLite.open");
    rejects("class Application { main() { final rows = SQLiteResult() } }", "Use SQLite.open");
}

#[test]
fn sqlite_prepared_bindings_transactions_and_metadata() {
    runs(r#"class Application { main() {
        final db = SQLite.open(':memory:')
        db.busyTimeout(50)
        db.executeBatch('PRAGMA foreign_keys = ON; CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT UNIQUE, price REAL, active INTEGER, payload BLOB, extra TEXT);')
        final insert = db.prepare('INSERT INTO items VALUES (?1, :name, ?3, ?4, ?5, ?6)')
        print(insert.parameterCount()) print(insert.parameterName(2)) print(insert.parameterIndex(':name'))
        insert.bindLong(1, 4294967296L)
        insert.bindString(insert.parameterIndex(':name'), 'hello\' ; DROP TABLE items; --')
        insert.bindDecimal(3, 2.5) insert.bindBool(4, true) insert.bindBlobHex(5, '00ff80') insert.bindNull(6)
        db.begin() print(db.inTransaction()) print(insert.execute()) print(db.changes()) print(db.lastInsertRowId())
        db.commit() print(db.inTransaction())
        final select = db.prepare('SELECT * FROM items WHERE id = ?')
        select.bindLong(1, 4294967296L)
        final rows = select.query()
        print(rows.rowCount()) print(rows.columnCount()) print(rows.columnName(1)) print(rows.columnIndex('NAME'))
        print(rows.next()) print(rows.getLong(0)) print(rows.getString(rows.columnIndex('name')))
        print(rows.getDecimal(2)) print(rows.getBool(3)) print(rows.getBlobHex(4)) print(rows.isNull(5)) print(rows.columnType(4))
        try { print(rows.getInt(0)) } catch (SQLiteError err) { print('overflow') }
        rows.reset() print(rows.next())
        db.begin() db.execute('DELETE FROM items') db.rollback()
        select.clearBindings() final empty = select.query() print(empty.rowCount()) empty.close()
        insert.clearBindings() insert.bindInt(1, 2) insert.bindString(2, 'second') print(insert.execute())
        db.begin() db.execute('DELETE FROM items') db.close()
        print(rows.getLong(0))
        try { select.execute() } catch (SQLiteError err) { print('closed statement') }
        select.close() insert.close() rows.close()
    } }"#, "6\n:name\n2\ntrue\n1\n1\n4294967296\nfalse\n1\n6\nname\n1\ntrue\n4294967296\nhello' ; DROP TABLE items; --\n2.5\ntrue\n00ff80\ntrue\nblob\noverflow\ntrue\n0\n1\n4294967296\nclosed statement\n");
}

#[test]
fn sqlite_returning_nulls_errors_and_parameter_validation() {
    runs(r#"class Application { main() {
        final db = SQLite.open(':memory:')
        db.execute('CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT NOT NULL UNIQUE)')
        final returning = db.prepare('INSERT INTO items(value) VALUES (?) RETURNING id, value')
        returning.bindString(1, 'first') final rows = returning.query()
        print(rows.next()) print(rows.getInt(0)) print(rows.getString(1)) rows.close()
        try { returning.query() } catch (SQLiteError err) { print('unique') }
        try { returning.bindInt(0, 1) } catch (SQLiteError err) { print('index') }
        try { returning.bindBlobHex(1, '0x') } catch (SQLiteError err) { print('hex') }
        try { db.execute('DELETE FROM items; DROP TABLE items') } catch (SQLiteError err) { print('multiple') }
        try { db.query('SELECT 1; SELECT 2') } catch (SQLiteError err) { print('multiple query') }
        final count = db.query('SELECT count(*) FROM items') print(count.next()) print(count.getInt(0)) count.close()
        try { db.execute('SELECT 1') } catch (SQLiteError err) { print('use query') }
        try { db.query('DELETE FROM items') } catch (SQLiteError err) { print('use execute') }
        try { db.busyTimeout(-1) } catch (SQLiteError err) { print('timeout') }
        final empty = db.query('SELECT id AS number FROM items WHERE 0') print(empty.columnName(0)) print(empty.next()) empty.close()
        returning.close() db.close()
    } }"#, "true\n1\nfirst\nunique\nindex\nhex\nmultiple\nmultiple query\ntrue\n1\nuse query\nuse execute\ntimeout\nnumber\nfalse\n");
    rejects("class Application { main() { final db = SQLite.open() } }", "expects 1 arguments");
    rejects("class Application { main() { final db = SQLite.open(':memory:') db.close(1) } }", "expects 0 arguments");
    rejects("class Application { main() { final db = SQLite.open(':memory:') final s = db.prepare('SELECT ?') s.bindLong(1, 'bad') } }", "Expected");
    rejects("class Application { main() { final s = SQLiteStatement() } }", "Use SQLite.open");
}

#[test]
fn sqlite_native_file_backup_restore_and_read_only() {
    let dir = Scratch(std::env::temp_dir().join(format!("ube-sqlite-native-{}", std::process::id())));
    fs::create_dir_all(&dir.0).unwrap();
    let source = r#"class Application { main() {
        print(SQLite.version() != '')
        final db = SQLite.open('$ROOT/source.db')
        print(db.readOnly())
        db.executeBatch('CREATE TABLE t(id INTEGER); INSERT INTO t VALUES (7);')
        db.backup('$ROOT/backup.db')
        db.execute('UPDATE t SET id = 9')
        db.restore('$ROOT/backup.db')
        db.close()
        final ro = SQLite.openReadOnly('$ROOT/source.db')
        print(ro.readOnly())
        final rows = ro.query('SELECT id FROM t') print(rows.next()) print(rows.getInt(0)) rows.close()
        try { ro.execute('DELETE FROM t') } catch (SQLiteError err) { print('read only') }
        ro.close()
    } }"#.replace("$ROOT", dir.0.to_str().unwrap());
    runs(&source, "true\nfalse\ntrue\ntrue\n7\nread only\n");
}
