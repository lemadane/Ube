# Concurrency in Ube

Tasks start immediately. Use `.await()` to obtain the result; failures use
ordinary `try`, `catch`, and `finally`.

```ube
final userTask = Async.start(
  () -> users.fetch(id))
final ordersTask = Async.start(
  () -> orders.fetch(id))
final user = userTask.await()
final orders = ordersTask.await()
```

## Yielding native methods: first supported slice

The compiler emits native resumable poll functions for eligible task bodies that
contain an await and use only known nonblocking IR operations. Channel waits,
task waits, supported combinators/deadlines, primitive arithmetic, local slots,
branches, loops, field accesses, and exception/cleanup control flow are covered.
The compiler selects this path automatically. A direct expression such as
`Async.start(() -> inbox.receive().await())` can yield. A call to an ordinary
method, or a block lambda invoked as a function, currently uses the blocking pool.
The legacy method-lowering tests and benchmark deliberately retain the old
`async` declaration syntax to measure that separate compiler path; it is not
the recommended application syntax.

A heap frame retains the resume position, registers, locals, pending exception,
and cancellation-delivery state. Pending task/channel waits register a Tokio
waker and return the worker to the scheduler. GC roots protect saved references
across suspension and worker migration, and frame roots are removed when the
frame is destroyed. Logging context is restored separately for each poll.
A 256-instruction budget yields long-running control flow cooperatively.

This is a conservative first slice, not a conversion of every task body.
Methods with ordinary method calls, trait dispatch, object construction, nested
async submissions, or unapproved runtime calls retain the blocking-pool backend.
This avoids running hidden blocking IO on scheduler workers. Interprocedural
blocking analysis, wider lowering coverage, and full async IO APIs remain work
for later stages. Synchronous entry-point code still blocks while awaiting.

Validation includes 2,048 suspended methods on one Tokio worker with two total
OS threads (main plus worker), multi-await loops, cancellation with awaited
finally cleanup, nullable results, channel-close handlers, frame-root lifetime,
logging context, and single-worker fairness. See the
[repeatable benchmark and measurements](../benchmarks/README.md). These bounded
checks do not establish parity with Rust or Java virtual threads.

## Task ownership and cleanup

Each task started with `Async.start` owns the tasks it starts. Tasks
started by synchronous helpers belong to that same parent task. Tasks started
from synchronous entry-point code belong to the program's root task scope. These are
task scopes, not a new scope at every brace or synchronous method call.

When a task body returns or throws, its unfinished children are cancelled and
joined before its result becomes available. App performs the same cleanup
before reporting an unhandled error or shutting down runtime resources. Await
work you want to complete; leaving it unawaited is not a way to launch detached
background work. A completed, unobserved child failure propagates at scope exit;
a failure already observed through an await is not raised again.

Cancellation flows to child tasks. Generated code observes cancellation at
basic-block boundaries and while awaiting. Cancellation is delivered once, so
subsequent awaits in `finally` can finish cleanup. Cleanup errors still follow
the normal exception rules. A cleanup failure can replace a cancellation or
timeout while retaining the earlier error as suppressed. An indivisible blocking operation cannot be forcibly
interrupted, and cleanup that never completes can still prevent termination.
There is no implicit detached-task API.

## Combining tasks and deadlines

| Operation | Behavior |
|---|---|
| `Async.start(operation)` | Starts one `Func() -> T` and returns `Async<T>`. |
| `Async.startAll([operations])` | Starts zero-argument functions and returns their values in input order. The argument is currently a list literal. |
| `Async.completed(value)` | Creates an already successful `Async<T>`. |
| `Async.failed(error)` | Creates an already failed `Async<void>` that preserves the supplied `Error`. |
| `Async.delay(ms)` | Completes after the delay; a negative delay fails. |
| `Async.all(tasks).await()` | Values in input order; first failure cancels remaining work. |
| `Async.any(tasks).await()` | First success; all failures produce an aggregate error. |
| `Async.race(tasks).await()` | First completion, whether successful or failed. |
| `Async.allSettled(tasks).await()` | An immutable `TaskOutcomes` sequence preserving each success or error. |
| `task.timeout(ms).await()` | On expiry, cancels the task and joins cleanup before reporting the timeout. |
| `task.cancel()` | Requests cooperative cancellation and reports whether this call initiated it. |
| `task.isCompleted()` | Reports whether a success or failure is already available. |
| `task.isCancelled()` | Reports whether cancellation has been requested or delivered. |

Combinators cancel and join remaining workers before publishing their result.
Cancelling or timing out a combinator also cancels and joins its inputs.
Consequently, a timeout limits the work's deadline, not a strict upper bound on
cleanup time. Empty `all` and `allSettled` inputs succeed; empty `any` and `race`
inputs fail. Literal non-task elements are diagnosed by the compiler. Untyped
lists are also validated at runtime, using registered opaque task IDs rather
than dereferencing their elements as pointers.

```ube
final results = Async.allSettled(
  [Async.start(() -> 0), Async.start(() -> false), Async.start(
    () -> 'hello')])
  .await()
print(
  results[0].succeeded())
print(
  results[0].value())
print(
  results[1].boolValue())
print(
  results[2].stringValue())
```

`TaskOutcomes` supports `len()`, `get(int)`, and `[int]`, with bounds checking.
Each `TaskOutcome` supports:

- `succeeded()`: whether the task succeeded.
- `value()`: an integer result as `long` (`int`, `long`, or `byte`).
- `boolValue()`, `decimalValue()`, `stringValue()`: checked scalar accessors.
- `error()`: the original `Error?`, or null on success.

Reading a failed outcome's value raises its original error, including its class
and fields. A mismatched accessor raises a catchable error. Success with zero or
false remains distinguishable from failure. These objects cannot be constructed
directly. Generic object-valued outcome access and typed nullable task lists are
not implemented; use ordinary typed awaits for those results. `Async.all` still
returns an untyped single-word list, and `any`/`race` currently have integer
result types in the language; use them with integer-valued tasks or use
`allSettled` and its checked scalar accessors.

## Safe sharing and channels

Built-in list and map operations synchronize their storage. Generated scalar
field reads and writes synchronize the payload and nullable presence together;
reference fields retain their tracing-heap barriers. Concurrent operations
therefore do not race on Rust collection storage or tear generated scalar
fields. This does not make a sequence of operations atomic: reading a counter
and then writing an increment can still lose updates. `final` prevents rebinding;
it does not freeze the referenced object.

Use `Channel<T>` to coordinate ownership or multi-step changes. Channels are
bounded (default capacity 64); `send` and `receive` return tasks. `close()` wakes
blocked operations and allows buffered values to drain. Sending to a closed
channel completes with false; receiving after drain raises a catchable error.
Sending an object reference does not copy or freeze it. Other mutable native
resources follow their own documented synchronization rules.

Compiler and runtime must be rebuilt together for the checked transport metadata
and outcome ABI. Native runtime ownership still includes shutdown-lived handles
and conservative escape pins; this change does not provide complete resource GC.

## Serialization in tasks

`.toJson()`, `.toXml()`, and `Xml.toObject()` are synchronous. Methods using serialization operations outside the yielding slice use the blocking backend; XML documents can still be returned across async calls. Native XML regressions cover async returns and catchable failures. See [XML](xml.md) and [testing](testing.md#serialization-end-to-end-regressions).

Serialization protects individual runtime reads and roots references, but does not freeze a mutable object graph. Coordinate concurrent writes when several fields or entries must describe the same logical instant.
