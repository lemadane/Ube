# Timawa Language Specification Baseline (Draft v0.5.0)

```text
Name: Timawa Programming Language
Compiler Command: timawa
Source Extension: .timawa
Tagline: Sharp, simple, native.
```

---

## 1. Language Identity & Design Goals

Timawa is positioned between Go and Java:
* Native performance and simple single-binary deployment like Go.
* Familiar, readable, brace-delimited syntax like Java.
* Pure object-oriented programming model (everything is an instance method of a class).
* Built-in `print(...)` directive for output (always appends a newline, equivalent to Java `System.out.println` or Python `print()`).
* Implicit void return types: methods without an explicit return type default to void. The `void` keyword does not exist in Timawa.
* Immutable by default (fields, parameters, local variables).
* Garbage-collected runtime with zero manual memory management boilerplate.
* Statically typed with structural generic constraints and flow-sensitive type narrowing.
* Direct native compilation through LLVM (no transpilation to Java, C, Go, or Rust).
* Lightweight task-based concurrency (long-term goal).

---

## 2. Built-in I/O Directive (`print`)

Timawa provides `print(...)` as the sole built-in output directive. `print` **always appends a newline** upon execution (equivalent to Java's `System.out.println` or Python's `print()`).

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

Timawa provides exactly six primitive data types (the `void` keyword does not exist):

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

Every user-defined type in Timawa is declared using the `class` keyword. Methods without an explicit return type are **implicitly void** (no `void` keyword).

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

Abstraction and behavior reuse in Timawa are defined using **`trait`**. Classes satisfy traits **implicitly** based on method signatures without needing an `implements` keyword:

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

Because Timawa has no static fields or global variables, singletons are created once at application startup inside `Application.main()` and injected into dependent classes via primary constructors:

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

## 7. Concurrency (`async`, `await`, `Channel<T>`)

Concurrency in Timawa executes instance methods asynchronously using `async`, non-blocking returns via `await`, and channel messaging via `Channel<T>`:

```java
class UserRepository {
    User findUser(long id) {
        return User(id, `Lem`, 35)
    }
}

class OrderProcessor {
    processOrders(Channel<string> channel) {
        channel.send(`Order #10042 Processed`)
    }
}

class Application {
    main() {
        final repo = UserRepository()
        final processor = OrderProcessor()
        final channel = Channel<string>(10)

        // 1. Async task execution (returns Task<User>)
        final Task<User> userTask = async repo.findUser(1000L)

        // 2. Channel task execution
        async processor.processOrders(channel)

        // 3. Non-blocking await
        final user = await userTask
        print(`Loaded user: ${user.name()}`)

        final result = await channel.receive()
        print(`Channel result: ${result}`)
    }
}
```

---

## 8. Composition Over Inheritance (No `extends` Keyword)

Timawa has **no class inheritance** (`extends` is prohibited). Code reuse and polymorphism are achieved through **Primary Constructor Composition & Delegation**:

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

In Timawa, enumerations and tagged algebraic variants are declared using **Sum Classes** (cases defined directly inside a `class`).

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

Timawa uses `for` as its **sole loop keyword**. There is no `while` or `do-while` keyword in the language. `break` exits the loop immediately, while `continue` skips to the next iteration.

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

In Timawa, `switch` is a first-class expression and statement without fall-through:

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

Timawa uses word-based logical operators: `not`, `and`, `nand`, `or`, `nor`, `xor`, `xnor`. Symbol `!` is prohibited.

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
