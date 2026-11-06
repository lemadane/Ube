# Testicle: native Ube tests

Testicle is Ube's built-in testing API. Write tests directly in Ube and run them with `ube test`. No library or test
annotations are required.

Use [Mockery](mockery.md) to inject strict trait mocks, configure primitive return
values or failures, and verify call counts.
Use [RestInPeace](rest-in-peace.md) for integration and end-to-end tests against
running HTTP services, with fluent requests and Testicle assertions.

Create `tests/CalculatorTests.ube`:

```ube
class CalculatorTests {
    beforeEach() { print('Starting test') }
    afterEach() { print('Finished test') }

    testAddition() {
        Testicle.equal(4, 2 + 2)
        Testicle.notEqual(0, 4)
        Testicle.isTrue(4 > 0)
        Testicle.isFalse(4 < 0)
    }

    async testAsync() {
        final values = Channel<int>(1)
        final sent = await values.send(42)
        Testicle.equal(42, await values.receive())
        values.close()
    }
}
```

```sh
ube test
ube test tests
ube test tests/CalculatorTests.ube
ube test tests --timeout 5000
```

With no path, the runner searches `tests/`. Directory discovery recursively
selects `*Tests.ube`, `*Test.ube`, `*_test.ube`, and `*.test.ube` files in sorted path order and skips symbolic
links. Every source filename must match its declared class or trait name exactly. Each class method starting with
`test` is a test. Test classes must have no constructor parameters or sum cases.
Tests and optional `beforeEach`/`afterEach` hooks take no parameters and omit the
return type. Tests and hooks may be async; the runner awaits them.

Each test gets a fresh class instance in a separate native process. Setup runs
before the test; teardown runs through `finally`, including when setup or an
assertion throws. A timeout or process termination cannot guarantee teardown.
Test files may contain helper classes, but must not define `main()` or the
reserved generated class `UbeTestRunner`.

## Assertions

| Method | Behavior |
| --- | --- |
| `Testicle.equal(expected, actual)` | Requires equal primitive values. |
| `Testicle.notEqual(expected, actual)` | Requires different primitive values. |
| `Testicle.isTrue(condition)` | Requires `true`. |
| `Testicle.isFalse(condition)` | Requires `false`. |
| `Testicle.fail(message)` | Always raises `AssertionError` with the supplied string. |

Equality assertions require two non-null values of the same primitive type:
`int`, `long`, `byte`, `decimal`, `bool`, or `string`. Strings compare by content.
Decimal comparison uses the existing floating-point equality semantics; use
`Money.amountText()` for exact monetary assertions. Objects and collections do
not yet have deep equality assertions.

Assertions work in ordinary Ube programs too. `AssertionError` follows the normal
exception ABI and satisfies `Error`. To test an expected exception, use a boolean
flag so failure to throw cannot silently pass:

```ube
testExpectedError() {
    var caught = false
    try { Money.parse('invalid', 'USD') }
    catch (MoneyError error) { caught = true }
    Testicle.isTrue(caught)
}
```

## Results and limits

The runner prints a `PASS` or `FAIL` label with the file, class, and method, then
a total. Test stdout/stderr remains visible. It continues after failed tests and
returns exit code `1` for failures, invalid input, or an empty suite; all passing
tests return `0`. The default execution timeout is 10,000 milliseconds per test.
`--timeout` accepts a positive integer in milliseconds and excludes compilation.

This first runner compiles and links a separate executable per test. It favors
process isolation over startup speed. Filtering, parameterized tests, coverage,
JUnit XML reports, assertion value diffs, and deep equality are not implemented.
Rebuild/install the compiler and runtime together when upgrading.

## XML tests written in Ube

The checked-in [XML Testicle suite](../examples/xml/tests/XmlTests.ube) contains 12 tests with assertions directly in Ube. Run it from the repository root after rebuilding the matching compiler/runtime:

```bash
./target/debug/ube-compiler test examples/xml/tests
```

The suite covers object/list/map serialization, nulls, Unicode/escaping, namespace options, document editing, GC handle survival, malformed XML, invalid customization, cycles/resources, and async calls. Mockery supplies an XML source to a consumer, testing successful parsing, malformed XML, and upstream failures with call-count verification. XML parsing and encoding use the real runtime.

`compiler/tests/xml_e2e.rs` copies and runs this same suite through `ube test` in an isolated project as part of Cargo testing. Rust additionally exercises filesystem/process boundaries, parser limits, compiler diagnostics, and optimized native code.

## Serialization end-to-end regressions

From the repository root, rebuild the matching compiler/runtime and run:

```bash
cargo build -p ube-runtime -p ube-compiler --offline
cargo test -p ube-runtime xml::tests --offline
cargo test -p ube-compiler --test xml_e2e --offline -- --nocapture
cargo test -p ube-compiler --test key_value_store_e2e --offline
```

Offline commands require dependencies to have been fetched already. Native compiler tests require `clang` and the matching runtime archive.

`compiler/tests/xml_e2e.rs` covers objects, lists, maps, nulls, Unicode, escaping, customization, namespace-aware traversal, document editing, malformed input, unsupported values, cycles, diagnostics, and async calls. It also copies the shipped XML project into an isolated directory, runs the CLI `run` and `build` paths, and checks the example README's expected output.

Additional native fixtures reject invalid numeric character references, duplicate namespace-expanded attributes, external entities, and inputs exceeding byte, depth, or node limits. They preserve CDATA, mixed content, attribute whitespace, namespace shadowing, and detached subtree namespaces. Customization tests cover root-only rules, reusable options, null/object attributes, expanded-name collisions, and reserved nil attributes. Document handles remain usable after edits and GC. Encoder-limit failures and subsequent valid conversions run both with the default build and LLVM `-O2`.

The file round-trip regression compiles separate native writer and reader programs. The writer persists XML; fresh reader processes parse and verify fields, lists, maps, namespaces, nulls, and an integer larger than JavaScript's exact integer range. A corrupted file must fail with `XmlParseError`. These tests exercise real generated executables and filesystem I/O. The KeyValueStore suite protects JSON serialization and persistence paths shared with the data traversal.

See the [XML reference](xml.md), [runnable XML project](../examples/xml/README.md), and [JSON reference](json.md). JUnit XML reporting remains a separate, unimplemented Testicle feature.
