//! Explicit construction uses the same checked constructor path as Type(...).
mod support;
use std::process::Command;

fn execute(source: &str, command: &str) -> std::process::Output {
    let directory = tempfile::tempdir().unwrap();
    let entry = support::write_source(&directory.path().join("App.ube"), source);
    Command::new("timeout")
        .args(["60s", env!("CARGO_BIN_EXE_ube-compiler"), command])
        .arg(entry).output().unwrap()
}

#[test]
fn explicit_construction_supports_defaults_nesting_chaining_generics_and_traits() {
    let output = execute(r#"
        trait Named { string name() }
        trait Allowed permit Person { string name() }
        class Person(string name = 'default')
        class Holder(Person person)
        class App {
            start() {
                final explicit = new Person('Ada')
                final implicit = Person('Ada')
                print(explicit.name() == implicit.name())
                print(new Person().name())
                print(new Holder(new Person('nested')).person().name())
                Named named = new Person('ordinary')
                Allowed allowed = new Person('restricted')
                print(named.name())
                print(allowed.name())
                final channel = new Channel<int>(1)
                channel.send(7)
                print(channel.receive().await())
                channel.close()
                final value = 'captured'
                final create = () -> new Person(value)
                print(create().name())
                try { throw new GenericError('error') }
                catch (Error error) { print(error.message()) }
            }
        }
    "#, "run");
    assert!(output.status.success(), "{}\n{}",
        String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout),
        "true\ndefault\nnested\nordinary\nrestricted\n7\ncaptured\nerror\n");
}

#[test]
fn new_requires_a_constructible_class_and_preserves_constructor_checks() {
    for (source, expected) in [
        ("class App { start() { final value = new } }", "Expected class name after 'new'"),
        ("class Person class App { start() { new Person } }", "Expected ("),
        ("class App { start() { new print('hello') } }", "requires a class constructor"),
        ("class App { start() { new log('hello') } }", "requires a class constructor"),
        ("class App { start() { new int(1) } }", "Expected class name after 'new'"),
        ("class App { start() { new Missing() } }", "Unknown class 'Missing'"),
        ("class App { start() { new channel<int>() } }", "Built-in classes require capitalization"),
        ("class Person(int age) class App { start() { new Person('wrong') } }", "for argument 1"),
        ("class Person class App { start() { new Person(1) } }", "constructor expected at most"),
        ("trait Named { string name() } class App { start() { new Named() } }", "cannot be constructed"),
        ("trait Named { string name() } class App { start() { Named() } }", "cannot be constructed"),
        ("class App { start() { new JsonObject() } }", "Use Json.toObject"),
        ("class App { start() { new SQL() } }", "SQL handles cannot be constructed"),
        ("class App { start() { new SQL.open('sqlite::memory:') } }", "Expected ("),
        ("class App { start() { final callback = () -> 1 new callback() } }", "Unknown class 'callback'"),
    ] {
        let output = execute(source, "check");
        let diagnostic = String::from_utf8_lossy(&output.stderr).to_string()
            + &String::from_utf8_lossy(&output.stdout);
        assert!(!output.status.success(), "accepted: {source}");
        assert!(diagnostic.contains(expected), "expected {expected:?}: {diagnostic}");
    }
}

#[test]
fn formatting_preserves_explicit_construction() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("App.ube");
    std::fs::write(&file, "class App { start() { final value = new Person('Ada') } }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
        .args(["format", "-w"]).arg(&file).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(std::fs::read_to_string(file).unwrap().contains("new Person('Ada')"));
}
