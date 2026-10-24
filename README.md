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

### 14. Virtual Thread Task Concurrency (`async` / `await` & `Task` Primitives)
- **Framework-Neutral Concurrency**: Patterned after Java's `vt-async-await` Virtual Threads model. Non-reactive, cheap sequential blocking `await`.
- **Spawning Tasks**: `async expr`, `Task.async(expr)`, or `async worker.process(1)`.
- **Promise Combinators**:
  - `Task.all(tasks)`: Fail-fast combinator (cancels remaining tasks on failure, re-throwing error into `catch(err)`).
  - `Task.any(tasks)`: Returns the result of the first task that successfully completes.
  - `Task.race(tasks)`: Returns outcome of the first completed task (cancels remaining tasks).
  - `Task.allSettled(tasks)`: Awaits all tasks to complete without throwing.
- **Timeouts & Cancellation**: `awaitAndCancel(task, timeoutMs)`, `task.cancel()`.
- **Inter-Task Channels**: Communication via typed `Channel` queue (`.send()`, `.receive()`).

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

### 21. High-Performance Web Server Standard Library (`Server`, `Request`, `Response`, `HTTP/1.1`, `HTTP/2`, `HTTP/3 QUIC`)
- **Express / Ktor Ergonomics**: Express-style fluent routing: `server.get("/users") { request -> ... }`, `server.post()`, `server.put()`, `server.delete()`, `server.use()` (middleware), `server.static()`, `server.websocket()`, `server.start(8080)`.
- **Multi-Protocol Support**: Default `HTTP/1.1` TCP transport, `server.enableHttp2()` for TLS ALPN multiplexing, and `server.enableHttp3(443)` for native UDP QUIC 0-RTT transport.
- **Request & Response Ergonomics**: `request.pathLong("id")`, `request.query("search")`, `request.hasBearerToken()`, `request.cookie("session")`, `request.protocol()`, `request.isHttp3()`, `Response.ok("hello")`, `Response.json(data)`.

### 22. First-Class Native JSON & XML Record Mapping (`Json`, `JsonObject`, `JsonArray`, `Xml`, `XmlDocument`, `XmlElement`)
- **Bi-Directional Document & Class Mapping**: `Json.parse(text)`, `Json.partialParse(text, TargetClass)`, `Json.stringify(obj)`, `Json.from(obj)`, `Json.to(text, TargetClass)`.
- **XML Document Traversal & Generation**: `Xml.parse(text)`, `Xml.create("root")`, `doc.root().child("tag").text()`, `doc.toString()`.
- **Reversible Character Codec (`DataNameCodec`)**: Maps XML/JSON tag/key names with hyphens (`Facility-ID`), namespaces (`rest:KitchenOrderScan`), or dots to Ube identifier conventions (`Facility$hyphen$ID`, `rest$KitchenOrderScan`).
- **Strict vs. Partial Parsing Modes**: Strict mode enforces 1-to-1 structural completeness; Partial mode selectively maps declared components while assigning defaults (`null`, `List.of()`, `0`, `false`).
- **Structured Format Error Handling**: Exception-free error handling via `guard`/`catch(err)` with `JsonParseError`, `XmlParseError`, `MissingDataError`, `ExcessDataError`.

---

## VS Code Extension & Syntax Highlighting

Ube includes an official VS Code extension in [`editors/vscode`](editors/vscode) providing full `.Ube` file recognition, syntax highlighting, bracket matching, string interpolation highlighting, and code folding.

### Installation Options:

#### Option 1: Quick Install (Copy Extension Directory)
```bash
# Linux / macOS
cp -r editors/vscode ~/.vscode/extensions/Ube-vscode

# Windows (Command Prompt)
xcopy /E /I editors\vscode %USERPROFILE%\.vscode\extensions\Ube-vscode
```

#### Option 2: Build & Install `.vsix` Package
```bash
cd editors/vscode
npx vsce package
code --install-extension Ube-vscode-0.5.0.vsix
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

### 11. Virtual Thread Task Concurrency (`async`, `await`, `Task` Combinators & `Channel`)

In Ube, lightweight task-based concurrency is patterned after Java Virtual Threads (`vt-async-await`). Tasks can be spawned with `async`, `Task.async()`, or `async method()`, awaited sequentially, or coordinated via static promise combinators:

```java
class CustomerService {
    async findRequired(long id) {
        return User(id, `Lem`, 35)
    }
}

class OrderService {
    async findForCustomer(long id) {
        return [`Order #1001`, `Order #1002`]
    }
}

class Application {
    main() {
        final customerService = CustomerService()
        final orderService = OrderService()

        // 1. Parallel submission on lightweight tasks (Java vt-async-await style)
        final customerTask = Task.async(customerService.findRequired(1000L))
        final ordersTask   = Task.async(orderService.findForCustomer(1000L))

        // 2. Sequential blocking await (cheap execution)
        final customer = await customerTask
        final orders   = await ordersTask
        print(`Loaded customer ${customer.name()} with ${orders.len()} orders`)

        // 3. Task.all Promise Combinator (Fail-fast & cancel remaining tasks)
        guard {
            final tasks = [
                async 10,
                async 20
            ]
            final results = await Task.all(tasks)
            print(`All tasks finished with ${results.len()} results`)

            catch(err) {
                print(`Task failed: ${err.message()}`)
            }
        }

        // 4. Inter-Task Channel Communication
        final channel = Channel()
        channel.send(42)
        final val = channel.receive()
        print(`Channel received: ${val}`)
    }
}
```

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
        final server = Server()

        // Enable HTTP/2 TLS ALPN and HTTP/3 UDP QUIC
        server.enableHttp2()
        server.enableHttp3(443)

        // Fluent Express/Ktor style routing & middleware
        server.use { req ->
            print(`[${req.method()}] ${req.url()} (${req.protocol()})`)
        }

        server.get(`/users/{id}`) { request ->
            long id = request.pathLong(`id`)
            return Response.json(`{"id": ${id}, "name": "Lem"}`)
        }

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
sudo cp target/release/ube-compiler /usr/local/bin/ube
sudo cp target/release/libTimawa_runtime.a /usr/local/lib/libTimawa_runtime.a
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

Timawa is open-source software licensed under the [MIT License](LICENSE).
