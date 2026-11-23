mod support;
use std::process::Command;
fn run(source: &str) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    let entry = support::write_source(&dir.path().join("App.ube"), source);
    Command::new("timeout")
        .args(["60s", env!("CARGO_BIN_EXE_ube-compiler"), "run"])
        .arg(entry)
        .output()
        .unwrap()
}
fn runs(source: &str, want: &str) {
    let o = run(source);
    assert!(
        o.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&o.stdout), want);
}
fn rejects(source: &str) {
    let dir = tempfile::tempdir().unwrap();
    let entry = support::write_source(&dir.path().join("App.ube"), source);
    let o = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
        .arg("check")
        .arg(entry)
        .output()
        .unwrap();
    assert!(!o.status.success(), "unexpected success");
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("[error]"),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&o.stderr).contains("panicked"),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}
#[test]
fn user_classes_preserve_fields_and_distinct_instantiations() {
    runs(
        r#"
class Box<T>(var T value) { T get() { return this.value() } }
class Pair<K,V>(K key,V value)
class App { start(){
    (Box of string) a = (Box of string)('hello')
    final b = Box(42)
    a.value('world')
    print(a.get())
    print(b.get())
    final pair = (Pair of string,(Box of int))('answer',b)
    print(pair.value().get())
} }
"#,
        "world\n42\n42\n",
    );
}
#[test]
fn generic_methods_infer_or_accept_explicit_arguments() {
    runs(
        r#"
class Utils { static <T> T identity(T value) { return value } }
class Box<T>(T value) { <U> U choose(U other) { return other } }
class App { start(){
    print(Utils.identity('hello'))
    print(Utils.identity<int>(42))
    print((Box of string)('a').choose<long>(99L))
} }
"#,
        "hello\n42\n99\n",
    );
}
#[test]
fn maps_check_two_types_and_distinguish_missing_from_null() {
    runs(
        r#"
class App { start(){
    (Map of string:int) counts
    counts.put('a',1)
    counts.put('a',2)
    print(counts.length())
    print(counts.get('a') else 0)
    print(counts.containsKey('missing'))
    print(counts.remove('a'))
    print(counts.length())
    (Map of string:string?) names
    names.put('a',null)
    print(names.containsKey('a'))
    print(names.get('a') else 'null value')
} }
"#,
        "1\n2\nfalse\ntrue\n0\ntrue\nnull value\n",
    );
}
#[test]
fn anonymous_union_arguments_narrow_and_keep_primitive_tags() {
    runs(
        r#"
class Cat(string name)
class Dog(int age)
class App { start(){
    (List of Cat | Dog) animals = [Cat('Kit'),Dog(4)]
    each animal in animals {
        if animal is Cat { print(animal.name()) }
        else { if animal is Dog { print(animal.age()) } }
    }
    (Map of string | long:string) labels
    labels.put('one','text')
    labels.put(1L,'number')
    print(labels.get('one') else 'missing')
    print(labels.get(1L) else 'missing')
} }
"#,
        "Kit\n4\ntext\nnumber\n",
    );
}
#[test]
fn named_generic_unions_construct_and_read_payloads() {
    runs(
        r#"
class Result<T,E> { case Ok(T value) case Err(E error) }
class App { start(){
    (Result of string,int) good = (Result of string,int).Ok('done')
    final bad = (Result of string,int).Err(7)
    print(good.isOk())
    print(good.unwrapOk())
    print(bad.isErr())
    print(bad.unwrapErr())
    (List of (Result of string,int)) results = [good,bad]
    print(results.length())
} }
"#,
        "true\ndone\ntrue\n7\n2\n",
    );
}
#[test]
fn nested_lists_and_map_literals_keep_types() {
    runs(
        r#"
class Box<T>(T value)
class App { start(){
    (List of (List of int)) matrix = [[1,2],[3]]
    print(matrix.get(0).get(1))
    (Map of string:(Box of int)) values = ['answer': (Box of int)(42)]
    final answer = values.get('answer') else (Box of int)(0)
    print(answer.value())
} }
"#,
        "2\n42\n",
    );
}
#[test]
fn wrong_arguments_and_invariant_aliases_are_rejected() {
    for source in [
        "class Box<T>(T value) class App { start(){ (Box of int) b = (Box of string)('bad') } }",
        "class Box<T>(T value) class App { start(){ final b = (Box of int)('bad') } }",
        "class Box<T>(T value) class App { start(){ final b = (Box of int,string)(1) } }",
        "class App { start(){ (Map of string:int) values values.put(1,'bad') } }",
        "class App { start(){ (List of string) names (Map of string:int) values = names } }",
    ] {
        rejects(source);
    }
}

#[test]
fn generic_traits_bounds_and_recursive_types() {
    runs(
        r#"
trait Value<T> { T value() }
class Box<T>(T value)
class Node<T>(T value, (Node of T)? next)
class Utils { static <T: (Value of string)> string read(T item) { return item.value() } }
class App { start(){
    (Value of string) text = (Box of string)('hello')
    print(text.value())
    print(Utils.read((Box of string)('bound')))
    final node = (Node of int)(1,(Node of int)(2,null))
    final next = node.next() else (Node of int)(0,null)
    print(next.value())
} }
"#,
        "hello\nbound\n2\n",
    );
    rejects("trait Named { string name() } class Box<T: Named>(T item) class App { start(){ final b = (Box of int)(1) } }");
}

#[test]
fn nullable_values_keep_presence_and_map_indexing() {
    runs(
        r#"
class Result<T,E> { case Ok(T value) case Err(E error) }
class App { start(){
    (List of int?) values = [0,null,42]
    values.add(null)
    values.add(7)
    each value in values { print(value else -1) }
    (Map of string:int?) counts
    counts['zero'] = 0
    counts['null'] = null
    print(counts['zero'] else -1)
    print(counts['null'] else -1)
    print(counts.containsKey('null'))
    print(counts.values().length())
    final result = (Result of string?,int).Ok(null)
    print(result.unwrapOk() else 'null')
} }
"#,
        "0\n-1\n42\n-1\n7\n0\n-1\ntrue\n2\nnull\n",
    );
}

#[test]
fn unions_of_generic_types_and_union_widening() {
    runs(
        r#"
class App { start(){
    (List of int) ints = [1]
    (List of string) strings = ['text']
    (List of int) | (List of string) value = strings
    if value is (List of string) { print(value.get(0)) }
    int | string narrow = 42
    int | string | bool wide = narrow
    if wide is int { print(wide) }
    int | string? nullable = null
    if nullable is string? { print(nullable else 'null') }
} }
"#,
        "text\n42\nnull\n",
    );
}

#[test]
fn invalid_union_defaults_and_constructors_are_rejected() {
    rejects("class App { start(){ int | string value } }");
    rejects("class Result<T> { case Ok(T value) } class App { start(){ (Result of int) result } }");
    rejects("class Result<T> { case Ok(T value) } class App { start(){ final r = (Result of int)(7,null) } }");
}

#[test]
fn contextual_case_inference_runtime_type_tests_and_specialization_caches() {
    runs(
        r#"
trait Value<T> { T value() }
class Box<T>(T value) { T choose(bool first) { return first then this.value() else this.value() } }
class Result<T,E> { case Ok(T value) case Err(E error) }
class Utils { static <K,V> V? lookup((Map of K:V) values,K key) { return values.get(key) } }
class App { start(){
    (Result of string,int) result = Result.Ok('inferred')
    print(result.unwrapOk())
    (Value of string) object = (Box of string)('typed')
    if object is (Box of string) { print(object.choose(true)) }
    print(object is (Box of int))
    print((Box of int)(42).choose(false))
    (Map of string:int) values
    values.put('answer',42)
    print(Utils.lookup(values,'answer') else 0)
    try { result.unwrapErr() } catch Error error { print('wrong case caught') }
} }
"#,
        "inferred\ntyped\nfalse\n42\n42\nwrong case caught\n",
    );
}

#[test]
fn generic_callbacks_and_explicit_union_arguments() {
    runs(
        r#"
class Utils {
    static <T> T identity(T value) { return value }
    static <T,U> U apply(T value, Func(T) => U transform) { return transform(value) }
}
class App { start(){
    int | string answer = Utils.identity<int | string>(42)
    if answer is int { print(answer) }
    print(Utils.apply(3,(int value) => { return value + 1 }))
    print(Utils.identity(Math.squareRoot(4.0)))
} }
"#,
        "42\n4\n2\n",
    );
}

#[test]
fn expanding_generic_recursion_is_diagnosed_without_overflow() {
    rejects("class App { static <T> recurse() { static.recurse<(List of T)>() } start(){ static.recurse<int>() } }");
}

#[test]
fn async_and_channels_preserve_generic_union_payloads() {
    runs(
        r#"
class Utils {
    static <T> T identity(T value) { return value }
    static async <T> T echo(T value) { return value }
}
class App { start(){
    final task = Utils.echo<string>('async')
    print(Utils.identity(task).await())
    final other = Async.start(() => 'callback')
    print(Utils.identity(other).await())
    final channel = (Channel of int | string)(1)
    channel.send('union').await()
    final value = channel.receive().await()
    if value is string { print(value) }
    channel.close()
} }
"#,
        "async\ncallback\nunion\n",
    );
}

#[test]
fn inference_preserves_untyped_collection_and_comprehension_types() {
    runs(
        r#"
class Box<T>(T value)
class Utils { static <T> T identity(T value) { return value } }
class App { start(){
    print(Box([1,2]).value().length())
    final values = for i = 1 to 3 { yield Utils.identity(i) }
    print(Utils.identity(values).length())
} }
"#,
        "2\n3\n",
    );
}

#[test]
fn inference_composes_with_typed_streams_and_union_traits() {
    runs(
        r#"
trait Succeeded { bool isOk() }
class Result<T> { case Ok(T value) case Err(string message) }
class Utils { static <T> T identity(T value) { return value } }
class App { start(){
    final stream = Stream.of(1,2).map((int value) => value + 1)
    print(Utils.identity(stream).sum())
    Succeeded result = (Result of int).Ok(42)
    print(result.isOk())
} }
"#,
        "5\ntrue\n",
    );
}

#[test]
fn runnable_ube_generic_example() {
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/generics/App.ube");
    let output = Command::new("timeout")
        .args(["60s", env!("CARGO_BIN_EXE_ube-compiler"), "run"])
        .arg(example)
        .output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout),
        include_str!("../../examples/generics/expected.txt"));
}

#[test]
fn angle_bracket_type_uses_are_rejected() {
    for source in [
        "class App { start() { List<string> names } }",
        "class App { start() { Map<string, int> values } }",
        "class App { start() { final values = List<int>() } }",
        "class App { start() { final values = new List<int>() } }",
        "class App { List<string> names() { return ['a'] } start() {} }",
        "class App { start() { (List of) values } }",
        "class App { start() { (Map of string,) values } }",
        "class App { start() { (List of List<int>) values } }",
    ] { rejects(source); }
}

#[test]
fn maps_require_a_colon_between_exactly_two_types() {
    for source in [
        "class App { start() { (Map of string,int) values } }",
        "class App { start() { final values = (Map of string,int)() } }",
        "class App { start() { final values = new (Map of string,int)() } }",
        "class App { start() { (Map of string) values } }",
        "class App { start() { (Map of string:) values } }",
        "class App { start() { (Map of :int) values } }",
        "class App { start() { (Map of string:int:bool) values } }",
        "class App { start() { (Map of string:int,bool) values } }",
        "class App { start() { (List of string:int) values } }",
    ] { rejects(source); }
}
