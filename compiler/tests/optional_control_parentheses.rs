use std::{fs, process::Command};
use ube_compiler::{Lexer, Parser};

fn runs(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let entry = directory.path().join("App.ube");
    fs::write(&entry, source).unwrap();
    let output = Command::new("timeout")
        .args(["45s", env!("CARGO_BIN_EXE_ube-compiler"), "run"])
        .arg(&entry)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "status {:?}\n{}\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn conditions_support_grouping_calls_and_expression_results() {
    runs(
        r#"class App {
        bool enabled() { return true }
        start() {
            if false { print('bad') }
            else if (false or this.enabled()) and not false { print('branch') }
            else { print('bad') }
            final result = if this.enabled() { yield 10 } else { yield 20 }
            print(result)
            final other = if false { yield 1 } else if true { yield 2 } else { yield 3 }
            print(other)
            if (true) { print('legacy') }
            var count = 0
            while (count < 2) and this.enabled() {
                print(count)
                count = count + 1
            }
            while false {} else { print('empty') }
        }
    }"#,
        "branch\n10\n2\nlegacy\n0\n1\nempty\n",
    );
}

#[test]
fn loop_statements_and_comprehensions_support_bare_headers() {
    runs(
        r#"class App { start() {
        final items = for i = (1 + 1) to 4, by (1 + 1) { yield i }
        each index, item in items { print('${index}: ${item}') }
        final doubled = each item in items { yield item * 2 }
        each item in doubled { print(item) }
        for i = 2 to 0, by -1 {
            if i == 1 { continue }
            print(i)
        }
        for i = 2 to 1 {} else { print('range empty') }
        each item in [] {} else { print('each empty') }
        var count = 0
        final values = while count < 2 {
            count = count + 1
            yield count
        }
        each value in values { print(value) }
        while { print('infinite') break }
        for (i = 0 to 0) { print(i) }
        each (item in [9]) { print(item) }
        while (false) {} else { print('legacy empty') }
    } }"#,
        "0: 2\n1: 4\n4\n8\n2\n0\nrange empty\neach empty\n1\n2\ninfinite\n0\n9\nlegacy empty\n",
    );
}

#[test]
fn bare_catches_execute_and_preserve_finally() {
    runs(
        r#"class App { start() {
        try { throw 'bare' }
        catch Error err { print(err.message()) }
        finally { print('cleanup') }
        try { throw 'legacy' }
        catch (Error err) { print(err.message()) }
        try { print('try finally') } finally { print('done') }
    } }"#,
        "bare\ncleanup\nlegacy\ntry finally\ndone\n",
    );
}

#[test]
fn malformed_headers_still_require_balanced_groups_and_braced_bodies() {
    for body in [
        "if true print('bad')",
        "if (true {}",
        "if true) {}",
        "while true print('bad')",
        "while (true {}",
        "for i = 0 to 1) {}",
        "for (i = 0 to 1 {}",
        "for i = 0 to 1, by {}",
        "each item in [1]) {}",
        "each (item in [1] {}",
        "try {} catch Error {}",
        "try {} catch (Error err {}",
        "try {} catch Error err) {}",
        "try {} catch Error err print('bad')",
    ] {
        let source = format!("class App {{ start() {{ {body} }} }}");
        let mut parser = Parser::new(Lexer::new(&source));
        parser.parse_program();
        assert!(
            !parser.diagnostics.is_empty(),
            "Unexpectedly accepted: {body}"
        );
    }
}
