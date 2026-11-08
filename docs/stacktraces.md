# Error stack traces

Thrown errors capture the active Ube call frames. Call `error.stacktrace()` inside a catch to get readable text:

```ube
class App {
    fail() {
        throw 'user not found'
    }

    start() {
        try {
            this.fail()
        } catch (Error error) {
            Console.printLine(error.stacktrace())
        }
    }
}
```

The result includes the error type and message, followed by frames from the throwing method outward:

```text
GenericError: user not found
  at App.fail (3:15)
  at App.start (8:13)
```

`stacktrace()` requires an error instance and no arguments. It works with built-in errors, concrete classes that satisfy `Error`, and error-compatible trait references. An error that has not been thrown has no captured trace and returns an empty string. Runtime failures also capture the active Ube methods. Unhandled errors print the captured frames automatically before exiting unsuccessfully.

The original throw trace is captured when an exception record is first created. Catching or rethrowing an error preserves that trace. Async tasks inherit their submission context, and awaiting a failed task adds the observer's active call frames. Boundaries are marked explicitly:

```text
GenericError: user not found
  at UserService.load (12:15)
  --- spawned at ---
  at Request.handle (24:9)
  --- spawned at ---
  at App.start (8:13)
  --- awaited at ---
  at Request.handle (25:9)
  --- spawned at ---
  at App.start (8:13)
  --- awaited at ---
  at App.start (9:13)
```

Spawn and await sections can repeat a caller when it both submits and awaits work; those sections describe different events. Await observations retain the same error object identity but store separate immutable diagnostic records. Repeated or concurrent awaiters do not change the source task's trace or one another's observation. A rethrow within a handler keeps that handler's observed trace.

Task contexts are installed only while their callbacks execute and are restored when they suspend, finish, or unwind. Coroutine locations survive suspension and worker migration. Both the coroutine backend and the blocking callback backend propagate submission contexts. Completed calls and unrelated worker tasks do not appear in later errors.

Trace formatting uses compiler/runtime metadata directly; it does not call `toString()` or serialize the error with `toJson()`.

## Current limits

- Frames show class/method names and line/column locations. Source filenames are not yet carried through the IR.
- Caller locations identify the active statement; the explicit throw frame identifies the throw expression.
- Submission ancestry retains the most recent 16 task boundaries. Await chains retain at most 32 observation boundaries.
- Each throw or await section formats at most 256 frames. Direct call stacks exceeding that budget have an omission marker.
- Native Rust/C library internals are not included.

Native regression tests cover nested calls, runtime errors, unhandled output, rethrows, nested task submissions/awaits, repeated and concurrent observers, suspension, and removal of completed frames:

```bash
cargo build -p ube-runtime
cargo test -p ube-compiler --test exceptions stacktrace
cargo test -p ube-compiler --test yielding_e2e
cargo test -p ube-runtime trace_tests
```
