//! Restricted traits through physical source loading, checking, LLVM, and native execution.
mod support;
use std::process::Command;

fn execute(source: &str, command: &str) -> std::process::Output {
    let directory = tempfile::tempdir().unwrap();
    let entry = support::write_source(&directory.path().join("App.ube"), source);
    Command::new("timeout")
        .args(["60s", env!("CARGO_BIN_EXE_ube-compiler"), command])
        .arg(entry)
        .output()
        .unwrap()
}

fn runs(source: &str, expected: &str) {
    let result = execute(source, "run");
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), expected);
}

fn rejects(source: &str, message: &str) {
    let result = execute(source, "check");
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!result.status.success(), "accepted: {source}");
    assert!(output.contains(message), "expected {message:?}: {output}");
}

const AUTH: &str = r#"
trait AuthorizedUser permit LegitUser, Admin { bool isAuthorized() }
trait OpenUser { bool isAuthorized() }
class LegitUser { bool isAuthorized() { return true } }
class Admin { bool isAuthorized() { return true } }
class Stranger { bool isAuthorized() { return false } }
"#;

#[test]
fn permitted_classes_work_in_fields_arguments_returns_and_nullable_values() {
    runs(
        &format!(
            r#"{AUTH}
        class Login(AuthorizedUser user)
        class App {{
            AuthorizedUser login() {{ return LegitUser() }}
            AuthorizedUser yieldedLogin() {{ yield Admin() }}
            bool authorize(AuthorizedUser user) {{ return user.isAuthorized() }}
            bool open(OpenUser user) {{ return user.isAuthorized() }}
            start() {{
                AuthorizedUser first = this.login()
                var user = first
                user = Admin()
                print(user.isAuthorized())
                print(this.authorize(LegitUser()))
                print(Login(Admin()).user().isAuthorized())
                AuthorizedUser? missing = null
                print((missing else LegitUser()).isAuthorized())
                print(this.open(Stranger()))
                final callback = (AuthorizedUser value) -> value.isAuthorized()
                print(callback(Admin()))
                print(this.yieldedLogin().isAuthorized())
                final channel = Channel<AuthorizedUser>(1)
                Channel<AuthorizedUser> alias = channel
                alias.send(Admin()).await()
                print(channel.receive().await().isAuthorized())
                channel.close()
            }}
        }}"#
        ),
        "true\ntrue\ntrue\ntrue\nfalse\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn restricted_trait_widening_requires_a_permit_subset_and_compatible_methods() {
    runs(
        r#"
        trait Narrow permit Admin { bool isAuthorized() }
        trait Wide permit Admin, User { bool isAuthorized() }
        trait Open { bool isAuthorized() }
        class Admin { bool isAuthorized() { return true } }
        class User { bool isAuthorized() { return false } }
        class App {
            Wide widen(Narrow user) { return user }
            Open erase(Wide user) { return user }
            start() {
                Narrow user = Admin()
                print(this.widen(user).isAuthorized())
                print(this.erase(User()).isAuthorized())
            }
        }"#,
        "true\nfalse\n",
    );
}

#[test]
fn defaults_are_materialized_only_for_permitted_classes() {
    runs(
        r#"
        trait Greeting permit Person { string greet() { return 'hello' } }
        class Person
        class Other { string greet() { return 'other' } }
        class App {
            string greet(Greeting value) { return value.greet() }
            start() { print(this.greet(Person())) }
        }"#,
        "hello\n",
    );
    rejects(
        r#"
        trait Greeting permit Person { string greet() { return 'hello' } }
        class Person
        class Other
        class App { start() { Greeting value = Other() } }
    "#,
        "Mismatched initializer type",
    );
}

#[test]
fn restricted_exception_matching_checks_concrete_identity() {
    runs(
        r#"
        trait AllowedError permit TrustedError { string message() }
        class TrustedError(string message)
        class OtherError(string message)
        class App {
            start() {
                try { throw TrustedError('trusted') }
                catch (AllowedError error) { print(error.message()) }
                try { throw OtherError('other') }
                catch (AllowedError error) { print('wrong handler') }
                catch (Error error) { print(error.message()) }
            }
        }"#,
        "trusted\nother\n",
    );
}

#[test]
fn unlisted_implementations_cannot_enter_restricted_traits() {
    for app in [
        "start() { AuthorizedUser user = Stranger() }",
        "start() { AuthorizedUser? user = Stranger() }",
        "start() { var user = this.login() user = Stranger() } AuthorizedUser login() { return Admin() }",
        "AuthorizedUser login() { return Stranger() } start() {}",
        "AuthorizedUser login() { yield Stranger() } start() {}",
        "AuthorizedUser login() { yield print('forged') } start() {}",
        "Admin login() { yield Stranger() } start() {}",
        "bool allowed(AuthorizedUser user) { return user.isAuthorized() } start() { this.allowed(Stranger()) }",
        "start() { final f = (AuthorizedUser user) -> user.isAuthorized() f(Stranger()) }",
        "start() { OpenUser erased = Admin() AuthorizedUser user = erased }",
        "AuthorizedUser login() { return print('forged') } start() {}",
        "start() { AuthorizedUser user = print('forged') }",
        "start() { Admin forged = print('forged') AuthorizedUser user = forged }",
        "start() { AuthorizedUser user = if (true) { yield Admin() } else { yield Stranger() } }",
        "start() { AuthorizedUser user = if (true) { yield Admin() } }",
        "start() { final mock = Mockery.mock(AuthorizedUser) }",
        "start() { AuthorizedUser user = if (true) { yield Stranger() } else { yield Stranger() } }",
        "start() { final f = (AuthorizedUser user) -> user.isAuthorized() OpenUser erased = Stranger() f(erased) }",
        "start() { final restricted = Channel<AuthorizedUser>(1) Channel<OpenUser> widened = restricted widened.send(Stranger()) AuthorizedUser user = restricted.receive().await() }",
    ] {
        let source = format!("{AUTH} class App {{ {app} }}");
        let result = execute(&source, "check");
        assert!(!result.status.success(), "accepted: {source}");
    }
    rejects(&format!("{AUTH} class Holder(AuthorizedUser user) class App {{ start() {{ Holder(Stranger()) }} }}"), "for argument 1");
    rejects(
        r#"
        trait Narrow permit Admin { bool isAuthorized() }
        trait Wide permit Admin, User { bool isAuthorized() }
        class Admin { bool isAuthorized() { return true } }
        class User { bool isAuthorized() { return true } }
        class App { start() { Wide user = Admin() Narrow invalid = user } }
    "#,
        "Mismatched initializer type",
    );
}

#[test]
fn permit_declarations_are_validated_before_use() {
    for (source, message) in [
        ("trait Contract permit Missing { int value() } class App { start() {} }", "must name a concrete class"),
        ("trait Contract permit Other { int value() } trait Other { int value() } class App { start() {} }", "must name a concrete class"),
        ("trait Contract permit Contract { int value() } class App { start() {} }", "must name a concrete class"),
        ("trait Contract permit Good, Good { int value() } class Good(int value) class App { start() {} }", "Duplicate class"),
        ("trait Contract permit Bad { int value() } class Bad class App { start() {} }", "does not satisfy"),
        ("trait Contract permit Bad { int value() } class Bad(string value) class App { start() {} }", "does not satisfy"),
        ("trait Contract permit Bad { int value() } class Bad { static int value() { return 1 } } class App { start() {} }", "does not satisfy"),
        ("trait Contract permit { int value() }", "Expected concrete class name"),
        ("trait Contract permit Good, { int value() }", "Expected concrete class name"),
        ("trait Contract permit int { int value() }", "Expected concrete class name"),
        ("trait Contract extends Other { int value() }", "Expected {"),
    ] {
        rejects(source, message);
    }
}

#[test]
fn permit_remains_a_contextual_keyword() {
    runs(
        r#"class App {
        int permit(int value) { return value }
        start() { final permit = 7 print(this.permit(permit)) }
    }"#,
        "7\n",
    );
}

#[test]
fn trait_defaults_cannot_bypass_restrictions() {
    rejects(
        r#"
        trait Authorized permit Admin { bool allowed() }
        trait Factory { Authorized create() { return Stranger() } }
        class Admin { bool allowed() { return true } }
        class Stranger { bool allowed() { return false } }
        class App { start() {} }
    "#,
        "Mismatched return type",
    );
}

#[test]
fn documented_example_runs_with_explicit_file_imports() {
    let entry = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/traits/App.ube");
    let output = Command::new("timeout")
        .args(["60s", env!("CARGO_BIN_EXE_ube-compiler"), "run"])
        .arg(entry)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "true\ntrue\nfalse\n"
    );
}
