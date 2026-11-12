# Ube Roadmap v2 — Proposed Scope and Release Gates

Status: proposal for prioritization, not a completion report or a production-readiness claim. This replaces the earlier fourteen-phase schedule. Current behavior is documented in [README.md](README.md), [the language specification](LANGUAGE_SPEC_DRAFT.md), and [the architecture](ARCHITECTURE.md).

The sequence is foundation, one working financial API, then extensions justified by use. Existing experimental code remains available during migration. This proposal does not remove language features, move repositories, or establish new compatibility guarantees by itself.

## Current Starting Point

- The compiler emits textual LLVM IR and invokes `clang` to produce native executables.
- Automatic `.toJson()` and `.toXml()` serialize concrete objects, lists, and maps. `Xml.toObject()` parses documents with explicit traversal; options configure roots, attributes, names, namespaces, and list items. Native tests cover CLI run/build and XML files across independent processes; see [XML](docs/xml.md). Typed XML-to-application reconstruction remains unimplemented.
- Classes, structural traits, control flow, collections, and explicit exception propagation exist, but native correctness still needs broader coverage. The trait-polymorphism regression now compiles and executes at `-O0` and `-O2`; unrelated-class specialization and default-method lowering have been corrected. This does not establish correctness for every program.
- Domain-specific names and lowering are embedded in the compiler. Its Cargo manifest directly depends on `ube-runtime`, whose dependencies include Tokio, TLS, and QUIC libraries. Package separation is a target, not the current architecture.
- The calling-thread, loopback server supports HTTP/1.x, HTTP/2, and HTTP/3 with bounded request bodies and compiled-server integration tests. Middleware, WebSockets, and other advertised advanced APIs remain incomplete.
- `Money` now has an exact text-based API and checked decimal arithmetic. Numeric factory inputs, `.amount()`, and language `decimal` remain `f64` boundaries; use decimal strings for exact financial inputs and outputs.
- Generated class objects now have tracing GC, method roots, pointer barriers, and automatic collection, with native and generated cycle/concurrency tests. Opaque escapes remain conservatively pinned and several runtime resources retain shutdown lifetimes. Whole-process reclamation and sustained service reliability remain release blockers.

## Phase 1 — Stabilize the Foundation

Planning window: approximately twelve weeks, reviewed every four weeks. Release gates decide readiness; the calendar does not promise completion.

### Scope Freeze

Concentrate new work on `class`, `trait`, `case`, `for`, `if`, `switch`, `try` / `catch` / `finally` / `throw`, `List`, `Map`, `print`, and `not` / `and` / `or`. Supporting declarations, methods, accessors, returns, loop exits, primitive types, nullability, and literal syntax remain part of that core. Existing array forms qualify for the stable subset only after native execution tests establish their supported behavior.

Keep the recently established rules consistent:

- Typed declarations and application startup zero-initialize through one implementation. Explicit constructors apply defaults. Non-nullable cycles and implicit runtime-resource initialization produce diagnostics.
- Backing fields are class-private. Public reads use `field()`; `var` writes use `field(value)`.
- Single quotes delimit one-line strings and character values. Backticks delimit multiline text blocks with escapes and `${expression}` interpolation. Double quotes are reserved for JSON content, and semicolons are prohibited in code.
- Loop `else` runs only for zero body entries. Documentation and executable examples cover normal completion, `break`, `continue`, and initially false conditions.

Fix nullable scalar representation and task/channel transport as foundation work. This is a correctness requirement for existing types, not concurrency API expansion. Maintain existing async/task/channel regressions while stabilizing their exception and cleanup paths. Defer concurrency API expansion, new protocols, financial features, typed XML reconstruction and schema validation, advanced date/time APIs, and additional syntax.

### Work

1. Inventory each advertised feature as implemented and execution-tested, experimental, unsupported, or planned. The README must distinguish those states. Unsupported operations should produce diagnostics rather than emit invalid IR or silently succeed.
2. Compile and execute every runnable core example in debug and release builds. Expand native coverage around trait dispatch, getters/setters, zero initialization, nullability, collections, nested control flow, and exception cleanup. Syntax-only tests do not establish runtime support.
3. Resolve the trait-polymorphism failure. If its textual assertion is invalid, replace it with meaningful LLVM verification and native behavior checks; establish that dispatch works before changing the assertion.
4. Check emitted IR with the supported LLVM toolchain. For rejected programs, require a source span and actionable message. Add regressions for each confirmed crash, verifier failure, miscompilation, or missed diagnostic.
5. Verify release flags and executable results. Measure direct-call optimization on representative examples; trait runtime dispatch must not be described as universally inlined or free.
6. Establish memory and resource baselines for repeated allocation, errors, task completion, and cleanup. Document existing retention and GC limitations.
7. Design the minimal package boundary: module/type loading, ordinary method resolution, a documented native ABI, runtime dependency selection, and linking of local packages. A registry and dependency solver are not prerequisites.

### Phase 1 Blocker: Nullable Scalars Across Async Boundaries

The compiler now lowers nullable primitives as an inline presence flag plus scalar payload and transports task/channel values through a two-word tagged ABI. Native regressions cover null versus zero/false, calls/returns, public fields, captures, all five nullable scalar types, repeated awaits, timeouts, failures, and closed channels. No ordinary numeric payload is reserved as a null sentinel.

Untyped collections still store single-word elements. Nullable scalars and tasks returning nullable scalars are diagnosed when stored, including task-combinator lists. Complete typed nullable collection/combinator support before declaring the full nullable matrix stable.

Remaining validation includes integer boundaries and floating-point bit patterns, multiple concurrent waiters, cancellation under contention, memory/ownership baselines, and supported target ABI coverage. Preserve existing exception and cleanup regressions. The two-word scalar representation requires no per-payload allocation; pointer/object lifetime rules remain unchanged.

### Exit Gates

- The complete supported test suite passes without excluding known compiler failures. CI jobs needing loopback access run in an environment that provides it.
- Every runnable core example builds and runs with expected output in debug and release configurations. Examples outside the stable subset are explicitly marked experimental.
- No known LLVM verifier failure or native correctness failure remains in the declared release matrix. This is a tested support claim, not a guarantee of zero undiscovered bugs.
- Trait dispatch and exception/cleanup behavior have native regressions, not just token or IR-substring assertions.
- Nullable primitives retain their presence and value across supported local, call, field, task, and channel paths. No accepted nullable scalar program in this test matrix emits invalid IR, and null never aliases zero or a failure outcome.
- Supported toolchain versions, target platform, diagnostics, memory limits, and reproducible build commands are documented.
- Domain APIs have an implementation-status inventory and a reviewable extraction design. No additional domain-specific compiler branches are introduced during the freeze.

## Phase 2 — Ship One Financial API Demonstration

Prerequisite: Phase 1 exit gates. Keep this phase to a single-currency financial API with JSON requests, exact amounts, persistence, and HTTP/1.1. Define a small demonstration such as a ledger or transfer simulation with a reproducible command and request/response examples.

### Deliver the Whole Request Path

1. **Exact amounts:** choose a documented integer-minor-unit or decimal representation, with explicit scale and rounding rules. Test checked overflow, negative values, serialization, arithmetic, and currency mismatch errors. Do not use the existing `f64` amount representation for this demonstration's monetary calculations. Defer foreign exchange and broad currency catalogs.
2. **JSON:** parse and validate request payloads, preserve exact amount representations, produce consistent responses, and return useful errors for malformed input. Do not introduce an ambiguous bare double-quoted language literal as a shortcut.
3. **Persistence:** implement and test one bounded persistence model, including restart behavior and failed-write handling. Document its durability and concurrency guarantees.
4. **HTTP/1.1:** implement the request method/path, headers, framing, body handling, status codes, and response content types required by the demonstration. Bound request size, waiting time, and concurrent work. Reject unsupported framing explicitly.
5. **Deployment:** provide explicit configurable bind host and port. Keep loopback as the documented default; exercise `0.0.0.0` binding only through explicit configuration in integration tests. Document the supported deployment and TLS termination arrangement.
6. **Behavior under failure:** verify validation failures, arithmetic errors, repeated requests, persistence errors, disconnects, cancellation, and cleanup through the complete request path. Specify idempotency and shared-state behavior for the chosen API.

### Extract Packages Incrementally

Proposed package names are organizational targets, not syntax already supported by Ube:

| Component | Responsibility |
| --- | --- |
| Core compiler | Parse, resolve, check, and lower ordinary language constructs |
| Core runtime | Allocation, core strings/collections, errors, task primitives where required, and a documented ABI |
| `ube-json` | JSON parsing, validation, and serialization |
| `ube-xml` | Bounded XML parsing, serialization, document traversal, and namespace handling |
| `ube-money` | Exact monetary values and single-currency arithmetic |
| `ube-io` | Files and the chosen persistence primitives |
| `ube-http` | HTTP server and request/response behavior |
| `ube-std` | Optional package collection or distribution umbrella |

First establish a local package and native-linking path. Migrate a small package through it, then migrate the demonstration dependencies. Only then consider moving packages into another repository. A directory rename alone does not remove compiler coupling.

Remove domain-specific aliases, type-checker registrations, method-name special cases, and LLVM declarations as their packages migrate. Keep the native ABI general and versioned. Compiler bootstrap must not require linking financial or HTTP implementations; installing the compiler and linking an application runtime are separate concerns.

### Exit Gates

- One documented command builds the demo, and scripted requests verify its complete behavior after startup and restart.
- Tests cover actual POST bodies, JSON failures, exact arithmetic, and persistence failures, not only route registration or type checking.
- The core compiler has no domain-specific knowledge of `Money`, `Json`, or `Server`. The same names can be declared as ordinary user classes without triggering domain lowering.
- An application using only the core profile does not link optional financial, XML, HTTP, TLS, or QUIC implementations.
- Package loading and linking are tested from a clean checkout without a hosted registry.
- Binary size, throughput, latency, and memory growth are measured with toolchain, hardware, and workload recorded. A five-megabyte binary is an experiment to measure, not a release promise.
- The release is labeled as a demonstration until its documented correctness, durability, security, and operational requirements have been validated. Passing a happy-path demo alone does not establish production readiness.

## Phase 3 — Expand from Observed Needs

Prerequisite: the financial API is used and its outstanding operational issues are understood. Add one independently testable package capability at a time.

Candidates include HTTP/2, HTTP/3/QUIC, typed XML reconstruction and schema validation, foreign exchange, richer currency catalogs, date/time extensions, and broader task/channel combinators. None is a mandatory item for the first financial API release. TLS needed for a deployment belongs in that deployment's requirements; protocol expansion does not have to precede a correctly deployed HTTP/1.1 service.

For each addition require a concrete consumer, package ownership, compatibility policy, complete native/integration tests, and measured dependency and binary-size impact. An unavailable API must be absent from the stable surface or fail explicitly; it must not remain an advertised no-op.

Revisit a production tracing collector, advanced tooling, registry distribution, and self-hosting when usage and measurements justify their cost. Changes to core syntax require a separate proposal and migration plan.

## Immediate Next Work

1. Resolve the known trait test failure and establish a fully passing native baseline.
2. Complete nullable collection/combinator support and the remaining tagged task/channel ABI validation matrix.
3. Classify the README's API claims and experimental examples against executable behavior.
4. Inventory domain coupling in the compiler and runtime; draft the smallest local-package/native-ABI path that can remove it.
5. Define the exact-money representation and one minimal API demonstration before expanding any financial or server API.

Track progress with evidence attached to these gates, rather than by the number of advertised APIs.
