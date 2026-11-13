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
        "ube-conditional-{}-{}",
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
fn conditional_values_are_lazy_and_right_associative() {
    runs(r#"
    class App {
        static var int calls = 0
        static int next() { static.calls = static.calls + 1 return 7 }
        start() {
            final status = true
              then 'ACTIVE'
              else 'INACTIVE'
            print(status)
            print(false then 1 else true then 2 else 3)
            print(true then false then 1 else 2 else 3)
            print(1 + 2 == 3 and true then 4 else 5)
            print(true then 6 else static.next())
            print(false then static.next() else 8)
            print(true then static.next() else 9)
            print(static.calls)
            string? missing = null
            print(missing then 'yes' else 'no')
            string? present = 'text'
            print(present then 'yes' else 'no')
            final yes = 'active' final no = 'inactive'
            final text = 'status=${true then yes else no}'
            print(text)
        }
    }
    "#, "ACTIVE\n2\n2\n4\n6\n8\n7\n1\nno\nyes\nstatus=active\n");
}

#[test]
fn nullable_and_numeric_branches_keep_their_result_types() {
    runs(r#"
    class App {
        int? maybe(bool flag) { return flag then 12 else null }
        start() {
            final first = true then null else 12
            final second = false then null else 12
            print(first)
            print(second)
            print(this.maybe(true))
            print(this.maybe(false))
            int? number = 4
            print(true then number else 3.5)
            int? absent = null
            print(true then absent else 3.5)
            print(true then 2 else 3.5)
            print(false then 2 else 3.5)
            string? name = null
            print((true then name else 'Guest') ?? 'fallback')
            final offset = 10
            final operation = (int value) -> value > 0 then value + offset else offset
            print(operation(2))
            print(operation(0))
        }
    }
    "#, "null\n12\n12\nnull\n4\nnull\n2\n3.5\nfallback\n12\n10\n");
}

#[test]
fn malformed_or_incompatible_branches_are_rejected() {
    for expression in ["if true then 1 else 2", "if (true) then 1 else 2", "true then 1", "true then else 2", "true then 1 else", "true ? 1 : 2"] {
        let source = format!("class App {{ start() {{ final value = {expression} }} }}");
        assert!(!diagnostics(&source).is_empty(), "{expression}");
    }
    rejects("class App { start() { final value = true then 1 else 'text' } }", "Conditional branches require compatible");
    rejects("class App { start() { final value = true then print(1) else print(2) } }", "Conditional branches require compatible");
    rejects("class App { start() { int value = true then 1 else null } }", "Mismatched initializer type");
}

#[test]
fn ternaries_preserve_braced_if_and_explicit_grouping() {
    runs(r#"
    class App {
        int choose(bool flag) {
            if (flag) { return 10 } else { return 20 }
        }
        start() {
            final value = if (true) { yield 3 } else { yield 4 }
            print(value)
            print(this.choose(true))
            print((false) then 1 else 2)
            print((true) and true then 5 else 6)
            print((true then false else true) then 7 else 8)
            print((true then 2 else 3) + 4)
        }
    }
    "#, "3\n10\n2\n5\n8\n6\n");
}

#[test]
fn else_fallback_is_lazy_and_composes_with_conditionals() {
    runs(r#"
    class App {
        int fail() { throw 'fallback evaluated' }
        start() {
            int? missing = null
            int? zero = 0
            bool? no = false
            print(missing else -1)
            print(zero else this.fail())
            print(no else true)
            print(missing else missing else 7)
            print(true then (missing else 3) else 4)
            print(false then 3 else (missing else 4))
            print((true then missing else 3) else 5)
            print(missing else false then 6 else 7)
            print(true then false then 1 else 2 else 3)
        }
    }
    "#, "-1\n0\nfalse\n7\n3\n4\n5\n7\n2\n");
    rejects("class App { start() { int? n = null final x = n else 'text' } }", "Incompatible fallback");
}
