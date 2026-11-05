# Testicle: native Ube tests

Testicle is Ube's built-in testing API. Write tests directly in Ube and run them with `ube test`. No library or test
annotations are required.

Use [Mockery](mockery.md) to inject strict trait mocks, configure primitive return
values or failures, and verify call counts.

Create `tests/calculator_test.ube`:

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
ube test tests/calculator_test.ube
ube test tests --timeout 5000
```

With no path, the runner searches `tests/`. Directory discovery recursively
selects `*_test.ube` and `*.test.ube` files in sorted path order and skips symbolic
links. An explicit `.ube` file can have any name. Each class method starting with
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
