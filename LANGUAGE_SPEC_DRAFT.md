# Ube Language Specification Baseline (Draft v0.6.0)

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
* Class-based programming model with instance members and explicitly declared static members; no free functions.
* Static declarations and qualification follow [static members](docs/static-members.md): `static.name` inside the owning class and `ClassName.name` outside it.
* Built-in `Log` for severity-filtered diagnostics on stderr and `Console` for plain stdout output.
* Implicit void return types: methods without an explicit return type default to void. Use `void` for function result types such as `Func() -> void`; method declarations omit the return type for no result.
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
class App {
    start() {
        log('App started')
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

Ube provides exactly six primitive data types (plus `void` as a no-result function type):

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

Every user-defined type in Ube is declared using the `class` keyword. Methods without an explicit return type are **implicitly void**. Function types spell that result as `void`, such as `Func() -> void`.

```ube
class User(
  long id,
  string name,
  var int age = 0) {
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
must match the declared class or trait name exactly, including case: `class App` belongs in `App.ube`.

Local imports use `use path.Type`, resolved relative to the importing file
as `path/Type.ube`. Dependencies are loaded transitively into a shared type
namespace, once per canonical file path, including circular imports. Local
import aliases and symbol lists are rejected. Missing files are errors. Each
imported file is independently checked against the one-type rule. Test-runner
entry points are generated internally and do not count as source declarations.
Documentation snippets grouping several types represent separate files.

### Project manifests and dependencies

`ube.toml` declares `[package]` metadata (`name`, `version`, and optional
`edition = "2026"`), optional `[build].entry` (default `src/App.ube`), and
`[dependencies]`. The entry is a relative `.ube` path within the project.
`ube init` initializes the current directory; `ube new <name>` creates a new
project with `src/App.ube` declaring `class App`. Neither command overwrites an
existing manifest.

Dependency aliases are importable identifiers. Dependencies use either
`{ path = "../package" }` or `{ git = "<repository>", rev = "<revision>" }`, with
`rev` optional. `ube add <alias> --path <directory>` and
`ube add <alias> --git <repository> [--rev <revision>]` record and install them.
`ube remove <alias>` removes the dependency and its installed subtree.
`ube install` resolves direct and transitive packages and records exact Git
commits, versions, and source checksums in `ube.lock`; `--update` explicitly
refreshes locked sources. Version ranges and a package registry are not supported.

`use alias.services.Type` resolves to `src/services/Type.ube` in the installed
dependency, taking precedence over local imports with that prefix. Dependencies
resolve their own imports within their own package context. Installed sources
are checked against `ube.lock` before compilation. Imported types share one
compilation namespace. Cyclic package dependencies are rejected.

Source-less `check`, `emit-ir`, `emit-llvm`, `build`, and `run` use the nearest
ancestor manifest. Manifest builds write to `.build/`; `ube clean` removes only
Ube-managed build output and preserves dependency snapshots and lockfiles.

## 5. Structural `trait` Subtyping (No `implements` Keyword)

Abstraction and behavior reuse in Ube are defined using **`trait`**. Classes satisfy traits **implicitly** based on method signatures without needing an `implements` keyword:

```ube
// Printable.ube
trait Printable {
    // Abstract method signature
    printDetails()

    // Default method implementation
    string getHeader() {
        return '=== PRINTABLE ITEM ==='
    }
}

// Class definitions — NO 'implements' keyword needed!
```

```ube
// User.ube
class User(
  long id,
  string name) {
    printDetails() {
        log('User #${this._id}: ${this._name}')
    }
}
```

```ube
// Invoice.ube
class Invoice(
  long id,
  decimal total) {
    printDetails() {
        log('Invoice #${this._id}: \$${this._total}')
    }
}
```

```ube
// App.ube
use Printable
use User
use Invoice

class App {
    render(Printable item) {
        log(
          item.getHeader())
        item.printDetails()
    }

    start() {
        final user = User(
          1000L,
          'Lem')
        final invoice = Invoice(
          5001L,
          199.99)

        this.render(user)
        this.render(invoice)
    }
}
```

---

## 6. App-Scoped Sharing via Constructor Injection

Create shared objects explicitly inside `App.start()` and inject them through primary constructors. Sharing is a lifetime convention, not an enforced singleton: other instances remain legal, and there is no singleton marker or registry.

```ube
// DatabaseConnection.ube
class DatabaseConnection(string url) {
    query(string sql) {
        log('Querying ${this._url}: ${sql}')
    }
}
```

```ube
// UserService.ube
use DatabaseConnection

class UserService(DatabaseConnection db) {
    findUser(long id) {
        this._db.query('SELECT * FROM users WHERE id = ${id}')
    }
}
```

```ube
// App.ube
use DatabaseConnection
use UserService

class App {
    start() {
        // Created explicitly and shared with dependent services
        final db = DatabaseConnection('postgres://localhost:5432')

        final userService = UserService(db)
        userService.findUser(1000L)
    }
}
```

---

## 7. Concurrency (`Async<T>` and `Channel<T>`)

Start work with `Async.start(() -> expression)` and retrieve its result with
`task.await()`. Ordinary methods remain synchronous until explicitly submitted
inside a zero-argument function. The task starts immediately; creating a function
alone does not execute it. Captures retain the current local values and shared
object references.

Channel operations, combinators, and timeouts also return `Async<T>` values.
Awaiting blocks a synchronous caller. Eligible task bodies use native resumable
poll functions; bodies with ordinary method calls or other potentially blocking
operations use the blocking pool. See [concurrency](docs/concurrency.md) for the
backend limits and ownership rules.

Each class belongs in its matching source file:

```ube
// Calculator.ube
class Calculator {
    int compute(int value) {
        return value * 2
    }
}
```

```ube
// App.ube
use Calculator

class App {
    start() {
        final calculator = Calculator()
        final first = Async.start(
          () -> calculator.compute(10))
        final second = Async.start(
          () -> calculator.compute(20))
        final result = Async.all([first, second])
          .await()
        log(result[0] + result[1])
    }
}
```

`Async.all`, `Async.any`, `Async.race`, and `Async.allSettled` return tasks and coordinate based on completion. `all` preserves input order and fails fast; `any` returns the first successful result; `race` returns the first completed outcome; `allSettled` waits for every task. Empty `any`/`race` inputs fail.

Channels declare an element type and optional positive capacity. `send(value)` returns `Async<bool>` that completes once capacity is available — `true` when queued, `false` if the channel is closed. `receive()` returns `Async<T>`; after buffered values are drained from a closed channel it fails with `channel is closed`. `close()` wakes every waiting operation.

```ube
final events = Channel<string>(16)
final accepted = events.send('ready')
  .await()
final event = events.receive()
  .await()
events.close()
```

**Timeouts.** `task.timeout(ms)` returns a new task that fails with `task timed out` (and cancels the original) if it does not finish in time. It works on any task, so channel timeouts need no extra API:

```ube
try {
    final message = inbox.receive()
      .timeout(1000)
      .await()
    log(message)
} catch (Error err) {
    log(
      err.message())   // "task timed out"
}
```

**Waiting on several sources.** There is no `select` statement; `Async.race([a.receive(), b.receive()]).await()` returns whichever completes first.

**Failures.** A task failure, timeout, or closed-channel receive propagates
from `.await()` into the enclosing `try`/`catch`. An unhandled failure inside
a task propagates to its awaiter; an unhandled application failure exits with
status 1. Failures are never silently turned into zero values.

A worker receives its cancellation request once, whether at a cancellation poll or during `.await()`. After delivery, subsequent awaits, including `finally` cleanup, wait normally. This does not suppress failures or timeouts from the awaited operations.

**Limits.** Cancellation is cooperative: generated async code checks for cancellation at basic-block boundaries, so an indivisible operation already in progress (for example a blocking native call) runs to completion. Task and channel payloads use a two-word ABI value: a 64-bit payload and presence metadata. Nullable scalars (`int?`, `long?`, `byte?`, `bool?`, `decimal?`) retain null independently of zero/false through calls, fields, captures, awaited results, and channel operations. Errors and channel closure remain separate exceptions. Arithmetic directly on an `Async` or `Channel` is a type error. Untyped collections cannot store nullable scalars or tasks returning nullable scalars; these cases receive diagnostics, including nullable task-combinator inputs. Compiler and runtime must be rebuilt together.

---

## 8. Composition Over Inheritance (No `extends` Keyword)

Ube has **no class inheritance** (`extends` is prohibited). Code reuse and polymorphism are achieved through **Primary Constructor Composition & Delegation**:

```ube
// Engine.ube
class Engine(int horsepower) {
    start() {
        log('Engine started (${this._horsepower} HP)')
    }
}
```

```ube
// Logger.ube
class Logger(string prefix) {
    info(string message) {
        log('[${this._prefix}] ${message}')
    }
}

// Composition via Primary Constructor fields
```

```ube
// Car.ube
use Engine
use Logger

class Car(
  string model,
  Engine engine,
  Logger logger) {
    startCar() {
        this._logger.info('Starting vehicle ${this._model}...')
        this._engine.start() // Delegation forwarding
    }
}
```

```ube
// App.ube
use Engine
use Logger
use Car

class App {
    start() {
        final logger = Logger('APP')
        final engine = Engine(400)
        final car = Car(
          'Mustang',
          engine,
          logger)
        car.startCar()
    }
}
```

---

## 9. Sum Classes: Current Limits

Sum-class declarations are parsed and type-checked, but variant construction,
pattern matching, and exhaustive native dispatch are not a complete implemented
feature. Do not use the older `switch` examples as runnable Ube programs. Use
ordinary classes and explicit `if`/`else` branches for executable code.

```ube
// Direction.ube — declaration syntax only
class Direction {
    case North
    case South
    case East
    case West
}
```

---

## 10. `each` / `in` and `while` Loops & Control Flow (`break` & `continue`)

Ube uses `each (item in items)` for collection iteration, `for` for numeric ranges, and `while` for conditional and infinite loops. There is no classic three-part `for` or `do-while` loop in the language. `break` exits the loop immediately, while `continue` skips to the next iteration.

An iteration header is `each (item in iterable)` or `each (index, item in iterable)`. The two-variable form binds a list index and element. Map key/value traversal uses the same syntax, but native map iteration is not yet supported. Numeric ranges use `for (i = 0 to 5)`. The separate `0..5` expression still has no native iteration support. Parentheses and a braced body are required. A comprehension uses the same header in expression position and emits values with `yield`:

```ube
final doubled = each (value in [1, 2, 3]) {
    yield value * 2
}
```

Conditional loops use `while (condition) { ... }`; infinite loops use `while { ... }`. Both `each` and `while` support `break`, `continue`, and an optional `else` block. See the [runnable loop sample](examples/loops/App.ube).

Both `while` forms can also be expressions that collect yielded values into a `List`:

```ube
final list = while {
    yield 1
    yield 2
    break
}
```

Use `final list = while (condition) { ... }` for a conditional collection loop. An unconditional expression needs a reachable exit such as `break` to finish producing its list.

Numeric range loops support an optional step and an inclusive endpoint:

```ube
for (i = 0 to 10, by 3) {
    print(i) // 0, 3, 6, 9
}
final descending = for (i = 10 to 0, by -3) {
    yield i // List containing 10, 7, 4, 1
}
```

The start, endpoint, and step are `int` expressions evaluated once, in that order. The default step is `1`. The loop variable is a read-only `int` scoped to the body. Bounds pointing away from the step give zero iterations; equal bounds give one iteration with any nonzero step. Literal zero steps are diagnosed at compile time, and computed zero steps raise a catchable `GenericError` before either body or fallback executes. Counter arithmetic does not wrap at integer boundaries. `continue` advances the counter before checking the endpoint again; `break` exits immediately. Range loops support the same zero-iteration `else` as `each` and `while`. `to` and `by` are contextual identifiers, not globally reserved keywords.

**Loop `else` means zero body entries.** An optional `else` block runs only if the loop body was never entered. This includes an empty collection or a condition that is initially false. Entering the body sets this state before its first statement: an immediate `break` or `continue` therefore prevents the fallback. Normal completion after one or more iterations also skips it. A return or propagated error follows its normal control-flow path, not the fallback. A nested loop tracks its own body-entry state independently.

```ube
class App {
    start() {
        each (item in []) {
            log(item)
        } else {
            log('empty') // Runs: zero iterations
        }

        each (item in [1]) {
            break
        } else {
            log('empty') // Does not run: the body was entered
        }
    }
}
```

Loop `else` does not test whether a search found a match. Use an explicit flag and a separate `if` for that purpose. See the [worked examples and output](README.md#zero-iteration-fallback).


```ube
class App {
    start() {
        // 1. Infinite Loop (Runs until exited)
        var counter = 0
        while {
            counter = counter + 1
            if (counter == 2) {
                continue
            }
            if (counter > 4) {
                break
            }
        }

        // 2. Conditional Loop (Checks the condition before every iteration)
        var remaining = 3
        while (remaining > 0) {
            remaining = remaining - 1
        }

        // 3. Collection Element Iteration
        final names = ['Lem', 'Rhea', 'Alex']
        each (name in names) {
            log('Name: ${name}')
        }

        // 4. Index & Item Destructuring Iteration
        each (index, name in names) {
            log('${index}: ${name}')
        }

        // 5. Map traversal syntax is not yet supported by native execution.

        // 6. Numeric range (inclusive endpoint)
        for (i = 0 to 4) {
            log('Index: ${i}')
        }
    }
}
```

---

## 11. Conditional Dispatch

`switch` expressions and statements are not currently accepted by the parser.
Use `if`/`else if`/`else` with explicit conditions:

```ube
class App {
    string grade(int score) {
        if (score >= 90) {
            return 'Grade A'
        } else if (score >= 80) {
            return 'Grade B'
        } else {
            return 'Grade F'
        }
    }

    start() {
        Console.printLine(
          this.grade(85))
    }
}
```

---

## 12. Explicit `this` Requirement

Inside any instance method of a class, every access to an instance field or instance method **must** be explicitly qualified with `this.`.

```ube
class Account(
  long id,
  var decimal balance) {
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

Boolean conditions work directly; reference and nullable conditions test presence.
Non-null numbers and strings require explicit comparisons such as `count != 0`
and `text != ''` in `if` and `while` conditions.

```ube
class App {
    start() {
        final isConnected = false
        final input = ''
        final data = 42

        // 1. Logical negation of boolean
        if (not isConnected) {
            log('System disconnected')
        }

        // 2. Explicit empty-string check
        if (input == '') {
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

The entry-point application instance uses the same zero-initialization path as local declarations. Its constructor defaults are not evaluated at startup. Configuration and resource initialization belong explicitly in `start()`.

Non-nullable class cycles are rejected during zero initialization; use nullable references to break them. Runtime-only types (including servers and channels) and traits require explicit initializers unless declared nullable. Zero initialization never invokes their creation APIs.

```ube
// User.ube
class User(string name = 'Guest')
```

```ube
// App.ube
use User

class App {
    start() {
        User zero                 // zero.name() is ''
        final configured = User() // configured.name() is 'Guest'
        User? absent              // null
    }
}
```

---

## 15. String Literals and Interpolated Text Blocks

Single quotes delimit one-line strings and character values: `'Hello'`, `'A'`, and `''`. Characters use the string representation; there is no separate `char` type. Escape sequences such as `\n`, `\t`, `\r`, `\`, and `\'` are interpreted, and existing `${...}` interpolation is supported. Unescaped physical line breaks are errors.

Backticks delimit interpolated text blocks, inline or across multiple lines. `${expression}` evaluates a Ube expression and inserts its formatted value. Physical line breaks and indentation are preserved without trimming. Escapes include `\n`, `\r`, `\t`, `\b`, `\f`, `\v`, `\\`, and an escaped backtick. Write `\${name}` to keep `${name}` literal. Hexadecimal `\xHH`, Unicode `\uHHHH`, and Unicode scalar `\u{...}` escapes are supported; numeric/NUL escapes and surrogate code points are rejected. A backslash followed by a physical line break continues the line without adding a newline. Expressions can contain quoted strings or nested text blocks, and are type-checked and captured in closures normally. Inserted values are not recursively interpolated. Tagged templates and `String.raw` are not implemented.

Double quotes are reserved for JSON syntax, not standalone Ube literals. JSON payloads can be written inside text blocks and passed to `Json.toObject`. Double each JSON backslash in Ube source so it reaches the JSON parser unchanged. No dedicated JSON literal type is introduced by this rule.

```ube
final name = 'Lem'
final count = 3
final greeting = `Hello ${name}
Items: ${count * 2}`
print(greeting)
// Hello Lem
// Items: 6
```

```ube
final character = 'A'
final message = 'Hello\nWorld'
final json = `{"name":"Ube","active":true,"line":"\\n"}`
final sql = `select id, name
from users
where active = true`
```

The SQL value contains two line breaks and no trailing newline. An opening line break, if present after the first backtick, would also be part of its value.

## 16. Automatic Data Serialization

Concrete object instances, lists, maps, and supported scalars expose `.toJson()` and `.toXml()` without an import or user-written method. The names `toJson` and `toXml` are reserved: custom methods and constructor fields with those names are rejected. Encoders include declared constructor fields, including private fields, recursively. Methods, computed getters, and object identity are omitted. Repeated references become repeated data; cyclic references, runtime resources, and non-finite decimals raise errors.

`Json.toObject(text)` returns parsed JSON data. `Xml.toObject(text)` returns an `XmlDocument`, whose `root()` provides element traversal. Neither call automatically reconstructs an application class. XML scalar text has no implicit application type; callers choose how to interpret it.

Default XML roots are the concrete class name, `list`, `map`, or `value`. Fields become child elements, list items use `item`, and map entries use `entry` with a `key` attribute. Null uses `xsi:nil="true"` with the XML Schema instance namespace; empty strings and empty containers remain distinct from null. Text and attributes are escaped.

`.toXml(XmlOptions())` accepts optional fluent configuration: `.root(name)`, `.attribute(field)`, `.rename(field, name)`, `.namespace(prefix, uri)`, and `.listItem(name)`. Field attribute/rename rules apply to the root data class. Names must be valid and prefixes bound. Parsed documents and elements serialize with `.toXml()` without options; use document editing methods to change them.

Malformed input raises `XmlParseError`; unsupported XML encoding or customization raises `XmlError`. Encoding and parsing enforce documented bounds. Serialization is synchronous and does not provide an atomic snapshot of a concurrently mutated object graph. See [JSON](docs/json.md) and [XML](docs/xml.md) for supported types, exact mappings, limits, and examples.


### Conditional expressions: `then` / `else`

Use `condition then value else value` for a choice between two expressions. Both single-line and multiline forms are supported:

```ube
class App {
    start() {
        final active = true
        final status = active
          then 'ACTIVE'
          else 'INACTIVE'
        print(status)
    }
}
```

The condition follows Ube's normal `if` rules: booleans test their value, and nullable/object conditions test presence. Bare numbers and strings require an explicit comparison. The condition is evaluated once. Only the selected branch executes. Both branches are required and must produce compatible, non-void values. Numeric branches promote to a common numeric type; a nullable branch makes the result nullable. Incompatible types are compile errors.

The conditional binds more loosely than arithmetic, comparisons, logical operators, and `??`, and more tightly than assignment. Nested expressions associate to the right: `a then x else b then y else z` means `a then x else (b then y else z)`. Parenthesize complex nesting for readability. Conditionals work in arguments, returns, interpolated strings, and closures. The symbolic `condition ? yes : no` form is not supported. Ube's existing `??` null fallback is unchanged.

Do not prefix a ternary expression with `if`; use `condition then yes else no`. Line breaks are allowed before `then` and `else`, and parentheses around the condition are optional. `then` remains contextual, so qualified member names retain their usual behavior. Braced `if` expressions with `yield` remain supported.


## USSR HTML templates

`html` followed by a backtick template is a built-in expression producing reserved type `Html`. Interpolations are parsed and type-checked as Ube expressions. Non-null scalar values are escaped for HTML; `Html` values compose only in element content. Component methods use ordinary typed parameters and an `Html` return type. `then`/`else` expressions select conditional fragments.

The compiler validates a bounded HTML grammar and rejects unsupported interpolation contexts. URL attributes require a single complete-value interpolation and are validated at runtime. `Response.ok(Html)` and `Response.html(Html)` send the HTML content type. Ordinary string templates and `Response.ok(string)` retain their behavior. See [USSR](docs/ussr.md) for the precise grammar restrictions and current collection limitations.

Supported HTMX attributes also accept the `data-hx-*` spelling with identical interpolation restrictions. Request URL aliases receive URL validation; unsupported dynamic HTMX attributes remain rejected even with the `data-` prefix.
