//! Collection streams through the native compiler and runtime.
mod support;
use std::{fs, process::Command};
fn run(source: &str, expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("App.ube");
    let entry = support::write_source(&entry, source);
    let output = Command::new("timeout")
        .args(["60s", env!("CARGO_BIN_EXE_ube-compiler"), "run"])
        .arg(entry)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}
#[test]
fn integer_pipelines_collect_reduce_and_short_circuit() {
    run(
        r#"class App { start() {
        final values = [5, 2, 3, 2, 1]
        final result = values.stream()
            .filter((int n) -> n > 1)
            .unique().sort()
            .map((int n) -> n * 10)
            .skip(1).limit(2).toList()
        print(result.length())
        print(result[0])
        print(result.get(1))
        result.add(60)
        each (n in result) { print(n) }
        print(result.stream().sum())
        print(Stream.range(1, 5).reduce(0, (int a, int b) -> a + b))
        print(Stream.of(1, 2, 3).some((int n) -> n == 2))
        print(Stream.of(1, 2, 3).every((int n) -> n > 0))
        print(Stream.of(1, 2, 3).none((int n) -> n < 0))
        print(Stream.of().count())
        print(Stream.of().findFirst() == null)
        print(Stream.of(0).findFirst() else 9)
        print(Stream.range(0, 2147483647).limit(2).count())
    } }"#,
        "2\n30\n50\n30\n50\n60\n140\n10\ntrue\ntrue\ntrue\n0\ntrue\n0\n2\n",
    );
}
#[test]
fn typed_mapping_strings_decimals_and_callbacks() {
    run(
        r#"class App {
        Stream<string> names() { return Stream.of('pear', 'apple', 'pear') }
        start() {
            List<string> names = this.names().unique().sort().collect()
            print(names[0])
            each (name in names) { print(name) }
            names.stream().forEach((string name) -> print(name))
            print(Stream.of(1, 2).map((int n) -> `item ${n}`).findFirst() else 'none')
            print(Stream.of(1.5, 2.5).sum() == 4.0)
            print(Stream.of(5000000000L, 2L).sum())
            print(Stream.of(true, false).filter((bool b) -> b).count())
            print(Stream.of('a', 'b').reduce('', (string a, string b) -> `${a}${b}`))
        }
    }"#,
        "apple\napple\npear\napple\npear\nitem 1\ntrue\n5000000002\n1\nab\n",
    );
}
#[test]
fn callbacks_are_lazy_and_stop_at_limits_matches_and_errors() {
    run(
        r#"class App {
        int fail(int n) { if (n == 2) { throw 'failed' } return n }
        start() {
            final pipeline = Stream.range(1, 5).peek((int n) -> print(n))
            print('created')
            print(pipeline.limit(1).count())
            print(Stream.range(1, 5).peek((int n) -> print(n)).some((int n) -> n == 2))
            print(Stream.range(1, 5).peek((int n) -> print(n)).limit(0).count())
            final bad = Stream.range(1, 5).map((int n) -> this.fail(n))
            try { bad.toList() } catch (GenericError error) { print('caught') }
            try { bad.count() } catch (GenericError error) { print('consumed') }
            final original = Stream.of(1, 2)
            final linked = original.filter((int n) -> true)
            try { original.count() } catch (GenericError error) { print('linked') }
            linked.close()
            linked.close()
            try { linked.count() } catch (GenericError error) { print('closed') }
        }
    }"#,
        "created\n1\n1\n1\n2\ntrue\n0\ncaught\nconsumed\nlinked\nclosed\n",
    );
}

#[test]
fn object_elements_and_captured_values_survive_gc() {
    run(
        r#"class Item(int value)
    class App {
        Stream<Item> items() { return Stream.of(Item(7), Item(9)) }
        Stream<int> captured() {
            final item = Item(4)
            return Stream.range(1, 3).map((int n) -> n + item.value())
        }
        start() {
            final objects = this.items()
            final captured = this.captured()
            GarbageCollector.collect()
            print(objects.map((Item item) -> item.value()).sum())
            print(captured.sum())
            final list = this.items().toList()
            GarbageCollector.collect()
            print(list[0].value())
            final first = Stream.range(1, 4).map((int n) -> Item(n)).findFirst()
            GarbageCollector.collect()
            print((first else Item(99)).value())
        }
    }"#,
        "16\n11\n7\n1\n",
    );
}

#[test]
fn list_sources_snapshot_and_validation_errors_are_catchable() {
    run(
        r#"class App { start() {
        final items = [1, 2]
        final stream = items.stream()
        items.add(3)
        print(stream.count())
        final negative = Stream.of(1)
        try { negative.limit(-1) } catch (GenericError error) { print('negative') }
        print(negative.count())
        try { ['text'].stream() } catch (GenericError error) { print('type') }
        final result = Stream.of(1).toList()
        try { result[-1] } catch (GenericError error) { print('bounds') }
        try { Stream.of(9223372036854775807L, 1L).sum() }
        catch (GenericError error) { print('overflow') }
        print(Stream.of().every((int n) -> false))
        print(Stream.of().none((int n) -> true))
    } }"#,
        "2\nnegative\n1\ntype\nbounds\noverflow\ntrue\ntrue\n",
    );
}

#[test]
fn invalid_callbacks_and_typed_container_forgery_are_rejected() {
    for body in [
        "Stream.of(1).filter((int n) -> n)",
        "Stream.of(1).map((string n) -> n)",
        "Stream.of(1).map((int n) -> print(n))",
        "Stream.of(1).forEach((int n) -> n)",
        "Stream.of(1).limit('1')",
        "Stream.of(1).findFirst((string n) -> true)",
        "Stream.of(1).findFirst((int n) -> n)",
        "Stream.of(1).findFirst((int n) -> true, (int n) -> true)",
        "Stream.of(true).sort()",
        "Stream.of(1, 'text')",
        "Stream.of(null)",
        "Stream.of(1).reduce(0L, (int a, int b) -> a + b)",
        "Stream<int> stream = [1, 2]",
        "List<string> names = Stream.of(1).toList()",
        "Stream.of('x').toList().add(42)",
        "Stream()",
        "final x = if (true) { yield Stream.of(1) } else { yield 1 }",
        "final x = if (true) { yield Stream.of(1).toList() }",
    ] {
        rejected(&format!("class App {{ start() {{ {body} }} }}"));
    }
    rejected("class App { Stream<int> values() { return print('x') } start() {} }");
    rejected("class App { List<int> values() { yield 42 } start() {} }");
}
fn rejected(source: &str) {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("App.ube");
    fs::write(&entry, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
        .arg("check")
        .arg(entry)
        .output()
        .unwrap();
    assert!(!output.status.success(), "accepted: {source}");
}

#[test]
fn documented_example_runs() {
    run(
        include_str!("../../examples/streams/App.ube"),
        "20\n30\n50\n100\napple\npear\n3\n-1\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn find_first_predicate_short_circuits_and_preserves_nullable_values() {
    run(
        r#"class Item(int value)
    class App {
        int failFallback() { throw 'unexpected fallback' }
        bool matches(int n) { print(n) return n == 2 }
        bool fails(int n) { throw 'predicate failed' }
        bool second(Item item) { GarbageCollector.collect() return item.value() == 2 }
        start() {
            print(Stream.range(1, 5).peek((int n) -> print(n))
                .findFirst((int n) -> this.matches(n)) else -1)
            print(Stream.of(1, 0, 2).findFirst((int n) -> n == 0) else 9)
            print(Stream.of(true, false).findFirst((bool b) -> not b) else true)
            print(Stream.of().findFirst((int n) -> this.fails(n)) == null)
            print(Stream.of(1, 2).findFirst((int n) -> n > 5) == null)
            final missing = Stream.of(1).findFirst((int n) -> n > 10)
            print(missing else -1)
            print(Stream.of(0).findFirst() else this.failFallback())
            print(Stream.of('a', 'b').findFirst((string s) -> s == 'b') else 'missing')
            final first = Stream.range(1, 4).map((int n) -> Item(n))
                .findFirst((Item item) -> this.second(item))
            GarbageCollector.collect()
            print((first else Item(99)).value())
            final bad = Stream.of(1, 2)
            try { bad.findFirst((int n) -> this.fails(n)) }
            catch (GenericError error) { print('caught') }
            try { bad.count() } catch (GenericError error) { print('consumed') }
        }
    }"#,
        "1\n1\n2\n2\n2\n0\nfalse\ntrue\ntrue\n-1\n0\nb\n2\ncaught\nconsumed\n",
    );
}

#[test]
fn matching_names_preserve_short_circuiting_empty_results_and_aliases() {
    run(
        r#"class App { start() {
            print(Stream.of().some((int n) -> true))
            print(Stream.of().every((int n) -> false))
            print(Stream.of().none((int n) -> true))
            print(Stream.range(1, 5).peek((int n) -> print(n)).some((int n) -> n == 2))
            print(Stream.range(1, 5).peek((int n) -> print(n)).every((int n) -> n < 2))
            print(Stream.range(1, 5).peek((int n) -> print(n)).none((int n) -> n == 2))
            print(Stream.of(1, 2).some((int n) -> n > 5))
            print(Stream.of(1, 2).every((int n) -> n > 0))
            print(Stream.of(1, 2).none((int n) -> n > 5))
            print(Stream.of(1, 2).anyMatch((int n) -> n == 2))
            print(Stream.of(1, 2).allMatch((int n) -> n > 0))
            print(Stream.of(1, 2).noneMatch((int n) -> n > 5))
        } }"#,
        "false\ntrue\ntrue\n1\n2\ntrue\n1\n2\nfalse\n1\n2\nfalse\nfalse\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
    for method in ["some", "every", "none"] {
        for args in ["", "(string n) -> true", "(int n) -> n"] {
            rejected(&format!(
                "class App {{ start() {{ Stream.of(1).{method}({args}) }} }}"
            ));
        }
    }
}

#[test]
fn sort_and_unique_preserve_order_laziness_and_compatibility() {
    run(
        r#"class App { start() {
            Stream.of(3, 1, 3, 2, 1).unique().forEach((int n) -> print(n))
            final values = [3, 1, 2, 1]
            final ordered = values.stream().peek((int n) -> print(n)).sort().unique()
            print('created')
            ordered.forEach((int n) -> print(n))
            print(values[0])
            Stream.of('b', 'a', 'b').unique().sort().forEach((string s) -> print(s))
            print(Stream.of().sort().unique().count())
            Stream.of(2, 1, 2).distinct().sorted().forEach((int n) -> print(n))
        } }"#,
        "3\n1\n2\ncreated\n3\n1\n2\n1\n1\n2\n3\n3\na\nb\n0\n1\n2\n",
    );
    for body in [
        "Stream.of(1).sort(1)",
        "Stream.of(1).unique(1)",
        "Stream.of(true).sort()",
        "Stream.of(true).sorted()",
    ] {
        rejected(&format!("class App {{ start() {{ {body} }} }}"));
    }
}

#[test]
fn empty_streams_and_skip_limit_boundaries() {
    run(
        r#"class App { start() {
            print(Stream.of().toList().length())
            print(Stream.of().collect().length())
            print(Stream.of().sum())
            print(Stream.of().reduce(42, (int a, int b) -> a + b))
            Stream.of().forEach((int n) -> print('unexpected'))
            print(Stream.of().filter((int n) -> true).map((int n) -> n + 1)
                .peek((int n) -> print('unexpected')).skip(1).limit(1).count())
            print(Stream.range(3, 3).count())
            print(Stream.range(4, 2).count())
            print(Stream.of(1, 2).skip(0L).limit(10L).sum())
            print(Stream.of(1, 2).skip(2).count())
            print(Stream.of(1, 2).skip(10L).count())
            print(Stream.of(1, 2).limit(0L).count())
            print(Stream.of(1, 2, 3).reduce(10, (int a, int b) -> a - b))
            final source = Stream.of(1, 2)
            try { source.skip(-1) } catch (GenericError error) { print('negative') }
            print(source.count())
        } }"#,
        "0\n0\n0\n42\n0\n0\n0\n3\n0\n0\n0\n4\nnegative\n2\n",
    );
}

#[test]
fn collection_length_replaces_the_abbreviated_name() {
    use ube_compiler::{lexer::Lexer, parser::Parser, semantics::TypeChecker};
    for receiver in ["List", "Map", "List<int>", "JsonArray", "ConsoleArgs", "TaskOutcomes"] {
        for (method, accepted) in [("length", true), ("len", false)] {
            let source = format!(
                "class App {{ int measure({receiver} values) {{ return values.{method}() }} start() {{}} }}"
            );
            let mut parser = Parser::new(Lexer::new(&source));
            let program = parser.parse_program();
            assert!(parser.diagnostics.is_empty(), "{source}");
            let mut checker = TypeChecker::new();
            assert_eq!(checker.check_program(&program), accepted,
                "{receiver}.{method}(): {:?}", checker.diagnostics);
        }
    }
}

#[test]
fn terminal_callback_errors_propagate_and_consume_each_stream() {
    let mut body = String::new();
    for (i, terminal) in [
        "forEach((int n) -> this.failVoid(n))",
        "reduce(0, (int a, int b) -> this.failInt(b))",
        "some((int n) -> this.failBool(n))",
        "every((int n) -> this.failBool(n))",
        "none((int n) -> this.failBool(n))",
    ]
    .iter()
    .enumerate()
    {
        body.push_str(&format!(
            "final stream{i} = Stream.of(1, 2)\n\
             try {{ stream{i}.{terminal} }} catch (GenericError error) {{ print(error.message()) }}\n\
             try {{ stream{i}.count() }} catch (GenericError error) {{ print('consumed') }}\n"
        ));
    }
    run(
        &format!(
            "class App {{
                void failVoid(int n) {{ throw 'callback failed' }}
                int failInt(int n) {{ throw 'callback failed' }}
                bool failBool(int n) {{ throw 'callback failed' }}
                start() {{ {body} }}
            }}"
        ),
        &"callback failed\nconsumed\n".repeat(5),
    );
}
