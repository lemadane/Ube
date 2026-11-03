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
    class Application { main() { try { throw A("bad", 42) }
      catch (B err) { print("wrong") } catch (A err) { print(err.message()) print(err.code()) }
      catch (Error err) { print("wrong general") } } }"#, "bad\n42\n");
}
#[test]
fn strings_are_boxed_once_and_error_trait_dispatches() {
    runs(r#"class Application { string message() { print("called") return "boom" }
    main() { try { throw this.message() } catch (Error err) { print(err.message()) } } }"#, "called\nboom\n");
}
#[test]
fn structural_category_matches_independent_classes() {
    runs(r#"trait Retryable { string message() bool retryable() }
    class A(string message, bool retryable) {} class B(string message) { bool retryable() { return true } }
    class Application { main() {
      try { throw A("a", false) } catch (Retryable err) { print(err.message()) print(err.retryable()) }
      try { throw B("b") } catch (Retryable err) { print(err.message()) print(err.retryable()) }
    } }"#, "a\nfalse\nb\ntrue\n");
}
#[test]
fn independent_traits_match_in_source_order() {
    runs(r#"trait Retryable { string message() bool retryable() }
    trait Security { string message() string securityCode() }
    class Both(string message, bool retryable, string securityCode) {}
    class Application { main() { try { throw Both("both", true, "AUTH") }
      catch (Security err) { print(err.securityCode()) }
      catch (Retryable err) { print("wrong") } } }"#, "AUTH\n");
}
#[test]
fn unmatched_errors_propagate_through_calls_and_cleanup_only_try() {
    runs(r#"class A(string message) {} class B(string message) {}
    class Application { fail() { throw A("boom") } relay() { this.fail() }
    main() { try { try { this.relay() } catch (B err) { print("wrong") } finally { print("inner") } }
      catch (A err) { print(err.message()) } finally { print("outer") } } }"#, "inner\nboom\nouter\n");
}
#[test]
fn try_finally_does_not_swallow_errors() {
    runs(r#"class Application { main() { try { try { throw "boom" } finally { print("cleanup") } }
      catch (Error err) { print(err.message()) } } }"#, "cleanup\nboom\n");
}
#[test]
fn accidental_conformance_requires_explicit_throw() {
    runs(r#"class Customer(string message) {} class Application { main() {
      final customer = Customer("hello") print("normal")
      try { throw customer } catch (Customer err) { print(err.message()) } } }"#, "normal\nhello\n");
}
#[test]
fn finally_runs_on_normal_completion_and_matching_catch() {
    runs(r#"class Application { main() {
      try { print("normal") } finally { print("first") }
      try { throw "boom" } catch (Error err) { print("handled") } finally { print("second") }
    } }"#, "normal\nfirst\nhandled\nsecond\n");
}
#[test]
fn nested_finally_runs_once_for_return_and_catch_return() {
    runs(r#"class Application {
      int value() { try { try { return 42 } finally { print("inner") } } finally { print("outer") } }
      int handled() { try { throw "boom" } catch (Error err) { return 7 } finally { print("catch cleanup") } }
      main() { print(this.value()) print(this.handled()) }
    }"#, "inner\nouter\n42\ncatch cleanup\n7\n");
}
#[test]
fn loop_exits_cleanup_only_regions_being_exited() {
    runs(r#"class Application { main() {
      try { for (x in [1,2]) { try { if (x == 1) { continue } break } finally { print(x) } } print("done") }
      finally { print("outer") }
    } }"#, "1\n2\ndone\nouter\n");
}
#[test]
fn exception_from_handler_runs_finally_before_outer_handler() {
    runs(r#"class Application { main() { try {
      try { throw "first" } catch (Error err) { throw "second" } finally { print("inner") }
    } catch (Error err) { print(err.message()) } } }"#, "inner\nsecond\n");
}
#[test]
fn cleanup_error_replaces_pending_error_and_preserves_it_as_suppressed() {
    let output = native(r#"class Application { main() { try { throw "original" } finally { throw "cleanup" } } }"#);
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
        final root = Root("root")
        try { throw PaymentError(Details("payment", "PAY"), root) }
        catch (PaymentError err) {
          print(err.message()) print(err.code())
          try { throw (err.cause() ?? GenericError("missing")) } catch (Root cause) { print(cause.message()) }
        }
      } }"#, "payment\nPAY\nroot\n");
}
#[test]
fn guarded_catches_narrow_before_condition_and_continue_on_false() {
    runs(r#"trait Retryable { string message() bool retryable() }
      class A(string message, bool retryable) {}
      class Application { main() { try { throw A("boom", false) }
        catch (Retryable err) if (err.retryable()) { print("wrong") }
        catch (A err) { print(err.message()) }
      } }"#, "boom\n");
}
#[test]
fn throwing_catch_condition_propagates_after_cleanup() {
    runs(r#"class A(string message) { bool test() { throw "condition" } }
      class Application { main() { try { try { throw A("original") }
        catch (A err) if (err.test()) { print("wrong") } finally { print("cleanup") }
      } catch (Error err) { print(err.message()) } } }"#, "cleanup\ncondition\n");
}
#[test]
fn async_failures_keep_type_fields_causes_and_finally_order() {
    runs(r#"class DbError(string message, int code) {}
      class Application { async int fail() { try { throw DbError("db", 99) } finally { print("worker cleanup") } }
        async int relay() { return await this.fail() }
        main() { try { final value = await this.relay() } catch (DbError err) { print(err.message()) print(err.code()) } finally { print("awaiter cleanup") } }
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
        finally { print("worker cleanup") }
      }
      main() { final ready = Channel<int>(1) final blocked = Channel<int>(1)
        final task = this.wait(ready, blocked) final started = await ready.receive()
        final cancelled = task.cancel()
        try { final value = await task } catch (Error err) { print(err.message()) }
      }
    }"#, "worker cleanup\ntask was cancelled\n");
}
#[test]
fn unhandled_sync_and_async_errors_exit_unsuccessfully() {
    for source in [r#"class Application { main() { throw "sync" } }"#,
      r#"class Application { async fail() { throw "async" } main() { final value = await this.fail() } }"#] {
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
    rejects(r#"class A(string message) {} class Application { main() { try { throw A("x") } catch (Error err) {} catch (A err) {} } }"#, "Unreachable catch");
    rejects(r#"trait R { string message() bool retryable() } class A(string message, bool retryable) {}
      class Application { main() { try { throw A("x", true) } catch (R err) {} catch (A err) {} } }"#, "Unreachable catch");
}
#[test]
fn catch_scopes_and_static_types_are_checked() {
    rejects(r#"class A(string message, int code) {} class Application { main() { try { throw A("x",1) } catch (A err) { print(err.missing()) } } }"#, "Member");
    rejects(r#"class Application { main() { try { throw "x" } catch (Error err) {} print(err) } }"#, "err");
    rejects(r#"class Application { main() { try { final local = 1 } finally { print(local) } } }"#, "local");
    rejects(r#"class Application { main() { try { throw "x" } catch (Error err) if (1) {} } }"#, "catch condition");
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
      try {{ final value = File.read("{path}") print("wrong") }}
      catch (FileNotFoundError err) {{ print("read") }} finally {{ print("cleanup") }}
      try {{ File.write("{path}", "data") print("wrong") }} catch (Error err) {{ print("write") }}
      try {{ File.append("{path}", "data") }} catch (Error err) {{ print("append") }}
      try {{ File.delete("{path}") }} catch (FileNotFoundError err) {{ print("delete") }}
      try {{ final info = File.info("{path}") }} catch (FileNotFoundError err) {{ print("info") }}
      try {{ final entries = Directory.list("{path}") }} catch (FileNotFoundError err) {{ print("list") }}
      try {{ Directory.delete("{path}") }} catch (FileNotFoundError err) {{ print("directory delete") }}
      print(File.exists("{path}"))
    }} }}"#), "read\ncleanup\nwrite\nappend\ndelete\ninfo\nlist\ndirectory delete\nfalse\n");
}

#[test]
fn runtime_file_handles_distinguish_empty_content_closed_handles_and_binary_data() {
    let path = std::env::temp_dir().join(format!("ube-file-handles-{}", std::process::id()));
    let path = path.to_string_lossy();
    runs(&format!(r#"class Application {{ main() {{
      File.write("{path}", "")
      print(File.read("{path}"))
      File.write("{path}", "hello")
      final file = File.open("{path}")
      print(file.read())
      file.close()
      try {{ print(file.read()) }} catch (Error err) {{ print("closed") }}
      File.delete("{path}")
    }} }}"#), "\nhello\nclosed\n");
    let binary = std::env::temp_dir().join(format!("ube-binary-{}", std::process::id()));
    fs::write(&binary, b"hello\0world").unwrap();
    runs(&format!(r#"class Application {{ main() {{
      try {{ print(File.read("{}")) }} catch (Error err) {{ print("binary rejected") }}
    }} }}"#, binary.display()), "binary rejected\n");
    fs::remove_file(binary).unwrap();
}

#[test]
fn runtime_server_startup_failures_are_catchable() {
    let reserved = std::net::TcpListener::bind(("127.0.0.1", 0)).ok();
    let port = reserved.as_ref().map(|listener| listener.local_addr().unwrap().port()).unwrap_or(8080);
    runs(&format!(r#"class Application {{ main() {{
      final app = server()
      try {{ app.start(-1) print("wrong") }} catch (Error err) {{ print("invalid port") }}
      try {{ app.start({port}) print("wrong") }} catch (Error err) {{ print("bind failed") }}
      finally {{ print("cleanup") }}
      print("recovered")
    }} }}"#), "invalid port\nbind failed\ncleanup\nrecovered\n");
}

#[test]
fn unhandled_runtime_io_errors_exit_unsuccessfully() {
    let output = native(r#"class Application { main() { File.read("/ube-nonexistent-directory/file") print("wrong") } }"#);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("FileNotFoundError"));
}
