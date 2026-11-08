# Function values

`Func(parameterTypes) -> returnType` describes a callable value. Parameter types and the result type are part of its signature:

```ube
Func(int) -> int
Func(int, string) -> bool
Func() -> void
Func() -> Async<int>
Func(int) -> Func(int) -> int
```

Function signatures must match exactly when assigning or returning a function. Calls check argument count and types, including the normal numeric conversions at the call site. `void` is allowed as a result, but not as a parameter. A function variable requires an explicit initializer.

Parentheses may group a function type in a local declaration, making the boundary before a long variable name easier to see. Both spellings are equivalent:

```ube
Func() -> int shortOperation = () -> 42
(Func() -> int) groupedOperation = () -> 42

(
    Func(int, string) -> Func(bool) -> int
) operationFactory = (int count, string label) -> (bool enabled) -> count
```

## Returning and calling functions

An expression closure uses typed parameters followed by `->` and its result expression. Its return type is inferred. Use `()` for no parameters.

```ube
class App {
    Func(int) -> int makeAdder(int amount) {
        return (int value) -> value + amount
    }

    int apply(Func(int) -> int operation, int value) {
        return operation(value)
    }

    start() {
        final addFive = this.makeAdder(5)
        Console.printLine(addFive(10))              // 15
        Console.printLine(this.apply(addFive, 20))  // 25
        Console.printLine(this.makeAdder(7)(3))     // 10

        Func(string) -> void output = (string text) -> Console.printLine(text)
        output('hello')
    }
}
```

Functions can be passed as arguments, stored in constructor fields, and returned by other functions. A function-valued constructor field follows the usual accessor rules: `box.operation()` retrieves the function, and `box.operation()(value)` calls it.

The supported general closure form has one expression. For a body with several statements, define a method and return a closure that calls it. Existing server handler blocks keep their syntax. Servers also accept values of type `Func(Request) -> Response`, including returned functions.

## Captures and lifetime

Creating a closure captures the current values of referenced local variables. Captured bindings and parameters are immutable inside the closure. Reassigning an outer variable afterward does not change the captured value:

```ube
var amount = 2
final read = () -> amount
amount = 99
Console.printLine(read()) // 2
```

Object captures retain the same object reference; they do not deep-copy the object. Mutations through that object's methods remain visible. Captured objects and other functions stay alive through garbage collection for as long as the closure needs them. Resource handles still require their normal explicit cleanup; keep a cache or file open until its callbacks finish.

Each invocation evaluates the expression again. Exceptions propagate to the caller's `try`/`catch`.

## Functions returning tasks

Start an ordinary method or expression on the task runtime by passing a zero-argument function to `Async.start`:

```ube
final firstTask = Async.start(() -> this.incrementMany(cache, 100))
final secondTask = Async.start(() -> this.incrementMany(cache, 100))
final firstResult = firstTask.await()
final secondResult = secondTask.await()
```

`Async.start(Func() -> T)` returns `Async<T>`. The function is called by the task runtime, so the expression inside it is not evaluated before the task starts. Ordinary method declarations do not need an `async` modifier.

`Func() -> Async<int>` is a function that creates or returns a task when called:

```ube
Func() -> Async<int> makeIncrement(Cache cache, int count) {
    return () -> this.incrementMany(cache, count)
}
```

Creating these function values does not call `incrementMany`:

```ube
final first = this.makeIncrement(cache, 100)
final second = this.makeIncrement(cache, 100)
first().await()
second().await()
```

Those two operations run sequentially. To start both before waiting:

```ube
final firstTask = first()
final secondTask = second()
firstTask.await()
secondTask.await()
```

An existing `Async<int>` is awaited directly: `firstTask.await()`. Tasks are not callable.

Run the complete example:

```bash
cargo run -p ube-compiler -- run examples/functions/App.ube
```

Expected output:

```text
15
100
100
200
```
