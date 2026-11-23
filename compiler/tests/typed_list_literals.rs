//! Contextual List<T> literal initialization through checking and native codegen.
mod support;
use std::process::Command;

fn execute(source: &str, command: &str) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    let entry = support::write_source(&dir.path().join("App.ube"), source);
    Command::new("timeout")
        .args(["60s", env!("CARGO_BIN_EXE_ube-compiler"), command])
        .arg(entry).output().unwrap()
}

fn runs(source: &str, expected: &str) {
    let output = execute(source, "run");
    assert!(output.status.success(), "{}\n{}",
        String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn string_literal_can_be_mutated_indexed_iterated_and_streamed() {
    runs(r#"class App { start() {
        (List of string) names = ['Lemuel', 'Ed']
        names.add('Dianne')
        print(names.length())
        print(names.get(2))
        print(names[0])
        each (name in names) { print(name) }
        print(names.stream().filter((string name) => name == 'Ed').count())
    } }"#, "3\nDianne\nLemuel\nLemuel\nEd\nDianne\n1\n");
}

#[test]
fn empty_literals_use_the_declared_element_type() {
    runs(r#"class App { start() {
        (List of string) names = []
        print(names.length())
        names.add('Dianne')
        print(names.get(0))
        (List of int) numbers = []
        numbers.add(42)
        print(numbers[0])
    } }"#, "0\nDianne\n42\n");
}

#[test]
fn numeric_elements_are_converted_before_storage() {
    runs(r#"class App { start() {
        (List of decimal) values = [1, 2.5]
        print(values.get(0) == 1.0)
        print(values.stream().sum() == 3.5)
        (List of long) numbers = [1, 5000000000L]
        print(numbers[0])
        print(numbers.get(1))
        (List of bool) flags = [true, false]
        print(flags.get(0))
        print(flags.get(1))
    } }"#, "true\ntrue\n1\n5000000000\ntrue\nfalse\n");
}

#[test]
fn object_elements_keep_their_types_and_are_evaluated_once_in_order() {
    runs(r#"class Person(string name)
    class App {
        string item(string value) { print(value) return value }
        start() {
            (List of string) names = [this.item('first'), this.item('second')]
            print(names.get(1))
            (List of Person) people = [Person('Lemuel')]
            people.add(Person('Dianne'))
            print(people.get(1).name())
        }
    }"#, "first\nsecond\nsecond\nDianne\n");
}

#[test]
fn literal_elements_and_later_additions_are_checked() {
    for body in [
        "(List of string) names = ['Lemuel', 42]",
        "(List of string) names = [null]",
        "(List of int) values = ['Ed']",
        "(List of int) values = [print('not a value')]",
        "(List of string) names = [] names.add(42)",
        "(List of string) names = ['Ed'] names.add(null)",
        "(List of string) names = ['Ed'] (List of int) values = names",
    ] {
        let source = format!("class App {{ start() {{ {body} }} }}");
        let output = execute(&source, "check");
        assert!(!output.status.success(), "accepted: {source}");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostic.contains("List literal") || diagnostic.contains("Invalid stream call 'add'")
            || diagnostic.contains("Mismatched initializer type"), "{diagnostic}");
    }
}

#[test]
fn untyped_literals_keep_existing_behavior() {
    runs(r#"class App { start() {
        final names = ['Lemuel', 'Ed']
        names.add('Dianne')
        print(names.length())
        final numbers = [1, 2]
        print(numbers.stream().sum())
    } }"#, "3\n3\n");
}

#[test]
fn declarations_without_initializers_create_independent_empty_lists() {
    runs(r#"class App { start() {
        (List of string) names
        (List of string) other
        print(names.length())
        each (name in names) { print('unexpected iteration') }
        print(names.stream().count())
        names.add('Dianne')
        print(names.get(0))
        print(other.length())
        (List of int) numbers
        numbers.add(42)
        print(numbers[0])
        (List of string)? absent
        print(absent == null)
    } }"#, "0\n0\nDianne\n0\n42\ntrue\n");
}

#[test]
fn list_fields_zero_initialize_in_locals_constructors_statics_and_startup() {
    runs(r#"class Group((List of string) names)
    class Node((List of Node) children)
    class App((List of string) names) {
        static final (List of string) shared
        start() {
            print(this.names().length())
            this.names().add('startup')
            print(this.names().get(0))
            static.shared.add('static')
            print(static.shared.get(0))
            Group first
            Group second
            first.names().add('local')
            print(first.names().get(0))
            print(second.names().length())
            final constructed = Group()
            print(constructed.names().length())
            Node node
            node.children().add(Node())
            print(node.children().length())
            print(node.children().get(0).children().length())
        }
    }"#, "0\nstartup\nstatic\nlocal\n0\n0\n1\n0\n");
}

#[test]
fn default_lists_still_reject_wrong_element_types() {
    for body in [
        "(List of string) names names.add(42)",
        "(List of string) names names.add(null)",
    ] {
        let output = execute(&format!("class App {{ start() {{ {body} }} }}"), "check");
        assert!(!output.status.success(), "accepted: {body}");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostic.contains("Invalid stream call 'add'")
            || diagnostic.contains("requires an explicit initializer"), "{diagnostic}");
    }
}

#[test]
fn indexed_assignment_runs_the_ube_example() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/list-index-assignment/App.ube");
    let output = Command::new("timeout")
        .args(["60s", env!("CARGO_BIN_EXE_ube-compiler"), "run"])
        .arg(path).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout),
        include_str!("../../examples/list-index-assignment/expected.txt"));
}

#[test]
fn indexed_assignment_checks_element_and_index_types() {
    for body in [
        "(List of string) names names[0] = 42",
        "(List of string) names names[0] = null",
        "(List of string) names names['0'] = 'bad'",
        "(List of string) names names[0L] = 'bad'",
        "(List of string) names names[0] = print('bad')",
        "(List of string)? names names[0] = 'bad'",
    ] {
        let source = format!("class App {{ start() {{ {body} }} }}");
        let output = execute(&source, "check");
        assert!(!output.status.success(), "accepted: {source}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("[error]"));
    }
}
