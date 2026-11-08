mod support;
use std::process::{Command, Output};
use ube_compiler::{Lexer, Parser, TypeChecker};

fn execute(source: &str, command: &str) -> Output {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("mock_test.ube");
    let file = support::write_source(&file, source);
    Command::new("timeout")
        .arg("90s")
        .arg(env!("CARGO_BIN_EXE_ube-compiler"))
        .arg(command)
        .arg(file)
        .output()
        .unwrap()
}
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn generated_mocks_inject_into_structural_traits_and_verify_native_calls() {
    let stdout = success(execute(
        r#"
        trait Greeting { string greet(string name) int count() bool ready() decimal ratio() long large() notify() }
        class Welcome(Greeting greeting) {
            string render() { this._greeting.notify() return this._greeting.greet('Ada') }
        }
        class App {
            start() {
                final greeting = Mockery.mock(Greeting)
                Mockery.verifyNoCalls(greeting)
                Mockery.returns(greeting, 'greet', 'Hello Ada')
                Mockery.returns(greeting, 'count', 0)
                Mockery.returns(greeting, 'ready', false)
                Mockery.returns(greeting, 'ratio', 0.25)
                Mockery.returns(greeting, 'large', 9007199254740993L)
                Mockery.allow(greeting, 'notify')
                Testicle.equal('Hello Ada', Welcome(greeting).render())
                Testicle.equal(0, greeting.count())
                Testicle.isFalse(greeting.ready())
                Testicle.equal(0.25, greeting.ratio())
                Testicle.equal(9007199254740993L, greeting.large())
                Mockery.verify(greeting, 'greet', 1)
                Mockery.verify(greeting, 'notify', 1)
                Testicle.equal(1L, Mockery.calls(greeting, 'greet'))
                Mockery.clearCalls(greeting)
                Mockery.verifyNoCalls(greeting)
                Testicle.equal('Hello Ada', greeting.greet('another argument'))
                print('mock passed')
            }
        }
    "#,
        "run",
    ));
    assert_eq!(stdout, "mock passed\n");
}

#[test]
fn strict_failures_counts_reset_disposal_and_handles_are_recoverable() {
    let stdout = success(execute(
        r#"
        trait Service { string load(int id) }
        class App {
            start() {
                final mock = Mockery.mock(Service)
                try { mock.load(1) } catch (MockeryError error) { print('unstubbed') }
                Mockery.verify(mock, 'load', 1)
                try { Mockery.verify(mock, 'load', 0) } catch (AssertionError error) { print('count mismatch') }
                try { Mockery.verifyNoCalls(mock) } catch (AssertionError error) { print('unexpected calls') }
                try { Mockery.verify(mock, 'load', -1) } catch (MockeryError error) { print('negative count') }
                Mockery.throws(mock, 'load', 'offline')
                try { mock.load(2) } catch (MockeryError error) { Testicle.equal('offline', error.message()) }
                Mockery.verify(mock, 'load', 2)
                Mockery.returns(mock, 'load', 'old')
                final saved = mock.load(3)
                Mockery.returns(mock, 'load', 'new')
                Testicle.equal('new', mock.load(4))
                Testicle.equal('old', saved)
                Mockery.reset(mock)
                Mockery.verifyNoCalls(mock)
                try { mock.load(5) } catch (MockeryError error) { print('reset strict') }
                Mockery.dispose(mock)
                Testicle.equal('old', saved)
                final other = Mockery.mock(Service)
                Mockery.returns(other, 'load', 'other')
                try { mock.load(6) } catch (MockeryError error) { print('disposed') }
                try { Mockery.dispose(mock) } catch (MockeryError error) { print('double dispose') }
                Testicle.equal('other', other.load(1))
            }
        }
    "#,
        "run",
    ));
    assert_eq!(stdout, "unstubbed\ncount mismatch\nunexpected calls\nnegative count\nreset strict\ndisposed\ndouble dispose\n");
}

#[test]
fn mocks_work_with_native_test_runner_async_consumers_and_parallel_calls() {
    let stdout = success(execute(
        r#"
        trait Worker { int value(int seed) }
        class Consumer(Worker worker) {
            async int read() { return this._worker.value(1) }
        }
        class MockTests {
            async testWorker() {
                final mock = Mockery.mock(Worker)
                Mockery.returns(mock, 'value', 42)
                final first = Consumer(mock).read()
                final second = Consumer(mock).read()
                Testicle.equal(42, await first)
                Testicle.equal(42, await second)
                Mockery.verify(mock, 'value', 2)
                Mockery.throws(mock, 'value', 'async failure')
                var caught = false
                try { final value = await Consumer(mock).read() }
                catch (MockeryError error) { caught = true }
                Testicle.isTrue(caught)
                Mockery.verify(mock, 'value', 3)
            }
        }
    "#,
        "test",
    ));
    assert!(stdout.contains("Tests: 1 passed, 0 failed"));
}

#[test]
fn invalid_mock_contracts_stubs_names_and_internal_access_are_rejected() {
    for body in [
        "final mock = Mockery.mock(Missing)",
        "final mock = Mockery.mock(Service) Mockery.returns(mock, 'load', 1)",
        "final mock = Mockery.mock(Service) Mockery.returns(mock, 'typo', 'text')",
        "final mock = Mockery.mock(Service) final method = 'load' Mockery.returns(mock, method, 'text')",
        "final mock = Mockery.mock(Service) Mockery.allow(mock, 'load')",
        "final mock = Mockery.mock(Service) Mockery.verify(mock, 'load', 'one')",
        "Mockery._call_int(1L, 'load')",
        "final mock = Mockery()",
        "final mock = UbeMockery_Service(1L)",
        "final mock = Mockery.mock(Service) Mockery.reset(mock, 1)",
    ] {
        let source = format!("trait Service {{ string load(int id) }} class App {{ start() {{ {body} }} }}");
        let mut parser = Parser::new(Lexer::new(&source));
        let program = parser.parse_program();
        assert!(parser.diagnostics.is_empty(), "{body}: {:?}", parser.diagnostics);
        let mut checker = TypeChecker::new();
        assert!(!checker.check_program(&program), "accepted {body}");
    }
    for source in [
        "trait Unsupported { Service load() } class Service class App { start() { final mock = Mockery.mock(Unsupported) } }",
        "trait Nullable { string? value() } class App { start() { final mock = Mockery.mock(Nullable) } }",
        "class UbeMockery_Custom",
        "trait UbeMockery_Custom { long attack() { return Mockery._call_long(1L, 'load') } }",
        "trait Collision { long _mockeryId() } class App { start() { final mock = Mockery.mock(Collision) } }",
        "trait Async { async int value() } class App { start() { final mock = Mockery.mock(Async) } }",
    ] {
        let mut parser = Parser::new(Lexer::new(source));
        let program = parser.parse_program();
        assert!(parser.diagnostics.is_empty(), "{source}: {:?}", parser.diagnostics);
        let mut checker = TypeChecker::new();
        assert!(!checker.check_program(&program), "accepted {source}");
    }
}
