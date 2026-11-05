# Financial runtime behavior

`Money.add(other)` and `Money.subtract(other)` require identical currency codes.
A mismatch returns no result through the native ABI and sets a pending
`CurrencyMismatchError`. Generated code checks the exception before using the
result, so normal catches, propagation through calls and awaited tasks, and
`finally` cleanup apply.

```ube
class Application {
    main() {
        try {
            final total = Money.php(10.0).add(Money.usd(5.0))
        } catch (CurrencyMismatchError error) {
            print(error.message())
        } finally {
            print('cleanup')
        }
        print('still running')
    }
}
```

The message is `Money currency mismatch: PHP vs USD` for these operands. The
operands remain unchanged. Operations with matching currency codes return a new
money value. Currency conversion remains an explicit operation.

`CurrencyMismatchError(message)` also constructs an ordinary error object.
`catch (Error error)` can handle it through the structural error contract. If
uncaught at an application entry point, it follows normal unhandled-exception
reporting and exits after cleanup. If uncaught inside a server route, the existing
request boundary returns HTTP 500 and the server continues serving requests.

## Audit status and remaining limitations

| Concern | Current status |
| --- | --- |
| Currency mismatch terminates inside arithmetic | Fixed: typed pending exception, checked by generated code. Native tests cover addition, subtraction, cleanup, async propagation, and unhandled reporting. A compiled-server test covers continued service after caught and uncaught errors. |
| Binary floating-point amounts | Still a limitation: `Money.amount`, percentage/exchange arithmetic, and the language `decimal` use `f64`. Exact decimal money is not implemented. The README's exact-arithmetic claim has been corrected. Representation, scale, overflow, rounding, and conversion rules need a coordinated API/ABI change; see the existing [roadmap](../MILESTONES.md). |
| Collection lookup returns zero for absence | Still a limitation of the raw `ube_list_get` and `ube_map_get` exports. Generated list indexing performs bounds checks, but generated map lookup still cannot consistently distinguish a missing entry from a stored zero. A presence-aware map lookup and compiler lowering remain follow-up work. |
| Failed array allocation retains capacity | Fixed: checked size multiplication and zero capacity whenever allocation fails. Tests cover overflow, invalid allocator layout, and normal allocation. No enormous OS allocation is needed for these tests. |
| Automated CI evidence | No GitHub Actions workflow files are present in this checkout. Local test results do not establish remote CI health or prove that no remote runs exist. No remote CI verification or workflow publication was performed as part of this fix. |

## End-to-end audit tests

[The audit suite](../compiler/tests/runtime_audit_e2e.rs) covers all four runtime
concerns. The currency, list, and precision/map programs run through the real CLI,
compiler, LLVM generation, native linking, and execution. The test fixture pairs
the compiler with the current runtime archive, avoiding stale installed libraries.
Array allocation has no Ube-language binding, so that test compiles and executes
a C consumer of the exported runtime ABI, including the returned struct layout.

On Unix with `clang` and `timeout` installed:

```sh
cargo test -p ube-compiler --test runtime_audit_e2e -- --test-threads=2
```

Current result: **3 passed, 2 ignored**. Passing cases cover typed currency
recovery and cleanup, list zero values versus out-of-bounds access, allocation
failure invariants, and successful allocations after failure. Server recovery is
covered separately:

```sh
cargo test -p ube-compiler --test server_e2e compiled_server_survives_caught_and_uncaught_currency_mismatches
```

The two ignored cases assert the intended behavior for exact decimal money and
missing map entries. They are explicitly marked as known unresolved bugs; they do
not assert that the incorrect behavior is acceptable. Run them deliberately with:

```sh
cargo test -p ube-compiler --test runtime_audit_e2e -- --ignored --test-threads=2
```

Both currently **fail**, as confirmed locally: money comparisons print `false`
where exact arithmetic requires `true`, and a missing map lookup returns zero
instead of selecting the `?? -1` fallback. Enable these regressions by removing
their ignore annotations when the implementations are fixed. These results are
local verification, not evidence of a successful remote CI run.
