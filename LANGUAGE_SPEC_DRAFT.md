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
* Built-in `Log` for severity-filtered diagnostics on stderr and `Console` for plain stdout output.
* Implicit void return types: methods without an explicit return type default to void. The `void` keyword does not exist in Ube.
* Immutable by default (fields, parameters, local variables).
* No semicolons in code: statement terminators and separators using `;` are compile errors. Semicolons inside literals and comments are permitted. A return value must begin on the same line as `return`; a newline ends a bare return.
* Garbage-collected runtime with zero manual memory management boilerplate.
* Statically typed with structural generic constraints and flow-sensitive type narrowing.
* Direct native compilation through LLVM (no transpilation to Java, C, Go, or Rust).
* Lightweight task-based concurrency (long-term goal).

---

## 2. Built-in Logging and Console Output

`log(value)` is shorthand for `Log.info(value)`. `Log.debug(value)`,
`Log.warn(value)`, and `Log.error(value)` select the other severity levels.
Each emitted record goes to stderr as `[LEVEL] message` followed by a newline.
Error logging does not throw or terminate execution.

The process-wide threshold defaults to `info`, hiding debug messages.
`Log.setLevel(level)` accepts case-insensitive strings `debug`, `info`, `warn`,
`error`, and `off`. Invalid levels raise a catchable `Error` and leave the
previous threshold unchanged. Argument expressions are evaluated once, including
when a message is filtered out.

`Console.print(value)` writes plain stdout without adding a newline;
`Console.printLine(value)` adds one. Console output is independent of log levels.
Both Console methods and all logging methods require exactly one value.
Supported values are strings, numeric and boolean scalars, nullable scalars and
strings, and values satisfying `Error` (formatted using `message()`). NULL is
formatted as `null`. Arbitrary objects and collections must be formatted explicitly.
`print(value)` is shorthand for `Console.printLine(value)` and has the same argument rules.

```ube
class Application {
    main() {
        log('Application started')
        Log.warn('Retrying connection')
        Log.setLevel('debug')
        Log.debug('Detailed diagnostics')
        Log.error('Connection failed')
        Console.printLine('User-facing result')
    }
}
```

### Mapped Diagnostic Context

`Log.put(string key, string value)` adds or replaces a field in the current
logging context. `Log.get(string key)` returns `string?`, with `null` for absent
keys. `Log.remove(string key)` removes a field; `Log.clear()` removes all fields.
Keys and values must be non-null strings. Returned values are stable copies.

Nonempty context is included in every emitted log record as a JSON object
between the severity label and message. Keys are sorted; keys and values are
escaped, including quotes, backslashes, and control characters. Context does not
change Console output or filtering.

Context is local to the current execution thread. A newly created native Ube
task inherits an isolated copy at creation time, including nested tasks.
Worker threads restore their prior contexts on task completion, errors, or
cancellation. No context changes are merged back into a parent or sibling.
Operations and requests reusing an execution thread must clear their context
explicitly, normally in `finally`. `Log.setLevel` remains process-wide.
String equality compares contents, including strings obtained through `Log.get`;
NULL equals only NULL, not an empty string.

---

## 3. Basic Data Types & Semantics

Ube provides exactly six primitive data types (the `void` keyword does not exist):

| Type Specifier | Native Representation | Description | Default Value |
| :--- | :--- | :--- | :--- |
| `string` | Immutable UTF-8 text; current C ABI uses NUL-terminated bytes | Textual string | `''` (empty string) |
| `int` | Signed 32-bit integer (`i32`) | Standard integer | `0` |
| `decimal` | IEEE 754 64-bit float (`double`) | Double-precision floating point | `0.0` |
| `byte` | Unsigned 8-bit integer (`u8`) | Raw byte data | `0` |
| `long` | Signed 64-bit integer (`i64`) | Wide integer | `0` |
| `bool` | 1-bit boolean (`i1`) | Logical truth value | `false` |
| Character literal | No distinct runtime char type | Single quotes, such as `'A'`; represented as a one-character `string` | Not a declared type |

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

### Source-file organization

A physical `.ube` source file may contain at most one top-level type declaration:
either one `class` or one `trait`. Multiple classes, multiple traits, and mixed
class/trait declarations in one file are compiler errors. Imports and comments
are allowed alongside the declaration; import-only files are allowed. The filename
must match the declared class or trait name exactly, including case: `class Application` belongs in `Application.ube`.

Local imports use `import path.Type`, resolved relative to the importing file
as `path/Type.ube`. Dependencies are loaded transitively into a shared type
namespace, once per canonical file path, including circular imports. Local
import aliases and symbol lists are rejected. Missing files are errors. Each
imported file is independently checked against the one-type rule. Test-runner
entry points are generated internally and do not count as source declarations.
Documentation snippets grouping several types represent separate files.

### Project manifests and dependencies

`ube.toml` declares `[package]` metadata (`name`, `version`, and optional
`edition = "2026"`), optional `[build].entry` (default `src/Main.ube`), and
`[dependencies]`. The entry is a relative `.ube` path within the project.
`ube init` initializes the current directory; `ube new <name>` creates a new
project with `src/Main.ube` declaring `class Main`. Neither command overwrites an
existing manifest.

Dependency aliases are importable identifiers. Dependencies use either
`{ path = "../package" }` or `{ git = "<repository>", rev = "<revision>" }`, with
`rev` optional. `ube add <alias> --path <directory>` and
`ube add <alias> --git <repository> [--rev <revision>]` record and install them.
`ube remove <alias>` removes the dependency and its installed subtree.
`ube install` resolves direct and transitive packages and records exact Git
commits, versions, and source checksums in `ube.lock`; `--update` explicitly
refreshes locked sources. Version ranges and a package registry are not supported.

`import alias.services.Type` resolves to `src/services/Type.ube` in the installed
dependency, taking precedence over local imports with that prefix. Dependencies
resolve their own imports within their own package context. Installed sources
are checked against `ube.lock` before compilation. Imported types share one
compilation namespace. Cyclic package dependencies are rejected.

Source-less `check`, `emit-ir`, `emit-llvm`, `build`, and `run` use the nearest
ancestor manifest. Manifest builds write to `.build/`; `ube clean` removes only
Ube-managed build output and preserves dependency snapshots and lockfiles.

## 5. Structural `trait` Subtyping (No `implements` Keyword)

Abstraction and behavior reuse in Ube are defined using **`trait`**. Classes satisfy traits **implicitly** based on method signatures without needing an `implements` keyword:

```java
trait Printable {
    // Abstract method signature
    printDetails()

    // Default method implementation
    string getHeader() {
        return '=== PRINTABLE ITEM ==='
    }
}

// Class definitions — NO 'implements' keyword needed!
class User(long id, string name) {
    printDetails() {
        log('User #${this._id}: ${this._name}')
    }
}

class Invoice(long id, decimal total) {
    printDetails() {
        log('Invoice #${this._id}: \$${this._total}')
    }
}

class Application {
    render(Printable item) {
        log(item.getHeader())
        item.printDetails()
    }

    main() {
        final user = User(1000L, 'Lem')
        final invoice = Invoice(5001L, 199.99)

        this.render(user)
        this.render(invoice)
    }
}
```

---

## 6. Application-Scoped Sharing via Constructor Injection

Create shared objects explicitly inside `Application.main()` and inject them through primary constructors. Sharing is a lifetime convention, not an enforced singleton: other instances remain legal, and there is no singleton marker or registry.

```java
class DatabaseConnection(string url) {
    query(string sql) {
        log('Querying ${this._url}: ${sql}')
    }
}

class UserService(DatabaseConnection db) {
    findUser(long id) {
        this._db.query('SELECT * FROM users WHERE id = ${id}')
    }
}

class Application {
    main() {
        // Created explicitly and shared with dependent services
        final db = DatabaseConnection('postgres://localhost:5432')

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
        log(result[0] + result[1])
    }
}
```

`Task.all`, `Task.any`, `Task.race`, and `Task.allSettled` return tasks and coordinate based on completion. `all` preserves input order and fails fast; `any` returns the first successful result; `race` returns the first completed outcome; `allSettled` waits for every task. Empty `any`/`race` inputs fail.

Channels declare an element type and optional positive capacity. `send(value)` returns `Task<bool>` that completes once capacity is available — `true` when queued, `false` if the channel is closed. `receive()` returns `Task<T>`; after buffered values are drained from a closed channel it fails with `channel is closed`. `close()` wakes every waiting operation.

```java
final events = Channel<string>(16)
final accepted = await events.send('ready')
final event = await events.receive()
events.close()
```

**Timeouts.** `task.timeout(ms)` returns a new task that fails with `task timed out` (and cancels the original) if it does not finish in time. It works on any task, so channel timeouts need no extra API:

```java
try {
    final message = await inbox.receive().timeout(1000)
    log(message)
} catch (Error err) {
    log(err.message())   // "task timed out"
}
```

**Waiting on several sources.** There is no `select` statement; `await Task.race([a.receive(), b.receive()])` returns whichever completes first.

**Failures.** An awaited failure (a `throw` inside an async method, a timeout, a closed-channel receive) is routed to the enclosing `try`/`catch`. Without a matching `catch`, inside an `async` method or `async` expression the failure propagates to whoever awaits that task; anywhere else the program stops with `Ube error: unhandled exception ...` and exit code 1. Failures are never silently turned into zero values.

A worker receives its cancellation request once, whether at a cancellation poll or during `await`. After delivery, subsequent awaits, including `finally` cleanup, wait normally. This does not suppress failures or timeouts from the awaited operations.

**Limits.** Cancellation is cooperative: generated async code checks for cancellation at basic-block boundaries, so an indivisible operation already in progress (for example a blocking native call) runs to completion. Task and channel payloads use a two-word ABI value: a 64-bit payload and presence metadata. Nullable scalars (`int?`, `long?`, `byte?`, `bool?`, `decimal?`) retain null independently of zero/false through calls, fields, captures, awaited results, and channel operations. Errors and channel closure remain separate exceptions. Arithmetic directly on a `Task` or `Channel` is a type error. Untyped collections cannot store nullable scalars or tasks returning nullable scalars; these cases receive diagnostics, including nullable task-combinator inputs. Compiler and runtime must be rebuilt together.

---

## 8. Composition Over Inheritance (No `extends` Keyword)

Ube has **no class inheritance** (`extends` is prohibited). Code reuse and polymorphism are achieved through **Primary Constructor Composition & Delegation**:

```java
class Engine(int horsepower) {
    start() {
        log('Engine started (${this._horsepower} HP)')
    }
}

class Logger(string prefix) {
    info(string message) {
        log('[${this._prefix}] ${message}')
    }
}

// Composition via Primary Constructor fields
class Car(string model, Engine engine, Logger logger) {
    startCar() {
        this._logger.info('Starting vehicle ${this._model}...')
        this._engine.start() // Delegation forwarding
    }
}

class Application {
    main() {
        final logger = Logger('APP')
        final engine = Engine(400)
        final car = Car('Mustang', engine, logger)
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
            case Pending -> 'Order is waiting for approval'
            case Approved(orderId, amount) -> 'Order #${orderId} approved for \$${amount}!'
            case Rejected(reason) -> 'Order rejected: ${reason}'
        }

        log(message)
    }
}
```

---

## 10. Unified `for` Loop & Control Flow (`break` & `continue`)

Ube uses `for` as its **sole loop keyword**. There is no `while` or `do-while` keyword in the language. `break` exits the loop immediately, while `continue` skips to the next iteration.

**Loop `else` means zero body entries.** An optional `else` block runs only if the loop body was never entered. This includes an empty collection or a condition that is initially false. Entering the body sets this state before its first statement: an immediate `break` or `continue` therefore prevents the fallback. Normal completion after one or more iterations also skips it. A return or propagated error follows its normal control-flow path, not the fallback. A nested loop tracks its own body-entry state independently.

```ube
class Application {
    main() {
        for (item in []) {
            log(item)
        } else {
            log('empty') // Runs: zero iterations
        }

        for (item in [1]) {
            break
        } else {
            log('empty') // Does not run: the body was entered
        }
    }
}
```

Loop `else` does not test whether a search found a match. Use an explicit flag and a separate `if` for that purpose. See the [worked examples and output](README.md#zero-iteration-fallback).


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
        final names = ['Lem', 'Rhea', 'Alex']
        for (name in names) {
            log('Name: ${name}')
        }

        // 4. Index & Item Destructuring Iteration
        for (index, name in names) {
            log('${index}: ${name}')
        }

        // 5. Map Key-Value Destructuring Iteration
        final userScores = [1000L: 95, 1001L: 88]
        for (id, score in userScores) {
            log('ID ${id} -> Score ${score}')
        }

        // 6. Numeric Range Iteration
        for (i in 0..5) {
            log('Index: ${i}')
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
            case 200 -> 'OK'
            default -> 'Unknown'
        }

        // 2. Variant Matching with Payload Destructuring
        final orderMessage = switch (OrderStatus.Approved(10042L, 99.99)) {
            case Pending -> 'Waiting'
            case Approved(id, amt) -> 'Approved ID: ${id}'
            case Rejected(r) -> 'Rejected: ${r}'
        }

        // 3. Type Pattern Matching ('is')
        final Object val = 'Hello'
        final typeDesc = switch (val) {
            case is string -> 'String'
            default -> 'Other'
        }

        // 4. Guard Clauses ('if' guards)
        final grade = switch (85) {
            case s if (s >= 90) -> 'Grade A'
            case s if (s >= 80) -> 'Grade B'
            default -> 'Grade F'
        }

        // 5. Multiple Value Or-Patterns
        final isWeekend = switch ('Saturday') {
            case 'Saturday', 'Sunday' -> true
            default -> false
        }

        // 6. Anonymous Object Structural Matching
        final accessLevel = switch ({ id: 1000, role: 'Admin' }) {
            case { role: 'Admin' } -> 'Admin Access'
            default -> 'Guest Access'
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
- **Underscore-Prefixed Fields** (`_name`, `_age`): **Class-private**. Only code inside the declaring class may read or write these backing fields. Public field access requires calls: `user.name()` or `user.age()` for getters, and `user.age(value)` for a `var` setter. Immutable fields have no setter. Direct field reads (`user.age`) and assignments (`user.age = value`) are rejected. Accessors require an object instance.

---

## 14. Word-Based Boolean Operators & The `not` Keyword

Ube uses word-based logical operators: `not`, `and`, `or`. Symbol `!` is prohibited.

Falsy values: `false`, `null`, `0`, `0.0`, `-0.0`, `''`. All other values are truthy.

```java
class Application {
    main() {
        final bool isConnected = false
        final string input = ''
        final Object data = 42

        // 1. Logical negation of boolean
        if (not isConnected) {
            log('System disconnected')
        }

        // 2. Falsiness check
        if (not input) {
            log('No input provided')
        }

        // 3. Negated type check ('is not')
        if (data is not string) {
            log('Data is not a text string')
        }
    }
}
```

---

## Zero Initialization of Declarations

A typed local declaration without an initializer uses zero initialization, not an implicit constructor call. Numbers become `0`, booleans `false`, strings `''`, and nullable references `null`. A user-defined class receives a fresh object whose fields are recursively zero-initialized. Constructor default expressions are ignored, including defaults that allocate objects or call methods. Explicit construction (`User()`) still evaluates defaults.

The entry-point application instance uses the same zero-initialization path as local declarations. Its constructor defaults are not evaluated at startup. Configuration and resource initialization belong explicitly in `main()`.

Non-nullable class cycles are rejected during zero initialization; use nullable references to break them. Runtime-only types (including servers and channels) and traits require explicit initializers unless declared nullable. Zero initialization never invokes their creation APIs.

```ube
class User(string name = 'Guest') {}
class Application {
    main() {
        User zero                 // zero.name() is ''
        final configured = User() // configured.name() is 'Guest'
        User? absent              // null
    }
}
```

---

## 15. String Literals and Raw Text Blocks

Single quotes delimit one-line strings and character values: `'Hello'`, `'A'`, and `''`. Characters use the string representation; there is no separate `char` type. Escape sequences such as `\n`, `\t`, `\r`, `\`, and `\'` are interpreted, and existing `${...}` interpolation is supported. Unescaped physical line breaks are errors.

Backticks delimit raw text blocks. A block can be inline or span multiple physical lines. Every character between the delimiters is preserved, including opening/trailing newlines, indentation, CR/LF line endings, double quotes, and backslashes. Raw blocks do not decode escapes or perform interpolation. The next backtick closes the block unconditionally; backticks cannot be escaped inside it. An unterminated block is a lexical error.

Double quotes are reserved for JSON syntax, not standalone Ube literals. JSON payloads can be written inside raw text blocks and passed to `Json.toObject`; JSON escaping remains part of the payload and is interpreted by the JSON parser, not the Ube lexer. No dedicated JSON literal type is introduced by this rule.

```ube
final character = 'A'
final message = 'Hello\nWorld'
final json = `{"name":"Ube","active":true,"line":"\n"}`
final sql = `select id, name
from users
where active = true`
```

The SQL value contains two line breaks and no trailing newline. An opening line break, if present after the first backtick, would also be part of its value.
