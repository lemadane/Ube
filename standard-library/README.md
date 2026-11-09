# Built-in library

Ube's built-in classes are registered by the compiler and implemented in the
Rust runtime. They do not require imports or Ube source stubs. The former `.tl`
prototypes and empty `Console` methods did not implement the current API and
have been removed.

```ube
// App.ube
class App {
    start() {
        Console.printLine('Hello from Ube')
    }
}
```

See the [language and built-in API reference](../README.md), the
[feature guides](../docs/README.md), and the [runnable examples](../examples).
Compiler signatures live in `compiler/src/builtins.rs` and
`compiler/src/semantics/type_checker.rs`; runtime implementations live in
`runtime/src`.
