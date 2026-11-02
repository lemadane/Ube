# Ube Programming Language

> **Enjoyable, simple, native.**

[![Language Specification](https://img.shields.io/badge/spec-v0.6.0-blue.svg)](LANGUAGE_SPEC_DRAFT.md)
[![Compiler Status](https://img.shields.io/badge/compiler-Rust%20%2B%20LLVM-orange.svg)](ARCHITECTURE.md)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

**Ube** (name taken from the delicious Pilippine purple yam root crop) is a NO INHERITANCE, NO INTERFACE, pure object-oriented composition programming language designed for native performance, low memory footprint, and high safety. Positioned between GO and JAVA, **Ube** combines native single-binary deployment and direct native code generation via LLVM like RUST.

---

## Language Core Principles & Policies

### 1. Pure Object-Oriented (No `static`, No Free Functions)
- **No Static Methods, Fields, or Classes**: Every executable function in Ube must be an instance method belonging to a class object.
 - **Application Entry Point**: The application entry point is an instance method (`main()`) on an `Application` class. The compiler automatically generates the native C-ABI wrapper.
- **Singletons via Dependency Injection**: Singletons are created once at application startup inside `Application.main()` and passed via primary constructors.

### 2. Zero Inheritance & Maximum Performance
- **No `extends` or Abstract Classes**: Ube has **no inheritance**.
- **0-Byte Vtable Overhead**: Because there is no inheritance, every method call is 100% statically known at compile time. There are no virtual method table (vtable) lookups.
- **Aggressive Inlining**: LLVM inlines instance method bodies directly into caller sites.
- **Cache Locality**: Class instance headers do not store virtual pointer (`vptr`) fields, resulting in smaller memory headers and superior CPU L1/L2/L3 cache hit rates.

### 3. Structural `trait` Subtyping (No `implements` Keyword)
- **Behavioral Contracts**: Abstraction and behavior reuse are declared using `trait`.
- **Implicit Structural Subtyping**: Classes satisfy traits **implicitly** based on method signatures (like Go and TypeScript, but powered by 0-cost LLVM static dispatch like Rust).
- **Default Method Logic**: Traits can supply default method implementations, enabling code reuse without class inheritance (`extends`).

### 4. Unified `class` Model & Enums via Sum Classes (No `enum` Keyword)
- **Single Type Keyword (`class`)**: `class` is the sole user-defined type keyword (no `enum`, `struct`, `record`, or `union`).
- **Primary Constructor Fields**: `class User(long id, string name, var int age)` automatically generates package-private backing fields (`_id`, `_name`, `_age`) and public getter methods (`id()`, `name()`, `age()`).
- **Enums via Sum Classes**: Enumerations and algebraic variants are declared using `case` clauses inside a `class` (e.g. `class Status { case Pending; case Approved(long orderId) }`).

### 5. No `void` Keyword & Implicit Void Return Types
- **No `void` Keyword**: The `void` keyword does not exist in Ube.
- **Implicit Void Return Type**: Methods declared without a return type automatically return void (`main() { ... }` or `birthday() { ... }`).
- **Explicit Return Types**: Methods returning values specify the return type before the method name (`int getAge() { return this._age }`).

### 6. Explicit `this.` Qualification
- All instance field and method accesses inside a class **must** be explicitly prefixed with `this.`. Unqualified access produces a compiler error.

### 7. Naming-Based Access Control
- Visibility is governed by identifier prefixes (`name` = public, `_name` = package-private/internal). There are no `public`, `private`, or `protected` keywords.

### 8. Built-in `print` Directive
- `print(...)` is the sole built-in output directive.
- **Always Appends Newline**: `print` always appends a newline (equivalent to Java's `System.out.println` or Python's `print()`).

### 9. Truthy & Falsy Conditions
- Conditions in `if (condition)` and `for (condition)` accept truthy and falsy values:
  - **Falsy**: `false`, `null`, `0`, `0.0`, `-0.0`, `` `` (empty string).
  - **Truthy**: `true`, non-zero numbers, non-empty strings, class instances, anonymous objects.
- **Flow-Sensitive Narrowing**: Evaluating a nullable `User?` in an `if (user)` block automatically narrows `user` to non-null `User`.

### 10. Word-Based Boolean Operators & `not` Keyword
- Logical operators use English words: `not`, `and`, `nand`, `or`, `nor`, `xor`, `xnor`. The `!` symbol is prohibited (replaced by `not`).

### 11. First-Class Nullability & Optional Operators
- Native `null` (never `nil`) with postfix `Type?` (`User?`).
- Optional chaining (`?.`), null coalescing (`??`), and null-coalescing assignment (`??=`).

### 12. Collections Policy & Unified `for` Loops (No `while` Keyword)
- **`for` is the Sole Loop Keyword**: There is no `while` or `do-while` keyword in Ube.
- **`for` / `else` (0-Iteration Fallback)**: `for (item in items) { ... } else { ... }`. The `else` block executes **ONLY if the loop body executed 0 times** (e.g. collection is empty `[]`, `null`, or condition is initially `false`).
- **`break` & `continue`**: `break` exits the nearest loop or guard block; `continue` skips to the next iteration.
- **`Array<T>`**: Fixed-size contiguous buffer (`Array<int>(10)`).
- **`List<T>`**: Dynamic growable sequence (`[1, 2, 3]` literal syntax, `.add()`, `.remove()`).
- **`Map<K, V>`**: Key-value hash map (`[1000L: `Lem`]` literal syntax, bracket lookup returns `V?`, key-value `for (k, v in map)` iteration).

### 13. Pattern Matching `switch` Expressions & Statements
- **First-Class Expressions**: `switch` can evaluate directly to a value (`final x = switch (...)`).
- **No Fall-Through**: Cases do not fall through (no `break` required inside cases).
- **Pattern Support**: Matches constant values, Sum Class variant payloads, type checks (`is`), guard clauses (`if`), multiple OR values, and anonymous objects.

### 14. Concurrency Runtime (Tokio)
- Async method calls are submitted to a shared multithreaded Tokio runtime. Receivers and arguments are captured before the call returns, and `await` blocks the calling Ube thread until completion.
- The Rust runtime provides typed `Task<T>` results/errors, completion-based combinators, timeouts, cancellation, and bounded closable channels.
- The compiler ABI transports task and channel values through a 64-bit slot, encoding primitive values and pointers. Ube await infers `Task<T>` from async method declarations; failures propagate into a surrounding `guard`/`catch`.
- One rule: **if it waits, you `await` it.** `async expr` (or an `async` method call) starts work; `await` gets the result.
- `Channel<T>(capacity)` is a typed bounded channel: `await ch.send(v)` (false once closed), `await ch.receive()`, and `ch.close()`. A closed-channel receive after queued values are drained is catchable.
- `await task.timeout(ms)` fails with a catchable `task timed out` and cancels the task; it works on any task, including `ch.receive()`.
- Unguarded failures propagate out of async code and otherwise stop the program with a clear error.

### 15. Exception Model (`guard { ... catch(err) { ... } exit { ... } }` & `throw`)
- **Unified Guard Scope**: `guard { ... catch(err) { ... } exit { ... } }` handles errors and resource cleanup in one single unified block.
- **`catch(err)` Clause**: Executes exclusively when an error is thrown inside `guard`.
- **`exit` Cleanup Clause**: Acts as the `finally`/cleanup block under `guard {}`, executing reliably on both success and error exit paths.
- **`return` Disallowance**: `return` inside a `guard` block is prohibited to guarantee predictable cleanup execution.
- **`throw` Keyword**: `throw("msg")` or `throw CustomError(...)` throws error objects or strings (auto-boxed to `GenericError`).

### 16. First-Class `if` Expressions & `for` Comprehensions (Mandatory `{}` & `yield`)
- **Expression Evaluation**: Both `if` and `for` can be evaluated directly as expressions (`final x = if (...) { ... }`).
- **Mandatory Braces `{}`**: ALL `if` expressions and `for` expressions require `{}` open and close braces.
- **Mandatory `yield` Keyword**: Emitting values from expression blocks requires `yield expr` (e.g. `final status = if (score >= 50) { yield `Passed` } else { yield `Failed` }`).
- **`for` Expression Comprehensions**: `final doubled = for (x in numbers) { yield x * 2 }` transforms collections into `List<T>`.

### 17. Lambdas & Method References
- **Brace Syntax**: `{ (x) -> x * 2 }`.
- **Compiler Lowering**: Lowered to synthetic, immutable single-method closure classes.
- **Method References**: `this.methodName` syntax for instance method callbacks.

### 18. Unified `DateTime` (`Instant` + Timezone) & `Duration` Standard Library
- **Single `DateTime` Class**: Eliminates Java's multi-class date/time API clutter (`Date`, `Time`, `LocalDate`, `LocalTime`, `LocalDateTime`, `ZonedDateTime`, `Instant`). One unified `DateTime` represents both the exact UTC Instant and its timezone display.
- **Instant Invariance**: `manila.in("Asia/Tokyo")` changes displayed clock time (`17:00` vs `18:00`) while preserving `manila.timestamp() == tokyo.timestamp()` (representing the exact same instant).
- **Unix Timestamps & Timezones**: `dt.timestamp()`, `dt.unix()`, `dt.unixMillis()`, `dt.unixNanos()`, `DateTime.fromUnix(sec)`, `DateTime.fromUnixMillis(ms)`.
- **Dedicated `Duration` Class**: `Duration.seconds(n)`, `Duration.hours(n)`, `Duration.days(n)` cleanly represents time amounts (`dt.addDuration(duration)`).

### 19. Financial Standard Library (`Money`, `Currency`, `Exchange`, `Percent`, `Rate`)
- **Immutability & Fixed-Point Precision**: `Money` values are immutable and backed by exact decimal arithmetic (never binary floating-point rounding errors).
- **Currency Safety**: `subtotal.add(shipping)` and `total.subtract(discount)` strictly enforce matching currencies. Mismatched currencies raise a runtime error unless explicitly converted via `Exchange.convert(dollar, Currency.PHP, rate)`.
- **Standard Currencies**: `Currency.PHP`, `Currency.USD`, `Currency.EUR`, `Currency.JPY`, `Currency.GBP`.
- **Financial Percentages & Rates**: `Percent(12)` and `Rate.percent(5.5)` for tax, interest, and discount calculations (`price.multiply(vat)`).

### 20. Native Filesystem Standard Library (`File`, `FileInfo`, `Directory`, `Path`, `after`)
- **Exception-Free File Operations**: High-performance IO without exceptions (`FileError`, `FileNotFoundError`, `PermissionError`, `AlreadyExistsError`, `InvalidPathError`, `DiskFullError`).
- **Object-Oriented & Static Helpers**: `File.read("app.txt")`, `File.write("out.txt", data)`, `file = File.open(path)`, `file.read()`, `file.close()`.
- **Deferred Cleanup**: Native deferred cleanup using `after file.close()` syntax.
- **Directories & Paths**: `Directory.create("logs")`, `Path.join("src", "main.Ube")`.

### 21. Web Server Standard Library (`server`, `request`, `response`)
- **Lambda and route syntax**: Inline handler arguments are supported, for example `server.get("/", request -> response.ok("hello"));`. The original trailing-lambda form remains supported.
- **Lowercase built-in type aliases**: Built-in class types accept lowercase names such as `server`, `request`, and `response`; existing PascalCase names such as `Server` and `Response` remain valid.
- **Express / Ktor Ergonomics**: Fluent routing with `server.get()`, `server.post()`, `server.put()`, `server.delete()`, `server.use()` (middleware), `server.static()`, `server.websocket()`, and `server.start(8080)`.
- **Multi-Protocol Support**: Default `HTTP/1.1` TCP transport, `server.enableHttp2()` for TLS ALPN multiplexing, and `server.enableHttp3(443)` for native UDP QUIC 0-RTT transport.
- **Request & Response Ergonomics**: `request.pathLong("id")`, `request.query("search")`, `request.hasBearerToken()`, `request.cookie("session")`, `request.protocol()`, `request.isHttp3()`, `Response.ok("hello")`, `Response.json(data)`.
- **Current runtime status**: `server.start(port)` currently runs a blocking, loopback-only HTTP/1.1 server with exact method/path route matching. TLS, HTTP/2/3, request-body parsing, middleware, static-file serving, and WebSockets are not implemented yet; several advanced `Request` accessors remain placeholders.

### 22. First-Class Native JSON & XML Record Mapping (`Json`, `JsonObject`, `JsonArray`, `Xml`, `XmlDocument`, `XmlElement`)
- **Bi-Directional Document & Class Mapping**: `Json.parse(text)`, `Json.partialParse(text, TargetClass)`, `Json.stringify(obj)`, `Json.from(obj)`, `Json.to(text, TargetClass)`.
- **XML Document Traversal & Generation**: `Xml.parse(text)`, `Xml.create("root")`, `doc.root().child("tag").text()`, `doc.toString()`.
- **Reversible Character Codec (`DataNameCodec`)**: Maps XML/JSON tag/key names with hyphens (`Facility-ID`), namespaces (`rest:KitchenOrderScan`), or dots to Ube identifier conventions (`Facility$hyphen$ID`, `rest$KitchenOrderScan`).
- **Strict vs. Partial Parsing Modes**: Strict mode enforces 1-to-1 structural completeness; Partial mode selectively maps declared components while assigning defaults (`null`, `List.of()`, `0`, `false`).
- **Structured Format Error Handling**: Exception-free error handling via `guard`/`catch(err)` with `JsonParseError`, `XmlParseError`, `MissingDataError`, `ExcessDataError`.

### 23. String Literals
- Ordinary strings use `"text"`; a double quote followed only by optional spaces/tabs and a physical line break begins a multi-line string, closed by the next unescaped `"`.
- Multi-line strings normalize LF, CRLF, and CR to `\n`, remove common incidental indentation (including the closing delimiter's indentation), and retain the newline before a closing delimiter on its own line. Put the closing quote after the last content character to omit that final newline.
- Escape sequences include `\n`, `\t`, `\r`, `\b`, `\f`, `\\`, `\"`, `\'`, `\s`, octal escapes, and `\u` followed by four hexadecimal digits. A backslash directly before a physical line break joins the next line without a newline.
- Braces are literal; multi-line strings do not interpolate.

```ube
final json = "
    {
        \"name\": \"Ube\",
        \"active\": true
    }
    "

final sql = "
    select id, name
    from users
    where active = true
    "

final html = "
    <article>
        <h1>Welcome</h1>
    </article>
    "

final ordinary = "Hello\nWorld"
```

---

## VS Code Extension & Syntax Highlighting

Ube includes an official VS Code extension in [`editors/vscode`](editors/vscode) providing `.ube` file recognition, syntax highlighting, bracket matching, string interpolation highlighting, code folding, and document formatting. Use **Format Document** (Shift+Alt+F on Linux/Windows, Shift+Option+F on macOS) to apply the initial two-space indentation and multiline call-argument rules in [`FORMATTER.md`](editors/vscode/FORMATTER.md).

### Installation Options:

#### Option 1: Quick Install (Copy Extension Directory)
```bash
# Linux / macOS
cp -r editors/vscode ~/.vscode/extensions/ube-lang.ube-vscode-0.6.0

# Windows (Command Prompt)
xcopy /E /I editors\vscode %USERPROFILE%\.vscode\extensions\ube-lang.ube-vscode-0.6.0
```

#### Option 2: Build & Install `.vsix` Package
```bash
cd editors/vscode
npx vsce package
code --install-extension ube-vscode-0.6.0.vsix
```

---

## Code Examples

### 1. Unified Classes & Methods

```java
class User(
    long id,
    string name,
    var int age = 0
) {
    // Implicit void return type (no 'void' keyword)
    birthday() {
        this._age = this._age + 1
    }

    // Explicit return type
    int getAge() {
        return this._age
    }
}
```

### 2. Structural `trait` Subtyping (No `implements` Keyword)

In Ube, `trait` provides behavioral contracts and default method bodies. Classes satisfy traits **implicitly** without an `implements` keyword:

```java
trait Printable {
    // Abstract method signature (implicit void return type)
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

        // Both User and Invoice implicitly satisfy Printable
        this.render(user)
        this.render(invoice)
    }
}
```

### 3. The `not` Keyword Use Cases

In Ube, the exclamation symbol `!` is not a valid operator. Logical negation and type checks use the `not` keyword:

```java
class Application {
    main() {
        final bool isConnected = false
        final string input = `` // Empty string (falsy)
        final Object data = 42

        // 1. Logical negation of boolean values
        if (not isConnected) {
            print(`System is disconnected`) // Executes because not false is true
        }

        // 2. Truthiness negation (falsy check)
        if (not input) {
            print(`No input provided`) // Executes because empty string is falsy
        }

        // 3. Negated type check ('is not')
        if (data is not string) {
            print(`Data is not a text string`) // Executes because data is an int
        }
    }
}
```

### 4. Composition Over Inheritance Use Cases

Because Ube has **no `extends` keyword**, code reuse and polymorphism are achieved cleanly through **Composition & Delegation**:

```java
// Component Class 1: Engine
class Engine(int horsepower) {
    start() {
        print(`Engine started (${this._horsepower} HP)`)
    }
}

// Component Class 2: Logger
class Logger(string prefix) {
    info(string message) {
        print(`[${this._prefix}] ${message}`)
    }
}

// Use Case 1: Component Embedding & Delegation (Replaces class Inheritance)
class Car(string model, Engine engine, Logger logger) {
    startCar() {
        this._logger.info(`Starting vehicle ${this._model}...`)
        this._engine.start() // Explicit forwarding delegation
    }
}

// Use Case 2: Behavior Injection via Primary Constructor (Strategy Pattern)
class DataExporter(Logger logger) {
    exportData(string data) {
        this._logger.info(`Exporting data: ${data}`)
    }
}

class Application {
    main() {
        // Instantiate reusable components
        final logger = Logger(`APP`)
        final v8Engine = Engine(450)

        // Compose components into higher-level objects
        final mustang = Car(`Mustang GT`, v8Engine, logger)
        mustang.startCar()

        final exporter = DataExporter(logger)
        exporter.exportData(`User Record #1000`)
    }
}
```

### 5. Singleton Implementation via Application-Scoped Dependency Injection

Because Ube omits global static variables, singletons are created once at application startup and passed into dependent services:

```java
class DatabaseConnection(string connectionUrl) {
    query(string sql) {
        print(`Executing query on ${this._connectionUrl}: ${sql}`)
    }
}

class UserService(DatabaseConnection db) {
    findUser(long id) {
        this._db.query(`SELECT * FROM users WHERE id = ${id}`)
    }
}

class OrderService(DatabaseConnection db) {
    findOrders(long userId) {
        this._db.query(`SELECT * FROM orders WHERE user_id = ${userId}`)
    }
}

class Application {
    main() {
        // Created ONCE at startup — acts as an application-level singleton
        final dbSingleton = DatabaseConnection(`postgres://localhost:5432/main`)

        // Injected into dependent service instances
        final userService = UserService(dbSingleton)
        final orderService = OrderService(dbSingleton)

        userService.findUser(1000L)
        orderService.findOrders(1000L)
    }
}
```

### 6. All Enum Use Cases via Sum Classes

In Ube, enumerations are declared using **Sum Classes** (defining `case` clauses inside a `class`):

```java
// Use Case 1: Simple Unit Enumerations (No Payloads)
class Direction {
    case North
    case South
    case East
    case West
}

// Use Case 2: Positional Payload Variants (Algebraic Data Types)
class OrderStatus {
    case Pending
    case Approved(long orderId, decimal amount)
    case Rejected(string reason)
}

// Use Case 3: Named Field Variant Payloads
class NetworkEvent {
    case Connected(string ip, int port)
    case Disconnected(int code)
    case DataReceived(string payload)
}

// Use Case 4: Enums with Instance Methods & Behavior
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
        // 1. Simple Unit Enum Usage
        final dir = Direction.North
        final dirName = switch (dir) {
            case North -> `Heading North`
            case South -> `Heading South`
            case East  -> `Heading East`
            case West  -> `Heading West`
        }
        print(dirName)

        // 2. Destructuring Payload Variant
        final status = OrderStatus.Approved(10042L, 250.00)
        final statusText = switch (status) {
            case Pending -> `Order Pending`
            case Approved(id, amount) -> `Order #${id} Approved for \$${amount}`
            case Rejected(reason) -> `Order Rejected: ${reason}`
        }
        print(statusText)

        // 3. Enum Method Call
        final level = LogLevel.Error
        if (level.isCritical()) {
            print(`Critical log level detected!`)
        }
    }
}
```

### 7. All `switch` Control Flow Use Cases

In Ube, `switch` is a first-class expression and statement supporting 6 core pattern-matching use cases (no fall-through, no `break` required inside cases):

```java
class Application {
    main() {
        // Case 1: Constant Value Matching
        final statusCode = 200
        final statusText = switch (statusCode) {
            case 200 -> `OK`
            case 404 -> `Not Found`
            case 500 -> `Internal Error`
            default -> `Unknown Code`
        }
        print(statusText)

        // Case 2: Variant Matching with Payload Destructuring
        final status = OrderStatus.Approved(10042L, 99.99)
        final orderMessage = switch (status) {
            case Pending -> `Waiting`
            case Approved(id, amt) -> `Approved ID: ${id}`
            case Rejected(r) -> `Rejected: ${r}`
        }
        print(orderMessage)

        // Case 3: Type Pattern Matching ('is')
        final Object val = `Hello World`
        final typeDesc = switch (val) {
            case is string -> `Text String`
            case is int -> `32-bit Integer`
            default -> `Other Type`
        }
        print(typeDesc)

        // Case 4: Guard Clauses ('if' guards)
        final score = 85
        final grade = switch (score) {
            case s if (s >= 90) -> `Grade A`
            case s if (s >= 80) -> `Grade B`
            case s if (s >= 70) -> `Grade C`
            default -> `Grade F`
        }
        print(grade)

        // Case 5: Multiple Value Or-Patterns (Comma-separated)
        final day = `Saturday`
        final isWeekend = switch (day) {
            case `Saturday`, `Sunday` -> true
            default -> false
        }
        print(`Is Weekend: ${isWeekend}`)

        // Case 6: Anonymous Object Structural Matching
        final userObj = { id: 1000, role: `Admin` }
        final accessLevel = switch (userObj) {
            case { role: `Admin` } -> `Full Administrative Access`
            case { id: uid } if (uid > 0) -> `Standard User Access`
            default -> `Guest Access`
        }
        print(accessLevel)
    }
}
```

### 8. All `for` Loop Use Cases (With `break` and `continue`)

Ube uses `for` as its **sole loop keyword** (no `while` or `do-while`).

```java
class Application {
    main() {
        // Case 1: Infinite Loop (Replaces while(true))
        var counter = 0
        for {
            counter = counter + 1
            if (counter == 2) {
                continue // Skip iteration
            }
            if (counter > 4) {
                break // Exit loop
            }
            print(`Infinite loop step: ${counter}`)
        }

        // Case 2: Conditional Condition Loop (Replaces while(condition))
        var remaining = 3
        for (remaining) {
            print(`Remaining: ${remaining}`)
            remaining = remaining - 1
        }

        // Case 3: Element Iteration Over List/Array
        final names = [`Lem`, `Rhea`, `Alex`]
        for (name in names) {
            if (name == `Rhea`) {
                continue
            }
            print(`Name: ${name}`)
        }

        // Case 4: Index & Item Destructuring Iteration
        for (index, name in names) {
            print(`${index}: ${name}`)
        }

        // Case 5: Map Key-Value Destructuring Iteration
        final scores = [1000L: 95, 1001L: 88]
        for (id, score in scores) {
            print(`ID ${id} -> Score ${score}`)
        }

        // Case 6: Numeric Range Iteration
        for (i in 0..5) {
            if (i == 3) {
                break
            }
            print(`Range index: ${i}`)
        }

        // Case 7: for / else 0-Iteration Fallback Clause
        final List<string> emptyItems = []
        for (item in emptyItems) {
            print(`Item: ${item}`)
        } else {
            print(`No items found (else executed for 0 iterations)`)
        }
    }
}
```

### 9. Truthy & Falsy Control Flow with Word Booleans

In Ube, conditions in `if (condition)` statements evaluate **truthiness and falsiness** without requiring explicit boolean comparisons:

```java
class Application {
    main() {
        final User? optionalUser = User(1000, `Lem`, 35)
        final int attempts = 3
        final string input = `` // Empty string (falsy)

        // 1. Truthy object evaluation & flow-sensitive narrowing
        if (optionalUser) {
            // Inside this block, 'optionalUser' is automatically narrowed from User? to non-null User
            print(`User loaded: ${optionalUser.name()}`)
        }

        // 2. Non-zero number (truthy) & empty string (falsy) using word booleans
        if (attempts and not input) {
            print(`System ready: ${attempts} attempts remaining and no pending input.`)
        }
    }
}
```

### 10. Word-Based Logical Operators (`not`, `and`, `nand`, `or`, `nor`, `xor`, `xnor`)

Ube uses English words for all boolean operators instead of symbols like `!`, `&&`, `||`:

```java
class Application {
    main() {
        final bool isReady = true
        final bool isConnected = false

        // 1. NOT operator
        if (not isConnected) {
            print(`System is currently disconnected`)
        }

        // 2. AND & NAND operators
        final bool bothActive = isReady and isConnected     // false
        final bool notBothActive = isReady nand isConnected // true

        // 3. OR & NOR operators
        final bool eitherActive = isReady or isConnected    // true
        final bool neitherActive = isReady nor isConnected   // false

        // 4. XOR (Exclusive OR) & XNOR (Equivalence) operators
        final bool exclusiveOne = isReady xor isConnected   // true (exactly one is true)
        final bool bothMatch = isReady xnor isConnected     // false (they differ)

        print(`bothActive: ${bothActive}`)
        print(`eitherActive: ${eitherActive}`)
        print(`exclusiveOne: ${exclusiveOne}`)
    }
}
```

### 11. Concurrency Runtime and Current Language Boundary

The runtime uses a shared multithreaded Tokio executor. Async Ube method calls are converted to generated callback thunks that capture the receiver and arguments and execute the method on Tokio's blocking pool. `await` blocks a synchronous Ube caller; the Rust API's `join` is available to async Rust callers. `Task<T>` carries `Result<T, TaskError>` rather than turning failures into successful values. `T` must be cloneable and thread-safe because task results can be observed by multiple waiters.

```rust
use std::time::Duration;
use ube_runtime::concurrency::{spawn_task, Channel, Task, TaskError};

let first = spawn_task(|_| async {
    tokio::time::sleep(Duration::from_millis(20)).await;
    Ok::<_, TaskError>(10)
});
let second = spawn_task(|_| async { Ok::<_, TaskError>(20) });

let values = Task::all(vec![first, second]).await?;
assert_eq!(values, vec![10, 20]);

let channel = Channel::bounded(8)?;
channel.send(42).await?;
assert_eq!(channel.receive().await?, Some(42));
channel.close();
assert_eq!(channel.receive().await?, None);
```

- `Task::all` preserves input order, fails on the first error, and requests cancellation of remaining tasks.
- `Task::any` returns the first successful result and aggregates failures if all tasks fail. `Task::race` returns the first completed outcome, including an error. Both request cancellation of the other tasks.
- `Task::all_settled` returns every outcome in input order. Empty `all` and `all_settled` inputs succeed with empty output; empty `any` and `race` return `TaskError::NoTasks`.
- `await_timeout` returns `TaskError::Timeout` and requests cancellation. Dropping an async future cancels it at await points. A generated Ube method callback already running on the blocking pool cannot be forcibly stopped; its task is marked cancelled but its synchronous body may continue.
- `Channel<T>` is bounded (default capacity 64). Sends wait for capacity, receive returns `None` after close and buffered values are drained, and close wakes blocked senders/receivers with a closed outcome.

The compiler now schedules calls to declared `async` methods and infers the awaited result type. Async method results are transported through a 64-bit ABI slot (integer/boolean/decimal bit representation or pointer); compiler-generated callbacks own their capture context, and task handles are cleaned up with the runtime. `Task.all`, `Task.any`, `Task.race`, and `Task.allSettled` also return tasks and observe completion order.

The compiler transports async method throws and runtime task failures to an enclosing `guard`/`catch`; use `err.message()` to inspect the message. Awaiting a failure without a surrounding `guard` propagates it from an async method or `async` expression to its awaiter; anywhere else the program stops with `Ube error: unhandled task failure: ...` (exit code 1). Generated async methods poll cancellation at basic-block boundaries and return early when cancelled; a currently executing indivisible operation cannot be interrupted. `async expr` captures visible locals when submitted and evaluates the expression on a worker. Language channels support explicit element types (`Channel<T>(capacity)`) and `close()`; `send(value)` returns `Task<bool>` and `receive()` returns `Task<T>`, with the channel wait performed on Tokio. `send` completes with false if the channel is closed; after buffered values are drained, `receive` fails with a catchable channel-closed error. `task.timeout(ms)` wraps any task (including a channel operation) with a deadline; `Task.race` waits on several channels at once. The language channel bridge transports non-null primitive values and pointer-sized values through the existing 64-bit ABI; nullable scalar payloads are rejected.

### 12. Built-in Collections (`List<T>`, `Map<K, V>`, Indexing & Bounds Protection)

```java
class Application {
    main() {
        // 1. List literal, indexing, and methods
        final numbers = [10, 20, 30]
        numbers.add(40)
        print(`List len: ${numbers.len()}, first element: ${numbers[0]}`)

        // 2. Map literal, key lookup, and methods
        final scores = [1000L: 95, 1001L: 88]
        print(`Map len: ${scores.len()}, score for 1000L: ${scores[1000L]}`)

        // 3. Out-of-bounds error handling
        guard {
            print(numbers[10]) // Out of bounds!

            catch(err) {
                print(`Caught error: ${err.message()}`)
            }
        }
    }
}
```

### 13. Structured Exception Model (`guard { ... catch(err) { ... } exit { ... } }` & `throw`)

```java
class Application {
    main() {
        guard {
            // Auto-boxed string throw into GenericError
            throw(`Invalid user request`)

            catch(err) {
                print(`Caught error message: ${err.message()}`)
            }

            exit {
                print(`Guard exit cleanup executed`)
            }
        }
    }
}
```

### 14. Unified `DateTime` (`Instant` + Timezone) & `Duration` Standard Library

```java
class Application {
    main() {
        // 1. Instant + Timezone Creation
        final manila = DateTime.of(2026, 8, 31, 17, 0, `Asia/Manila`)
        final tokyo = manila.in(`Asia/Tokyo`)

        // 2. Instant Invariance (Timestamps match 100%, displayed clock hours differ)
        print(`Timestamps equal: ${manila.timestamp() == tokyo.timestamp()}`) // true!
        print(`Manila hour: ${manila.hour()}, Tokyo hour: ${tokyo.hour()}`)     // 17 vs 18

        // 3. Unix Timestamps & Formatting
        final dt = DateTime.fromUnix(1788159600)
        print(`Formatted: ${dt.format(`yyyy-MM-dd HH:mm:ss`)}`)

        // 4. Time Amounts with Duration
        final start = DateTime.now()
        final timeout = Duration.seconds(30)
        final expires = start.addDuration(timeout)
        print(`Expires in: ${expires.diffSeconds(start)} seconds`)
    }
}
```

### 15. Financial Standard Library (`Money`, `Currency`, `Exchange`, `Percent`, `Rate`)

```java
class Application {
    main() {
        // 1. Immutable Financial Money Creation
        final price = Money.php(1250.75)
        final dollar = Money.usd(100.00)
        print(`Price amount: ${price.amount()}, Currency: ${price.currency().code()}`)

        // 2. Currency-Safe Arithmetic
        final shipping = Money.php(100.00)
        final total = price.add(shipping)
        print(`Total PHP amount: ${total.amount()}`)

        // 3. Explicit Currency Conversion via Exchange
        final converted = Exchange.convert(dollar, Currency.PHP, 58.50)
        print(`100 USD converted to PHP: ${converted.amount()}`)

        // 4. Financial Percentages & Taxes
        final vat = Percent(12)
        final tax = total.multiply(vat)
        print(`VAT Tax amount: ${tax.amount()}`)
    }
}
```

### 16. Native Filesystem & Deferred `after` Cleanup

```java
class Application {
    main() {
        // 1. One-shot static conveniences
        File.write(`logs/app.txt`, `Server initialized\n`)
        final content = File.read(`logs/app.txt`)
        print(`Log content: ${content}`)

        // 2. Object-oriented handle & deferred cleanup
        guard {
            final file = File.open(`data.csv`)
            after file.close() // Native deferred cleanup execution

            final data = file.read()
            print(`Read ${data.len()} bytes`)

            catch(err) {
                print(`File error: ${err.message()}`)
            }
        }
    }
}
```

### 17. High-Performance Native Web Server (`HTTP/1.1`, `HTTP/2`, `HTTP/3 QUIC`)

```java
class Application {
    main() {
        final server = server()

        server.get("/", request -> response.ok("Hello from Ube!"));

        server.start(8080)
    }
}
```

### 18. First-Class Native JSON & XML Document and Record Mapping

```java
class KitchenOrderScan(
    string RestaurantID,
    string OrderType
)

class Application {
    main() {
        // 1. JSON Document Parsing & Getters
        final jsonText = `{"restaurantID": "0603", "orderType": "DineIn"}`
        final json = Json.parse(jsonText)
        print(`Restaurant ID: ${json.getString(\`restaurantID\`)}`)

        // 2. Strongly-Typed Record Mapping & Stringification
        final scan = Json.partialParse(jsonText, KitchenOrderScan)
        final formattedJson = Json.stringify(scan)
        print(`JSON Stringified: ${formattedJson}`)

        // 3. XML Traversal & Generation
        final doc = Xml.create(`user`)
        doc.root().element(`name`).text(`Lem`)
        doc.root().element(`age`).text(`40`)
        print(`Generated XML:\n${doc.toString()}`)
    }
}
```

### 19. Compile-Time Null Safety & Variable Initializers

```java
class User(string name = `Guest`)

class Application {
    main() {
        // 1. Non-nullable type declaration automatically invokes default constructor User()
        User u1          // Automatically calls User() constructor
        User u2 = User() // Explicitly calls User() constructor

        // 2. Nullable type declaration requires '?' and defaults to null
        User? u3        // Defaults to null
        User? u4 = null // Explicitly assigned null

        // 3. Optional chaining & null coalescing
        final displayName = u3?.name() ?? `Anonymous`
        print(`Display Name: ${displayName}`)
    }
}
```

---

## Quick Start (Ubuntu / Linux)

### 1. Prerequisites

Install system build tools and LLVM:

```bash
sudo apt update
sudo apt install -y build-essential clang lld llvm cmake curl git
```

Install Rust via `rustup`:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

### 2. Build & Install

```bash
cargo build --release --workspace

# Install for the current user (the runtime library is kept beside the CLI)
install -Dm755 target/release/ube-compiler "$HOME/.cargo/bin/ube"
install -Dm644 target/release/libube_runtime.a "$HOME/.cargo/bin/libube_runtime.a"

# Or install system-wide
sudo install -Dm755 target/release/ube-compiler /usr/local/bin/ube
sudo install -Dm644 target/release/libube_runtime.a /usr/local/lib/libube_runtime.a
```

### 3. Usage Commands

```bash
# 1. Create a new Ube project folder structure
Ube new my_app
cd my_app

# 2. Fast syntax and static type check
Ube check src/main.ube

# 3. Fast compile & execute immediately (1-step execution)
Ube run src/main.ube

# 4. Build standalone native release executable (with -O3 & LTO optimization)
Ube build --release src/main.ube -o my_app
./my_app

# 5. (Optional) Inspect generated textual LLVM IR for compiler debugging
Ube emit-llvm src/main.ube -o main.ll
```

---

## Architecture & Project Structure

For detailed compiler pipeline design, module layout, object layout headers, memory management, and compiler architecture, see:
- [Compiler & Workspace Architecture (`ARCHITECTURE.md`)](ARCHITECTURE.md#2-project--workspace-directory-structure)

---

## Documentation Links

- [Language Specification Baseline (`LANGUAGE_SPEC_DRAFT.md`)](LANGUAGE_SPEC_DRAFT.md)
- [Compiler & Runtime Architecture (`ARCHITECTURE.md`)](ARCHITECTURE.md)
- [Unresolved Design Decisions (`DECISIONS_NEEDED.md`)](DECISIONS_NEEDED.md)
- [Phased Milestones & Roadmap (`MILESTONES.md`)](MILESTONES.md)
- [Master Implementation Plan (`IMPLEMENTATION_PLAN.md`)](IMPLEMENTATION_PLAN.md)

---

## License

Ube is open-source software licensed under the [MIT License](LICENSE).
