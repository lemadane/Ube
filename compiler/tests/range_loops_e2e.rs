//! Exercise loop syntax through the real CLI, native compiler, and executable.
use std::{fs, process::Command};

fn runs(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let entry = directory.path().join("App.ube");
    fs::write(&entry, source).unwrap();
    let output = Command::new("timeout")
        .args(["45s", env!("CARGO_BIN_EXE_ube-compiler"), "run"])
        .arg(&entry).output().unwrap();
    assert!(output.status.success(), "status {:?}\nstdout: {}\nstderr: {}",
        output.status.code(), String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn each_in_and_for_by_execute_through_cli() {
    runs(r#"class App { start() {
        final items = [10, 20]
        each (index, item in items) { print(`${index}: ${item}`) }
        final doubled = each (item in items) { yield item * 2 }
        each (item in doubled) { print(item) }
        for (i = 0 to 10, by 2) { print(i) }
        final values = for (i = 2 to 4) { yield i }
        each (value in values) { print(value) }
    } }"#, "0: 10\n1: 20\n20\n40\n0\n2\n4\n6\n8\n10\n2\n3\n4\n");
}

#[test]
fn inclusive_bounds_descending_steps_continue_and_else() {
    runs(r#"class App { start() {
        for (i = 10 to 0, by -3) { print(i) }
        for (i = 0 to 6, by 3) { print(i) }
        for (i = 4 to 4, by -2) { print(i) }
        for (i = 5 to 1) { print('bad') } else { print('ascending empty') }
        for (i = 1 to 5, by -1) { print('bad') } else { print('descending empty') }
        for (i = 0 to 4) {
            if (i == 1) { continue }
            if (i == 3) { break }
            print(i)
        } else { print('bad') }
        for (i = 0 to 0) { continue } else { print('bad') }
    } }"#, "10\n7\n4\n1\n0\n3\n6\n4\nascending empty\ndescending empty\n0\n2\n");
}

#[test]
fn bounds_are_evaluated_once_and_loop_variables_are_scoped() {
    runs(r#"class App {
        int bound(string label, int value) { print(label) return value }
        start() {
            var limit = 4
            var step = 2
            final i = 99
            for (i = this.bound('start', 0) to this.bound('end', limit), by this.bound('step', step)) {
                limit = 0
                step = 0
                print(i)
            }
            print(i)
            final begin = 1
            final end = 3
            final render = () -> { return for (j = begin to end) { yield j * 10 } }
            each (value in render()) { print(value) }
            final task = Async.start(() -> { return for (j = begin to end, by 2) { yield j } })
            final values = task.await()
            each (value in values) { print(value) }
        }
    }"#, "start\nend\nstep\n0\n2\n4\n99\n10\n20\n30\n1\n3\n");
}

#[test]
fn range_boundaries_do_not_wrap_or_loop_forever() {
    runs(r#"class App { start() {
        for (i = 2147483646 to 2147483647, by 3) { print(i) }
        for (i = 2147483647 to 2147483647) { print(i) }
        final minimum = -2147483647 - 1
        for (i = minimum to minimum, by -1) { print(i) }
        for (i = minimum + 1 to minimum, by -3) { print(i) }
    } }"#, "2147483646\n2147483647\n-2147483648\n-2147483647\n");
}

#[test]
fn dynamic_zero_step_is_catchable_and_cleanup_survives_loop_exits() {
    runs(r#"class App { int zero() { return 0 } start() {
        try {
            for (i = 0 to 2, by this.zero()) { print('bad') } else { print('bad else') }
        } catch (Error err) { print(err.message()) } finally { print('zero cleanup') }
        for (i = 1 to 2) {
            try { if (i == 1) { continue } break }
            finally { print(`cleanup ${i}`) }
        }
        for (i = 0 to 1) {
            for (j = 0 to 1) { print(`${i}:${j}`) break }
        }
    } }"#, "Range step must not be zero\nzero cleanup\ncleanup 1\ncleanup 2\n0:0\n1:0\n");
}

#[test]
fn invalid_range_headers_types_and_old_each_syntax_are_rejected() {
    for (body, expected) in [
        ("from (i = 0 to 3) {}", "Expected"),
        ("final values = from (i = 0 to 3) { yield i }", "Expected"),
        ("each (item from [1]) { print(item) }", "Expected in"),
        ("for (i = 0 to 3, by 0) {}", "Range step must not be zero"),
        ("for (i = 0 to 3, by 'x') {}", "Range step must be int"),
        ("for (i = 0L to 3) {}", "Range start must be int"),
        ("for (i = 0 to 3.5) {}", "Range end must be int"),
        ("for (i = 0 to 3, by true) {}", "Range step must be int"),
        ("for (i = 0 until 3) {}", "Expected 'to'"),
        ("for (i = 0 to 3, 2) {}", "Expected 'by'"),
        ("for (i = 0 to 3) { i = 10 }", "immutable"),
        ("for (i = 0 to 3) {} print(i)", "Undeclared identifier"),
    ] {
        let source = format!("class App {{ start() {{ {body} }} }}");
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("App.ube");
        fs::write(&entry, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
            .arg("check").arg(&entry).output().unwrap();
        assert!(!output.status.success(), "Unexpectedly accepted: {body}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "Expected {expected} for {body}, got {stderr}");
    }
}
