# Stabilization progress

Ube remains experimental. These changes resolve specific correctness failures;
they do not establish production readiness.

| Area | Implemented and tested | Remaining gate |
| --- | --- | --- |
| Compiler traits | Only conforming classes are specialized; defaults have concrete implementations; mixed-type trait arguments execute correctly. The original regression is compiled and run at `-O0` and `-O2`. | Wider native/compiler fuzzing and compatibility coverage. |
| Collection lookup | Integer map indexing returns a nullable value; zero remains present and missing keys select the `??` fallback. | Typed collection payloads beyond the current integer lowering and ownership integration. |
| Serialization | Automatic object/list/map JSON and XML encoding; bounded XML documents, options, namespaces, traversal, catchable failures, async calls, CLI run/build, and files exchanged across independent native processes. See [test commands](testing.md#serialization-end-to-end-regressions). | Typed application-object reconstruction, schema validation, explicit XML disposal, and sustained memory/compatibility testing. |
| Money | Exact string inputs/outputs, checked arithmetic, overflow and precision-loss rejection. | Application-level currency scale/rounding policies and financial persistence validation. Language `decimal` remains floating point by design. |
| Memory | Generated-object roots and barriers, automatic collection, cycle/concurrent-mutator tests, and loop stack-slot reuse. | Precise runtime-resource tracing/disposal, reducing conservative escape pins, and long-running process memory validation; see [memory boundary](memory.md). |
| HTTP | Bounded bodies across HTTP/1.x, HTTP/2, HTTP/3; malformed/oversized/timeout rejection, stream isolation, and compiled-server regressions. | Sustained load/security testing and unfinished server APIs. |
| Concurrency | Native stress repeats nullable zero/false/null transport, channel close, cancellation, and async cleanup 32 times. | Broader contention, interleaving, sanitizer, and soak testing. |
| CI | Workflow checks the workspace, full debug suite, isolated PostgreSQL/MySQL tests, and optimized native regressions. | A successful remote run after publication; local execution does not prove hosted CI health. |

Run `cargo test --workspace --locked -- --test-threads=2` on a machine that permits
localhost TCP/UDP sockets. Four database integration tests require isolated
services and are opt-in locally; the CI workflow supplies dedicated databases.
The workflow does not use production database credentials or a production service.

Compiler and runtime must be rebuilt together. Native audit tests deliberately
pair the compiler with the current archive instead of an older installed copy.
