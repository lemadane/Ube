# Exact Money and recoverable financial errors

Use string inputs and outputs when exact decimal values matter:

```ube
class App {
    start() {
        final subtotal = Money.parse(
          '0.10',
          'PHP')
        final shipping = Money.parse(
          '0.20',
          'PHP')
        print(
          subtotal.add(shipping)
            .amountText()) // 0.3
        print(
          Money.parse(
            '100',
            'PHP')
            .multiplyPercent('12.5')
            .amountText()) // 12.5
        final dollars = Money.parse(
          '3.5',
          'PHP')
          .convertExact(
            'USD',
            '0.1')
        print(
          dollars.amountText()) // 0.35
    }
}
```

| API | Behavior |
| --- | --- |
| `Money.parse(amountText, currencyCode)` | Exact fixed-point input; three uppercase ASCII letters for the currency code. |
| `money.amountText()` | Exact normalized decimal output, without implied currency-specific formatting. |
| `money.add(other)`, `money.subtract(other)` | Checked exact arithmetic, requiring matching currency codes. |
| `money.multiplyPercent(percentText)` | Exact percentage multiplication, including division by 100. |
| `money.convertExact(currencyCode, rateText)` | Exact multiplication by the supplied rate, with an explicit target code. |

Money uses a decimal significand of up to 96 bits and up to 28 fractional digits.
Inputs are bounded to 64 characters, accept fixed-point decimal notation, and
reject exponents, separators, and whitespace. Arithmetic uses checked integer
intermediates and rejects overflow or any required rounding with `MoneyError`.
An intermediate overflow is rejected even if a more elaborate algorithm could
represent the final result. No implicit currency scale or rounding policy is
applied. Choose those policies at the application boundary.

Numeric factories such as `Money.php(0.1)`, the numeric `Money(...)` constructor,
`Percent`, and numeric exchange rates still accept `f64`. Their shortest decimal
representation is converted into checked Money arithmetic, but information
already lost in a floating-point expression cannot be recovered. `amount()` is
an approximate floating-point compatibility view. Use `parse()` and
`amountText()` throughout exact workflows. **Language `decimal` remains `f64`.**
Compiler and runtime must be rebuilt together; Money's runtime object now carries
its canonical decimal amount alongside the compatibility view.

## Errors and process survival

`add()` and `subtract()` set a pending `CurrencyMismatchError` on a mismatch.
Invalid exact inputs, overflow, or precision loss set `MoneyError`. Generated
code checks these errors before using the returned value. Concrete catches,
structural `catch (Error error)`, propagation through awaited tasks, and `finally`
cleanup use the normal exception ABI.

```ube
try {
    Money.parse(
      '10',
      'PHP')
      .add(
        Money.parse(
          '5',
          'USD'))
} catch (CurrencyMismatchError error) {
    print(
      error.message())
} finally {
    print('cleanup')
}
```

Operands remain unchanged. Uncaught errors at application entry follow normal
unhandled-exception reporting and exit after cleanup. Uncaught server-route
errors produce HTTP 500 while the server continues serving requests.

## Audit and validation status

| Concern | Status |
| --- | --- |
| Currency mismatch terminates inside arithmetic | Fixed with typed exceptions, native execution, async propagation, cleanup, and compiled-server recovery tests. |
| Floating-point money | Exact text API and checked internal decimal arithmetic implemented. Numeric compatibility APIs and language `decimal` remain approximate boundaries. |
| Collection lookup ambiguity | Integer map indexing now preserves missing entries separately from stored zero. Legacy raw `ube_map_get`/`ube_list_get` exports retain their payload-only ABI; compiler map lowering performs a presence check and list indexing performs bounds checks. |
| Failed array allocation retains capacity | Fixed with checked size multiplication and zero capacity on failure; tested through the exported C ABI. |
| CI evidence | A [workflow](../.github/workflows/ci.yml) now runs debug, optimized native, and isolated database tests. A remote run has not yet been observed. |

The [audit suite](../compiler/tests/runtime_audit_e2e.rs) compiles and executes
real Ube programs through the CLI and current runtime archive. It also compiles C
consumers for array and tracing-heap APIs without public Ube-language bindings.
Both former known-failure regressions (decimal Money and missing map entries)
are now enabled; no audit correctness tests are ignored.

```sh
cargo test -p ube-compiler --test runtime_audit_e2e -- --test-threads=2
cargo test -p ube-compiler --test server_e2e compiled_server_survives_caught_and_uncaught_currency_mismatches
```

See [stabilization progress](stabilization.md) for remaining release gates,
including runtime-resource tracing and sustained service reliability.
