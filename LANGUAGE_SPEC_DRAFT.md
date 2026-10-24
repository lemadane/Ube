# Ube Language Specification Baseline (Draft v0.5.0)

```text
Name: Ube Programming Language
Compiler Command: ube
Source Extension: .ube
Tagline: Sharp, simple, native.
```

---

## 1. Language Identity & Design Goals

Ube is positioned between Go and Java:
* Native performance and simple single-binary deployment like Go.
* Familiar, readable, brace-delimited syntax like Java.
* Pure object-oriented programming model (everything is an instance method of a class).
* Built-in `print(...)` directive for output (always appends a newline, equivalent to Java `System.out.println` or Python `print()`).
* Implicit void return types: methods without an explicit return type default to void. The `void` keyword does not exist in Ube.
* Immutable by default (fields, parameters, local variables).
* Garbage-collected runtime with zero manual memory management boilerplate.
* Statically typed with structural generic constraints and flow-sensitive type narrowing.
* Direct native compilation through LLVM (no transpilation to Java, C, Go, or Rust).
* Lightweight task-based concurrency (long-term goal).

---

## 2. Built-in I/O Directive (`print`)

Ube provides `print(...)` as the sole built-in output directive. `print` **always appends a newline** upon execution (equivalent to Java's `System.out.println` or Python's `print()`).

```java
class Application {
    main() {
        final name = `Lem`
        print(`Hello, ${name}!`)
    }
}
```

---

## 3. Basic Data Types & Semantics

Ube provides exactly six primitive data types (the `void` keyword does not exist):

| Type Specifier | Native Representation | Description | Default Value |
| :--- | :--- | :--- | :--- |
| `string` | Immutable UTF-16 pointer + length | Textual string | `` (empty string) |
| `int` | Signed 32-bit integer (`i32`) | Standard integer | `0` |
| `decimal` | IEEE 754 64-bit float (`double`) | Double-precision floating point | `0.0` |
| `byte` | Unsigned 8-bit integer (`u8`) | Raw byte data | `0` |
| `long` | Signed 64-bit integer (`i64`) | Wide integer | `0` |
| `bool` | 1-bit boolean (`i1`) | Logical truth value | `false` |
| `char` (N/A) | *No distinct char type* | Represented as 1-character `string` | `` |

---

## 4. Unified Class Model & Implicit Void Methods

Every user-defined type in Ube is declared using the `class` keyword. Methods without an explicit return type are **implicitly void** (no `void` keyword).

```java
class User(
    long id,
    string name,
    var int age = 0
) {
    birthday() {
        this._age = this._age + 1
    }

    int getAge() {
        return this._age
    }
}
```

---

## 5. Structural `trait` Subtyping (No `implements` Keyword)

Abstraction and behavior reuse in Ube are defined using **`trait`**. Classes satisfy traits **implicitly** based on method signatures without needing an `implements` keyword:

```java
trait Printable {
    // Abstract method signature
    printDetails()

    // Default method implementation
    string getHeader() {
        return `=== PRINTABLE ITEM ===`
    }
}

// Class definitions — NO 'implements' keyword needed!
class User(long id, string name) {
    printDetails() {
        print(`User #${this._id}: ${this._name}`)
    }
}

class Invoice(long id, decimal total) {
    printDetails() {
        print(`Invoice #${this._id}: \$${this._total}`)
    }
}

class Application {
    render(Printable item) {
        print(item.getHeader())
        item.printDetails()
    }

    main() {
        final user = User(1000L, `Lem`)
        final invoice = Invoice(5001L, 199.99)

        this.render(user)
        this.render(invoice)
    }
}
```

---

## 6. Singletons via Application-Scoped Dependency Injection

Because Ube has no static fields or global variables, singletons are created once at application startup inside `Application.main()` and injected into dependent classes via primary constructors:

```java
class DatabaseConnection(string url) {
    query(string sql) {
        print(`Querying ${this._url}: ${sql}`)
    }
}

class UserService(DatabaseConnection db) {
    findUser(long id) {
        this._db.query(`SELECT * FROM users WHERE id = ${id}`)
    }
}

class Application {
    main() {
        // Created ONCE at startup — acts as an application-level singleton
        final db = DatabaseConnection(`postgres://localhost:5432`)

        final userService = UserService(db)
        userService.findUser(1000L)
    }
}
```

---

## 7. Concurrency (`async`, `await`, `Task<T>`, `Channel<T>`)

Ube concurrency follows one rule: **if it waits, you `await` it.** Anything that can take time — an `async` method call, an `async` expression, a channel `send`/`receive`, a combinator, or a timeout — immediately returns a `Task<T>`; `await` gets its value.

- There is exactly one way to start work: call an `async` method, or write `async expr`. `async` binds like `await` (to the operand right after it), so write `async (a + b)` to run a whole expression. `async expr` captures visible locals at submission and evaluates the expression on a worker. (`Task.async(...)` was removed.)
- Calls to methods declared `async` are submitted to the runtime's multithreaded worker pool. The receiver and arguments are captured at submission.
- `await` blocks the current Ube thread until its task completes; it is not a non-blocking suspension of the current method.

```java
class Calculator {
    async compute(int value) {
        return value * 2
    }
}

class Application {
    main() {
        final calculator = Calculator()
        final first = calculator.compute(10)
        final second = calculator.compute(20)
        final result = await Task.all([first, second])
        print(result[0] + result[1])
    }
}
```

`Task.all`, `Task.any`, `Task.race`, and `Task.allSettled` return tasks and coordinate based on completion. `all` preserves input order and fails fast; `any` returns the first successful result; `race` returns the first completed outcome; `allSettled` waits for every task. Empty `any`/`race` inputs fail.

Channels declare an element type and optional positive capacity. `send(value)` returns `Task<bool>` that completes once capacity is available — `true` when queued, `false` if the channel is closed. `receive()` returns `Task<T>`; after buffered values are drained from a closed channel it fails with `channel is closed`. `close()` wakes every waiting operation.

```java
final events = Channel<string>(16)
final accepted = await events.send(`ready`)
final event = await events.receive()
events.close()
```

**Timeouts.** `task.timeout(ms)` returns a new task that fails with `task timed out` (and cancels the original) if it does not finish in time. It works on any task, so channel timeouts need no extra API:

```java
guard {
    final message = await inbox.receive().timeout(1000)
    print(message)

    catch(err) {
        print(err.message())   // "task timed out"
    }
}
```

**Waiting on several sources.** There is no `select` statement; `await Task.race([a.receive(), b.receive()])` returns whichever completes first.

**Failures.** An awaited failure (a `throw` inside an async method, a timeout, a closed-channel receive) is routed to the enclosing `guard`/`catch`. Without a `guard`, inside an `async` method or `async` expression the failure propagates to whoever awaits that task; anywhere else the program stops with `Ube error: unhandled task failure: ...` and exit code 1. Failures are never silently turned into zero values.

**Limits.** Cancellation is cooperative: generated async code checks for cancellation at basic-block boundaries, so an indivisible operation already in progress (for example a blocking native call) runs to completion. Task and channel payloads are carried through a 64-bit slot; non-null primitive and pointer-sized channel payloads are supported and nullable scalar payloads are rejected. Arithmetic directly on a `Task` or `Channel` is a type error.

---

## 8. Composition Over Inheritance (No `extends` Keyword)

Ube has **no class inheritance** (`extends` is prohibited). Code reuse and polymorphism are achieved through **Primary Constructor Composition & Delegation**:

```java
class Engine(int horsepower) {
    start() {
        print(`Engine started (${this._horsepower} HP)`)
    }
}

class Logger(string prefix) {
    info(string message) {
        print(`[${this._prefix}] ${message}`)
    }
}

// Composition via Primary Constructor fields
class Car(string model, Engine engine, Logger logger) {
    startCar() {
        this._logger.info(`Starting vehicle ${this._model}...`)
        this._engine.start() // Delegation forwarding
    }
}

class Application {
    main() {
        final logger = Logger(`APP`)
        final engine = Engine(400)
        final car = Car(`Mustang`, engine, logger)
        car.startCar()
    }
}
```

---

## 9. All Enum Use Cases via Sum Classes

In Ube, enumerations and tagged algebraic variants are declared using **Sum Classes** (cases defined directly inside a `class`).

```java
// Use Case 1: Simple Unit Enumeration
class Direction {
    case North
    case South
    case East
    case West
}

// Use Case 2: Positional Payload Variant
class OrderStatus {
    case Pending
    case Approved(long orderId, decimal amount)
    case Rejected(string reason)
}

// Use Case 3: Named Field Payload Variant
class NetworkEvent {
    case Connected(string ip, int port)
    case Disconnected(int code)
    case DataReceived(string payload)
}

// Use Case 4: Enum Methods & Behavior
class LogLevel {
    case Debug
    case Info
    case Warning
    case Error

    bool isCritical() {
        return switch (this) {
            case Error -> true
            default -> false
        }
    }
}

class Application {
    main() {
        final status = OrderStatus.Approved(10042L, 199.99)

        // Pattern matching via switch (exhaustive compile-time check)
        final message = switch (status) {
            case Pending -> `Order is waiting for approval`
            case Approved(orderId, amount) -> `Order #${orderId} approved for \$${amount}!`
            case Rejected(reason) -> `Order rejected: ${reason}`
        }

        print(message)
    }
}
```

---

## 10. Unified `for` Loop & Control Flow (`break` & `continue`)

Ube uses `for` as its **sole loop keyword**. There is no `while` or `do-while` keyword in the language. `break` exits the loop immediately, while `continue` skips to the next iteration.

```java
class Application {
    main() {
        // 1. Infinite Loop (Replaces while(true))
        var counter = 0
        for {
            counter = counter + 1
            if (counter == 2) { continue }
            if (counter > 4) { break }
        }

        // 2. Conditional Condition Loop (Replaces while(condition))
        var remaining = 3
        for (remaining) {
            remaining = remaining - 1
        }

        // 3. Collection Element Iteration
        final names = [`Lem`, `Rhea`, `Alex`]
        for (name in names) {
            print(`Name: ${name}`)
        }

        // 4. Index & Item Destructuring Iteration
        for (index, name in names) {
            print(`${index}: ${name}`)
        }

        // 5. Map Key-Value Destructuring Iteration
        final userScores = [1000L: 95, 1001L: 88]
        for (id, score in userScores) {
            print(`ID ${id} -> Score ${score}`)
        }

        // 6. Numeric Range Iteration
        for (i in 0..5) {
            print(`Index: ${i}`)
        }
    }
}
```

---

## 11. All Pattern-Matching `switch` Use Cases

In Ube, `switch` is a first-class expression and statement without fall-through:

```java
class Application {
    main() {
        // 1. Constant Value Matching
        final statusText = switch (200) {
            case 200 -> `OK`
            default -> `Unknown`
        }

        // 2. Variant Matching with Payload Destructuring
        final orderMessage = switch (OrderStatus.Approved(10042L, 99.99)) {
            case Pending -> `Waiting`
            case Approved(id, amt) -> `Approved ID: ${id}`
            case Rejected(r) -> `Rejected: ${r}`
        }

        // 3. Type Pattern Matching ('is')
        final Object val = `Hello`
        final typeDesc = switch (val) {
            case is string -> `String`
            default -> `Other`
        }

        // 4. Guard Clauses ('if' guards)
        final grade = switch (85) {
            case s if (s >= 90) -> `Grade A`
            case s if (s >= 80) -> `Grade B`
            default -> `Grade F`
        }

        // 5. Multiple Value Or-Patterns
        final isWeekend = switch (`Saturday`) {
            case `Saturday`, `Sunday` -> true
            default -> false
        }

        // 6. Anonymous Object Structural Matching
        final accessLevel = switch ({ id: 1000, role: `Admin` }) {
            case { role: `Admin` } -> `Admin Access`
            default -> `Guest Access`
        }
    }
}
```

---

## 12. Explicit `this` Requirement

Inside any instance method of a class, every access to an instance field or instance method **must** be explicitly qualified with `this.`.

```java
class Account(long id, var decimal balance) {
    deposit(decimal amount) {
        this._balance = this._balance + amount
    }
}
```

---

## 13. Access Control (Naming-Based Visibility)

- **Unprefixed Identifier** (`Name`, `methodName`): **Public**.
- **Underscore-Prefixed Identifier** (`_Name`, `_methodName`): **Package-Private / Internal**.

---

## 14. Word-Based Boolean Operators & The `not` Keyword

Ube uses word-based logical operators: `not`, `and`, `nand`, `or`, `nor`, `xor`, `xnor`. Symbol `!` is prohibited.

Falsy values: `false`, `null`, `0`, `0.0`, `-0.0`, `` ``. All other values are truthy.

```java
class Application {
    main() {
        final bool isConnected = false
        final string input = ``
        final Object data = 42

        // 1. Logical negation of boolean
        if (not isConnected) {
            print(`System disconnected`)
        }

        // 2. Falsiness check
        if (not input) {
            print(`No input provided`)
        }

        // 3. Negated type check ('is not')
        if (data is not string) {
            print(`Data is not a text string`)
        }
    }
}
```
