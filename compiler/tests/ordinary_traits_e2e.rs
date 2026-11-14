//! Ordinary traits admit only compatible classes from the compiled application.
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
fn foreign_objects_cannot_enter_application_traits() {
    for body in [
        "start() { Message value = GenericError('foreign') }",
        "start() { Message? value = GenericError('foreign') }",
        "Message get() { return GenericError('foreign') } start() {}",
        "Message get() { yield GenericError('foreign') } start() {}",
        "accept(Message value) {} start() { this.accept(GenericError('foreign')) }",
        "start() { Error erased = GenericError('foreign') Message value = erased }",
        "start() { Message value = print('foreign') }",
        "start() { Local value = print('foreign') Message result = value }",
        "start() { Message value = if (true) { yield Local('local') } else { yield GenericError('foreign') } }",
        "start() { Message value = Local('local') value = GenericError('foreign') }",
        "start() { final callback = (Message value) -> value.message() callback(GenericError('foreign')) }",
    ] {
        let source = format!("trait Message {{ string message() }} class Local(string message) class App {{ {body} }}");
        let output = execute(&source, "check");
        assert!(!output.status.success(), "accepted foreign value: {source}");
        let diagnostic = String::from_utf8_lossy(&output.stderr).to_string()
            + &String::from_utf8_lossy(&output.stdout);
        assert!(diagnostic.contains("type") || diagnostic.contains("compatible")
            || diagnostic.contains("argument"), "{diagnostic}");
    }
}

#[test]
fn matching_json_methods_and_erased_foreign_permits_do_not_confer_membership() {
    for source in [
        "trait Reader { string getString(string key) } class App { start() { Reader value = Json.toObject('{}') } }",
        "trait Message { string message() } trait Foreign permit GenericError { string message() } class App { start() { Foreign value = GenericError('foreign') Message erased = value } }",
        "trait Message { string message() } class Holder(Message value) class App { start() { Holder(GenericError('foreign')) } }",
    ] {
        let output = execute(source, "check");
        assert!(!output.status.success(), "accepted foreign value: {source}");
        let diagnostic = String::from_utf8_lossy(&output.stderr).to_string()
            + &String::from_utf8_lossy(&output.stdout);
        assert!(diagnostic.contains("Mismatched initializer type")
            || diagnostic.contains("for argument"), "{diagnostic}");
    }
}

#[test]
fn imported_application_classes_and_defaults_remain_structural() {
    let output = execute(r#"
        trait Message { string message() }
        trait OtherMessage { string message() }
        trait Greeting { string greet() { return 'hello' } }
        class Local(string message)
        class App {
            Message get() { yield Local('local') }
            OtherMessage widen(Message value) { return value }
            start() {
                print(this.widen(this.get()).message())
                Greeting greeting = Local('local')
                print(greeting.greet())
            }
        }
    "#, "run");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "local\nhello\n");
}

#[test]
fn ordinary_catches_exclude_foreign_errors_but_error_still_catches_them() {
    let output = execute(r#"
        trait Message { string message() }
        class Local(string message)
        class App {
            start() {
                try { throw Local('local') }
                catch (Message value) { print(value.message()) }
                try { throw GenericError('foreign') }
                catch (Message value) { print('wrong handler') }
                catch (Error value) { print(value.message()) }
            }
        }
    "#, "run");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "local\nforeign\n");
}
