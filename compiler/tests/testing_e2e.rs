use std::{
    fs,
    process::{Command, Output},
};

fn run(files: &[(&str, &str)], args: &[&str]) -> Output {
    let directory = tempfile::tempdir().unwrap();
    for (name, source) in files {
        let file = directory.path().join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, source).unwrap();
    }
    Command::new("timeout")
        .arg("90s")
        .arg(env!("CARGO_BIN_EXE_ube-compiler"))
        .arg("test")
        .args(args)
        .current_dir(directory.path())
        .output()
        .unwrap()
}

#[test]
fn discovers_native_tests_and_supports_assertions_async_and_hooks() {
    let output = run(
        &[
            (
                "tests/nested/math_test.ube",
                r#"
            class MathTests {
                beforeEach() { print('setup') }
                afterEach() { print('teardown') }
                testValues() {
                    Testicle.equal(2, 1 + 1)
                    Testicle.equal('abcd', 'abcd')
                    Testicle.equal(false, false)
                    Testicle.equal(1.5, 1.5)
                    Testicle.isTrue(true)
                    Testicle.isFalse(false)
                    Testicle.notEqual(1, 2)
                }
                async testAsync() { Testicle.equal(3, 3) }
            }
        "#,
            ),
            ("tests/ignored.ube", "not Ube source"),
        ],
        &[],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("Tests: 2 passed, 0 failed"));
    assert_eq!(stdout.matches("setup\n").count(), 2);
    assert_eq!(stdout.matches("teardown\n").count(), 2);
}

#[test]
fn failures_are_typed_run_teardown_and_do_not_stop_other_tests() {
    let output = run(
        &[(
            "sample.ube",
            r#"
        class Checks {
            afterEach() { print('cleanup') }
            testFailure() { Testicle.equal(1, 2) print('unreachable') }
            testRecovery() {
                try { Testicle.fail('expected failure') }
                catch (AssertionError error) { Testicle.equal('expected failure', error.message()) }
                Testicle.isTrue(true)
            }
        }
    "#,
        )],
        &["sample.ube"],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
    assert!(
        stdout.contains("Tests: 1 passed, 1 failed"),
        "{stdout}\n{stderr}"
    );
    assert!(!stdout.contains("unreachable"));
    assert_eq!(stdout.matches("cleanup\n").count(), 2);
    assert!(stderr.contains("AssertionError"), "{stderr}");
}

#[test]
fn timeouts_kill_the_test_and_allow_subsequent_tests_to_run() {
    let output = run(
        &[(
            "slow.test.ube",
            r#"
        class Slow {
            testTimeout() { for (true) {} }
            testAfter() { Testicle.isTrue(true) }
        }
    "#,
        )],
        &[".", "--timeout", "100"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("timed out"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Tests: 1 passed, 1 failed"));
}

#[test]
fn invalid_sources_signatures_empty_suites_and_options_fail() {
    for (source, expected) in [
        ("class Empty {}", "No tests found"),
        (
            "class Invalid { testArgs(int n) {} }",
            "require no parameters",
        ),
        (
            "class Invalid { main() {} testOne() {} }",
            "must not define main",
        ),
        (
            "class Invalid { testOne() { Testicle.equal(1, 'one') } }",
            "Testicle requires a static call",
        ),
        (
            "class Invalid { testOne() { Testicle.isTrue(null) } }",
            "Testicle requires a static call",
        ),
        (
            "class Invalid { testOne() { final a = Testicle() } }",
            "Testicle does not create instances",
        ),
    ] {
        let output = run(&[("bad.ube", source)], &["bad.ube"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = run(&[], &["--timeout", "0"]);
    assert_eq!(output.status.code(), Some(1));
}
