# Ube Programming Language

> **Enjoyable, simple, native.**

[![Language Specification](https://img.shields.io/badge/spec-v0.6.0-blue.svg)](LANGUAGE_SPEC_DRAFT.md)
[![Compiler Status](https://img.shields.io/badge/compiler-Rust%20%2B%20LLVM-orange.svg)](ARCHITECTURE.md)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

**Ube** (name taken from the delicious Pilippine purple yam root crop) is a NO INHERITANCE, NO INTERFACE, class-based composition programming language designed for native performance, low memory footprint, and high safety. Positioned between GO and JAVA, **Ube** combines native single-binary deployment and direct native code generation via LLVM like RUST.

---

## Projects and dependencies

Create a project or initialize an existing directory:

```sh
ube new my-app
cd my-app
# Or run `ube init` in an existing directory (`--name my-app` overrides its name).
ube build
ube run
ube test
ube clean
```

`ube new` creates `ube.toml` by default, `src/App.ube` containing `class App`, a `tests/`
directory, a README, and a `.gitignore`. `ube init` preserves existing source
files, creates or updates `.gitignore` with `.build/` while preserving
existing rules, and refuses to overwrite an existing manifest. If your sources already
exist, set the entry path to their correctly capitalized filename.

Projects support exactly one manifest: `ube.toml`, `ube.yaml`, or `ube.json`.
All formats use the same fields and validation. Multiple manifests in one
project cause an error; Ube never silently chooses one. TOML remains the default:

```sh
ube new my-app --format yaml
ube init --name my-app --format json
```

`ube add`, `ube remove`, and `ube install` preserve the selected format.
YAML and JSON are serialized again when dependencies change, so formatting and
YAML comments may change. `ube.lock` always uses the same TOML format.

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2026"

[build]
entry = "src/App.ube"

[dependencies]
# Local path, relative to this manifest:
greetings = { path = "../greetings" }
# Git dependency; rev is an optional branch, tag, or commit:
# utilities = { git = "https://github.com/your-org/ube-utilities.git", rev = "v1.0.0" }
```

With no source argument, `ube check`, `ube emit-ir`, `ube emit-llvm`, `ube build`,
and `ube run` find the nearest supported manifest in the current directory or its parents.
The default entry is `src/App.ube`. Before compilation, `ube build` verifies
that `.gitignore` protects `.build/` and fails on missing or conflicting rules.
This also applies to explicit source builds;
standalone files use their containing directory. `ube build` writes the executable and LLVM
IR to `.build/<package-name>` and `.build/<package-name>.ll`; `--release` and
`-o <output>` are supported. Explicit file commands such as
`ube build src/App.ube -o app` continue to work. `ube test` defaults to the
project's `tests/` directory, including when invoked from a subdirectory.

```sh
ube add greetings --path ../greetings
ube add utilities --git https://github.com/your-org/ube-utilities.git --rev v1.0.0
ube install
ube install --update
ube remove utilities
```

`ube add` records and installs a dependency. Its alias must be an identifier
because it is used in imports. `--path` is interpreted from the calling directory
and saved relative to the project root. `ube remove` removes its manifest entry,
lock records, and installed snapshots, including dependencies no longer needed.
It preserves the dependency's original repository or directory.

Dependencies are Ube projects with `ube.toml` and `.ube` source files under
`src/`. Import a dependency's types using its alias:

```ube
// src/App.ube; greetings/src/Greeting.ube declares class Greeting.
import greetings.Greeting
class App {
    start() {
        log(Greeting().message())
    }
}
```

`import greetings.services.Greeting` resolves to the dependency's
`src/services/Greeting.ube`. A declared dependency alias takes precedence over a
local import with the same prefix. Transitive dependencies resolve within their
own packages. Types currently share one compilation namespace, so imported
packages must avoid duplicate class or trait names. Package dependency cycles
are rejected; circular source imports within a package remain supported.

`ube install` installs direct and transitive dependencies into `.build/.deps/` and
writes **`ube.lock`**, recording source specifications, package versions, exact Git
commits, and SHA-256 checksums of manifests and `.ube` sources. Commit `ube.lock`;
ignore `.build/`. Install respects existing Git pins and rejects local
source changes that differ from the lock. Use `ube install --update` to accept
changed local sources and refresh Git revisions. A fixed `--rev` commit remains
fixed even with `--update`. Builds verify installed sources against the lock;
run `ube install` after editing dependencies manually or after a fresh checkout.
Git installations require Git and access to the source repository. Ube currently
supports local paths and Git repositories; there is no public package registry or
version-range resolution. Installation copies only manifests and `.ube` sources,
rejects source symlinks, and runs no package installation scripts.

`ube clean` deletes generated outputs inside Ube-managed `.build/` directories.
It preserves source files, `ube.toml`, `ube.lock`, `.build/.conf`, and installed dependencies
in `.build/.deps/`. The `.build/` directory remains when it contains preserved state.
It refuses unmanaged or symlinked `.build` directories. Outputs created with an
explicit source path or custom `-o` outside `.build/` must be removed separately.

## Proposed Clean Architecture and SOLID enforcement

**Status: proposed, not implemented.** Architecture enforcement would be optional.
Projects without an architecture policy would continue working normally. The
same policy would be supported in `ube.toml`, `ube.yaml`, and `ube.json`.

The proposed mechanism separates checks that the compiler can enforce from
design concerns that require linting, tests, or human judgment:

| Mechanism | Checks | Result |
|---|---|---|
| Compiler | Declared layer boundaries, forbidden dependencies or built-ins, dependency cycles | Build error |
| Proposed linter | Complex methods, large interfaces, classes with many dependencies | Warning for review |
| Contract tests | Whether implementations honor shared behavioral expectations | Test failure |

The first implementation would focus on compiler enforcement:

1. Declare layers by source directory, such as domain, application,
   infrastructure, and presentation.
2. Specify which layers each layer may depend on. For example, application
   could depend on domain, while domain could not depend on infrastructure.
3. Optionally restrict direct use of built-ins such as SQL, HTTP, filesystem,
   and environment access in business layers.
4. Explicitly identify composition files, such as `App.ube`, where concrete
   implementations are constructed and connected to business abstractions.
   Exceptions would apply only to those declared files.
5. During `ube check`, `ube build`, and `ube test`, inspect resolved dependencies
   from imports, type signatures, inheritance, object construction, and fully
   qualified references. Reject forbidden dependencies, dependency cycles, and
   source files outside the declared layers.
6. Report each violation with its source location, originating layer, and
   forbidden dependency.

These boundaries support Clean Architecture and SOLID's Dependency Inversion
principle: business layers define abstractions, and infrastructure implements
them. SOLID is broader than dependency rules. Single Responsibility and
Interface Segregation need design review, with lint warnings as supporting
signals. Open/Closed depends on how extension points are designed. Liskov
Substitution requires behavioral contract tests in addition to type checking.
Passing automated checks would not prove that a design is clean or fully SOLID.

Teams would run the same compiler checks, future lint checks, and contract tests
locally and in CI. No architecture configuration syntax or new lint command is
available yet.

## Encrypted environment values

Store configuration once instead of supplying every variable on each execution.
Ube encrypts it into `.build/.conf`. `ube new` and `ube init` ensure `.gitignore`
includes `.build/`. `ube build` fails if that ignore rule is missing or any
`.build` exception prefixed by `!` (including `! `) is present. In Git repositories,
builds and `ube env` also check Git's effective ignore rules and refuse to proceed
if the configuration is tracked or not ignored.

Import a nonempty JSON object with string values:

```json
{
  "DB_PASSWORD": "your-password",
  "okta_username": "username_value",
  "okta_secret": "secret_value"
}
```

```sh
ube env file.json
```

**JSON import replaces the entire configuration.** Variables absent from the
file are removed, and the previous encryption key is not required. After a
verified encrypted save, Ube prints and flushes the new encryption key before
consuming (deleting) the input JSON. If key output fails, the JSON is preserved
so you can retry the import. Invalid input or a failed save also preserves the input. If deletion
fails after saving, Ube still prints the new key and reports that the plaintext
needs manual removal. Deletion cannot erase Git history, editor history, or backups.

For individual additions or updates, use `--add`. The first save needs no key:

```sh
ube env --add "DB_PASSWORD":"your-password", "okta_username":"username_value", "okta_secret": "secret_value"
```

Later updates require the latest key and preserve other stored variables:

```sh
ube env --key "<latest-key>" --add "DB_PASSWORD":"new-password", "API_TOKEN":"token-value"
```

Comma-separated batches save atomically. Spaces after the colon, colons within
values, and empty values are supported; quote arguments for your shell. An
optional `--key "<latest-key>"` on JSON import verifies the existing configuration
before replacing it. Names must be environment identifiers; `UBE_ENV_` is
reserved. Total stored data and JSON input are limited to 1 MiB.

**Every successful save rotates the encryption key. Only the latest key decrypts
the current configuration.** Ube prints the new key after saving and never stores
it or embeds it in compiled programs. Keep it in a password manager or secret
manager. Never commit it or expose it in chats, screenshots, recordings, or logs.
Command-line keys and values can appear in shell history and process listings.

For runtime or deployment, provision the latest key in a protected file outside
the project. The file accepts the printed 64-character hexadecimal key or 32 raw
bytes; on Unix, restrict permissions to its owner (`chmod 600`).

```sh
UBE_ENV_KEY_FILE=/secure/my-app.key ube run
```

For a deployed executable, supply the encrypted configuration and key separately:

```sh
UBE_ENV_FILE=/srv/my-app/.build/.conf \
UBE_ENV_KEY_FILE=/run/secrets/my-app.key \
/srv/my-app/my-app
```

Read values with `Env.require('DB_PASSWORD')` (`string`, throws if missing),
`Env.get('OPTIONAL_TOKEN')` (`string?`), or `Env.has('API_TOKEN')` (`bool`). Real
process environment variables take precedence. Runtime lookup decrypts on
access; builds never require the key, and decrypted values are not exported into
the process environment. `ube clean` preserves `.build/.conf`.

See [encrypted environment configuration](docs/environment.md) for recovery,
Git protection, and security limits.

## Testing Ube programs with Testicle

Use `ube test` to discover `test*` methods in `tests/**/*Tests.ube` and
`tests/**/*Test.ube`. Native tests support `Testicle.equal`, `Testicle.notEqual`,
`Testicle.isTrue`, `Testicle.isFalse`, and `Testicle.fail`, async tests, per-test
setup/teardown, process isolation, and execution timeouts.

```sh
ube test
ube test examples/testing
```

See [the native testing API](docs/testing.md) for examples and current limits.
Use [Mockery](docs/mockery.md) for strict trait mocks, typed stubs, and call-count
verification with `Mockery.mock`, `Mockery.returns`, and `Mockery.verify`.
Use [RestInPeace](docs/rest-in-peace.md) to test running HTTP services with fluent
requests, typed JSON getters, and Testicle assertions. It supports HTTP/1.x,
verified HTTPS/HTTP/2, response-size limits, and request timeouts.

## Source files and local imports

Each `.ube` file may declare **one top-level class or one top-level trait**.
A class and a trait must live in separate files, too. Imports do not count as
declarations; an import-only file is allowed. The filename before `.ube` must
match its class or trait name character for character, including capitalization:
`class App` belongs in `App.ube`, never `main.ube`.

Use explicit imports to load other files relative to the importing file:

```ube
// Greeting.ube
trait Greeting {
    string greet()
}
```

```ube
// Welcome.ube
import Greeting
class Welcome(Greeting greeting) {
    string message() { return this._greeting.greet() }
}
```

`import services.Greeting` loads `services/Greeting.ube`. Imports are transitive;
repeated and circular imports load each physical file once. Imported declarations
share the compilation's type namespace. Aliases and symbol-list imports are not
supported by the local file loader. A missing import is a compiler error.
These rules apply to `check`, `build`, `run`, IR emission, and `test`.
The test runner executes tests declared in each discovered test file, without
also executing tests from its imported dependencies.

Examples below that show multiple types together describe separate source files;
put each class or trait in its own file and import its dependencies.

## Language Core Principles & Policies

### 1. Class-Based Programming (No Free Functions)
- **Instance and Static Members**: Fields and methods belong to instances by default. An explicit `static` modifier makes a member belong to the class. Use `static.name` inside the owning class and `ClassName.name` outside it; static methods have no `this`.
- **Static Fields**: Declare `static var int total = 0` for mutable state or `static final int limit = 100` for an immutable binding. Static fields use direct reads and assignments; instance fields retain their accessor rules. See [static members](docs/static-members.md) for initialization, visibility, and concurrency rules.
- **Application Entry Point**: The application entry point is an instance or static method (`start()`) on a class, conventionally `App` in `App.ube`. `App.start()` takes precedence over other classes' `start()` methods. `main()` is an ordinary method. The compiler automatically generates the native C-ABI wrapper.
- **Shared Instances via Constructor Injection**: Create an object explicitly in `App.start()` and pass it to dependent classes. This shares the instance but does not enforce singleton uniqueness; there is no singleton marker or runtime registry.

### 2. Zero Inheritance & Maximum Performance
- **No `extends` or Abstract Classes**: Ube has **no inheritance**.
- **Dispatch**: Concrete instance calls use direct functions. Trait calls use runtime structural method lookup; objects do not carry an inheritance vtable.
- **Aggressive Inlining**: LLVM inlines instance method bodies directly into caller sites.
- **Cache Locality**: Class instance headers do not store virtual pointer (`vptr`) fields, resulting in smaller memory headers and superior CPU L1/L2/L3 cache hit rates.

### 3. Structural `trait` Subtyping (No `implements` Keyword)
- **Behavioral Contracts**: Abstraction and behavior reuse are declared using `trait`.
- **Implicit Structural Subtyping**: Classes satisfy traits **implicitly** based on method signatures (with direct concrete calls and runtime structural dispatch for trait values).
- **Default Method Logic**: Traits can supply default method implementations, enabling code reuse without class inheritance (`extends`).

### 4. Unified `class` Model & Enums via Sum Classes (No `enum` Keyword)
- **Single Type Keyword (`class`)**: `class` is the sole user-defined type keyword (no `enum`, `struct`, `record`, or `union`).
- **Primary Constructor Fields**: `class User(long id, string name, var int age)` automatically generates class-private backing fields (`_id`, `_name`, `_age`) and public getters (`id()`, `name()`, `age()`). A `var` field also supports a setter using the same name with one argument, such as `age(30)`.
- **Empty Class Bodies**: Classes with no body must omit braces, for example `class Address(string city)`. Empty `{}` class bodies are rejected.
- **Enums via Sum Classes**: Enumerations and algebraic variants are declared using `case` clauses inside a `class` (e.g. `class Status { case Pending case Approved(long orderId) }`).

### 5. No `void` Keyword & Implicit Void Return Types
- **No `void` Keyword**: The `void` keyword does not exist in Ube.
- **Implicit Void Return Type**: Methods declared without a return type automatically return void (`start() { ... }` or `birthday() { ... }`).
- **Explicit Return Types**: Methods returning values specify the return type before the method name (`int getAge() { return this._age }`).

### 6. Explicit `this.` Qualification
- Instance field and method accesses inside a class **must** be explicitly prefixed with `this.`. Static members of that class require `static.`. Unqualified access produces a compiler error.

### 7. Naming-Based Access Control
- Public fields use accessor calls: `user.name()` and `user.age()` read values; `user.age(value)` writes a `var` field. Immutable fields have no setter. Direct access such as `user.age` or `user.age = value` is rejected.
- Underscore-prefixed backing fields are class-private: only code inside `User` can read or write `user._age`. There are no `public`, `private`, or `protected` keywords.

### 8. Built-in Logging & Console Output
- `log(value)` and `Log.info(value)` are equivalent. `Log.warn(value)`, `Log.debug(value)`, and `Log.error(value)` record the corresponding severity.
- Logs go to **stderr** with a level label and a newline, for example `[INFO] Ready`. Logging an error does not throw or stop the program.
- The default threshold is `info`: debug messages are hidden. `Log.setLevel('debug')` enables every level; `info`, `warn`, `error`, and `off` select higher thresholds. The setting is shared across tasks.
- `Console.read()` reads one Unicode character; `Console.readLine()` reads a line, removing its LF or CRLF terminator. Both take no arguments and return `string?`: `null` at EOF, and an empty string for an empty line. Input failures raise `GenericError`. Reads use buffered stdin and may wait for Enter in an interactive terminal.
- `Console.print(value)` writes plain **stdout** without a newline; `Console.printLine(value)` adds one. Console output is never filtered.
- Values can be strings, numbers, booleans, nullable scalars/strings, or objects satisfying `Error` (their `message()` is logged). NULL is written as `null`.
- `print(value)` is shorthand for `Console.printLine(value)`. Use `log(...)` for diagnostics.

```ube
class App {
    start() {
        log('App started')
        Log.info('Ready')
        Log.warn('Retrying connection')
        Log.setLevel('debug')
        Log.debug('Detailed diagnostics')
        Log.error('Connection failed')
        Console.printLine('User-facing result')
    }
}
```

`Log` also provides **MDC (Mapped Diagnostic Context)** for attaching fields to
subsequent log records:

```ube
Log.put('requestId', 'req-42')
try {
    string? requestId = Log.get('requestId')
    Log.info('Processing request')
    // stderr: [INFO] {"requestId":"req-42"} Processing request
} finally {
    Log.clear()
}
```

| Method | Behavior |
| --- | --- |
| `Log.put(string key, string value)` | Add or replace a context field. |
| `Log.get(string key)` | Return `string?`; missing keys return `null`. |
| `Log.remove(string key)` | Remove one field; missing keys are harmless. |
| `Log.clear()` | Remove every field in the current context. |

Context fields appear on all emitted log levels as a JSON object with sorted,
escaped keys and values. Plain Console output is unchanged. Keys and values must
be non-null strings; use interpolation to format other values. Returned values
are copies and remain valid after context changes.

Each Ube task inherits a snapshot when it is created. Changes in a child task,
parent, or sibling do not affect the other contexts. Worker contexts are restored
when tasks finish, including errors and cancellation. Clear context in `finally`
at request or operation boundaries so it does not carry into the next operation
on the same execution thread. The logging threshold remains process-wide.
See the [runnable context example](examples/log_context/App.ube).

### 9. Truthy & Falsy Conditions
- Conditions in `if (condition)` and `for (condition)` accept truthy and falsy values:
  - **Falsy**: `false`, `null`, `0`, `0.0`, `-0.0`, `''` (empty string).
  - **Truthy**: `true`, non-zero numbers, non-empty strings, class instances, anonymous objects.
- **Flow-Sensitive Narrowing**: Evaluating a nullable `User?` in an `if (user)` block automatically narrows `user` to non-null `User`.

### 10. Word-Based Boolean Operators & `not` Keyword
- Logical operators use English words: `not`, `and`, `or`. The `!` symbol is prohibited (replaced by `not`).

### 11. First-Class Nullability & Optional Operators
- Native `null` (never `nil`) with postfix `Type?` (`User?`).
- Optional chaining (`?.`), null coalescing (`??`), and null-coalescing assignment (`??=`).

### 12. Collections Policy & Unified `for` Loops (No `while` Keyword)
- **`for` is the Sole Loop Keyword**: There is no `while` or `do-while` keyword in Ube.
- **`for` / `else` (0-Iteration Fallback)**: `for (item in items) { ... } else { ... }`. The `else` block executes **only when the loop body is entered zero times**, such as an empty collection or an initially false condition. Once the body is entered, even an immediate `break` or `continue` prevents the fallback. See the [worked examples](#zero-iteration-fallback).
- **`break` & `continue`**: `break` exits the nearest loop; `continue` skips to the next iteration.
- **`Array<T>`**: Fixed-size contiguous buffer (`Array<int>(10)`).
- **`List<T>`**: Dynamic growable sequence (`[1, 2, 3]` literal syntax, `.add()`, `.remove()`).
- **`Map<K, V>`**: Key-value hash map (`[1000L: `Lem`]` literal syntax, key-value `for (k, v in map)` iteration; integer bracket lookup returns `int?`, preserving missing entries separately from stored zero).

### 13. Pattern Matching `switch` Expressions & Statements
- **First-Class Expressions**: `switch` can evaluate directly to a value (`final x = switch (...)`).
- **No Fall-Through**: Cases do not fall through (no `break` required inside cases).
- **Pattern Support**: Matches constant values, Sum Class variant payloads, type checks (`is`), guard clauses (`if`), multiple OR values, and anonymous objects.

### 14. Concurrency Runtime (Tokio)
- `Async.start(() -> expression)` submits a zero-argument function to the shared multithreaded Tokio runtime. Captured values are retained for the task, and `.await()` returns its result.
- `Async.startAll([operations])` starts several zero-argument functions and collects their values in input order. `Async.completed(value)`, `Async.failed(error)`, and `Async.delay(ms)` construct tasks without an `async` declaration.
- Tasks support `.cancel()`, `.timeout(ms)`, `.isCompleted()`, and `.isCancelled()`. `Async.all`, `Async.any`, `Async.race`, and `Async.allSettled` combine existing tasks.
- The Rust runtime provides typed `Task<T>` results/errors, completion-based combinators, timeouts, cancellation, and bounded closable channels.
- The compiler ABI transports task and channel values through an inline two-word value (64-bit payload plus presence metadata), encoding primitive values and pointers. `Async.start` produces `Async<T>`; `.await()` retrieves `T`; failures propagate into a surrounding `try`/`catch`.
- One rule: **start work with `Async.start`, then call `.await()` when its result is needed.** Ordinary methods need no `async` declaration.
- `Channel<T>(capacity)` is a typed bounded channel: `ch.send(v).await()` (false once closed), `ch.receive().await()`, and `ch.close()`. A closed-channel receive after queued values are drained is catchable.
- `task.timeout(ms).await()` fails with a catchable `task timed out` and cancels the task; it works on any task, including `ch.receive()`.
- Ube uses `Async<T>` and combinators such as `Async.all(...)` and `Async.race(...)`. Legacy `async` and prefix `await` forms remain accepted for source compatibility; new code should use `Async.start(() -> expression)` and `value.await()`.
- Unhandled failures propagate out of async code and otherwise stop the program with a clear error.

### 15. Exception Model (`try` / `catch` / `finally` & `throw`)
- **`try` Block**: Followed by typed catches, a `finally` block, or both.
- **Typed Catches**: `catch (Error err)` handles any error; concrete classes and structural error traits can narrow the match. Optional `if (condition)` guards select handlers in source order.
- **`finally` Cleanup**: Runs on normal completion, errors, returns, and loop exits. If cleanup throws while another error is pending, the cleanup error replaces it and preserves the original as suppressed.
- **`throw` Keyword**: `throw 'msg'` wraps a string in `GenericError`; `throw CustomError(...)` preserves the error object. Error objects structurally provide `string message()`.


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
- **Instant Invariance**: `manila.in('Asia/Tokyo')` changes displayed clock time (`17:00` vs `18:00`) while preserving `manila.timestamp() == tokyo.timestamp()` (representing the exact same instant).
- **Unix Timestamps & Timezones**: `dt.timestamp()`, `dt.unix()`, `dt.unixMillis()`, `dt.unixNanos()`, `DateTime.fromUnix(sec)`, `DateTime.fromUnixMillis(ms)`.
- **Dedicated `Duration` Class**: `Duration.seconds(n)`, `Duration.hours(n)`, `Duration.days(n)` cleanly represents time amounts (`dt.addDuration(duration)`).

### 19. Financial Standard Library (`Money`, `Currency`, `Exchange`, `Percent`, `Rate`)
- **Exact Money API**: `Money.parse('0.10', 'PHP')`, `.amountText()`, `.multiplyPercent('12.5')`, and `.convertExact('USD', '0.1')` use checked decimal arithmetic without silent rounding. Numeric factories and `.amount()` remain approximate `f64` boundaries; language `decimal` is unchanged. See [limits and error behavior](docs/financial.md).
- **Currency Safety**: `subtotal.add(shipping)` and `total.subtract(discount)` strictly enforce matching currencies. Mismatched currencies raise catchable `CurrencyMismatchError` through the exception ABI unless explicitly converted via `Exchange.convert(dollar, Currency.PHP, rate)`.
- **Standard Currencies**: `Currency.PHP`, `Currency.USD`, `Currency.EUR`, `Currency.JPY`, `Currency.GBP`.
- **Financial Percentages & Rates**: `Percent(12)` and `Rate.percent(5.5)` for tax, interest, and discount calculations (`price.multiply(vat)`).

### 20. Native Filesystem Standard Library (`File`, `FileInfo`, `Directory`, `Path`)
- **Catchable File Errors**: File and directory failures raise typed errors such as `FileNotFoundError`, `PermissionError`, or `FileError`.
- **Object-Oriented & Static Helpers**: `File.read('app.txt')`, `File.write('out.txt', data)`, `file = File.open(path)`, `file.read()`, `file.close()`.
- **Resource Cleanup**: Close file handles in a `finally` block.
- **Directories & Paths**: `Directory.create('logs')`, `Path.join('src', 'main.Ube')`.

### 21. Web Server Standard Library (`Server`, `Request`, `Response`)
- **Lambda and route syntax**: Use Java-style lambdas inside the argument list: `server.get('/', (Request request) -> { return Response.ok('hello') })`. Expression bodies such as `(Request request) -> Response.ok('hello')` also work. Kotlin-style trailing handler blocks are rejected.
- **Built-in naming**: Built-in classes require their canonical capitalized names, such as `Server`, `Request`, `Response`, `Json`, and `SQLite`. Lowercase class aliases are not supported. Instance variables such as `server` and `request` may still be lowercase.
- **Express / Ktor Ergonomics**: Fluent routing with `server.get()`, `server.post()`, `server.put()`, `server.delete()`, `server.use()` (middleware), `server.static()`, `server.websocket()`, and `server.start(8080)`.
- **OAuth 2.0 & JWT**: `OAuth2.create(...)` supports external authorization-code providers with session-bound state, S256 PKCE, and refresh tokens. `Jwt.sign(...)` and `Jwt.verify(...)` support HS256/RS256 with explicit issuer, audience, and expiration validation. See [authentication setup and API](docs/authentication.md), [the OAuth example](examples/oauth2/App.ube), and [the JWT example](examples/jwt/App.ube).
- **Sessions**: `request.session()` provides `get(key)`, `set(key, value)`, `remove(key)`, `regenerate()`, and `destroy()`. Values are strings, missing values return `null`, and cookies are managed automatically for HTTP/1.1, HTTP/2, and HTTP/3. Storage is in memory per server with a fixed 30-minute lifetime. See [session setup and API](docs/sessions.md) and [the runnable example](examples/sessions/App.ube).
- **HTTP/3 Support**: Configure `server.tls(certPath, keyPath)` and `server.enableHttp3(udpPort)` before `start(tcpPort)`. QUIC shares routes and sessions with TCP, supports concurrent streams, and is advertised with `Alt-Svc`. `server.http3Port()` reports the bound UDP port. See [HTTP/3 setup](docs/http3.md) and [the runnable example](examples/http3/App.ube).
- **HTTP/2 Support**: Call `server.enableHttp2()` before `start()`. With `server.tls(certPath, keyPath)`, TLS ALPN negotiates HTTP/2 or HTTP/1.1. Without TLS, cleartext HTTP/2 prior knowledge (h2c) and HTTP/1.1 share the same port. HTTP/1.1 upgrade to h2c is not supported. See [HTTP/2 setup](docs/http2.md) and [the runnable example](examples/http2/App.ube).
- **Request & Response Ergonomics**: `request.pathLong('id')`, `request.query('search')`, `request.hasBearerToken()`, `request.cookie('session')`, `request.protocol()`, `request.isHttp3()`, `Response.ok('hello')`, `Response.json(data)`.
- **Current runtime status**: `server.start(port)` blocks the calling thread and serves loopback HTTP/1.1, optional TLS/HTTP/2, and configured HTTP/3 with exact method/path route matching. Query parameters, request headers, redirects, protocol reporting, and sessions work across transports. `request.body()` reads bounded UTF-8 request payloads (1 MiB maximum) across HTTP/1.x, HTTP/2, and HTTP/3; see [request bodies and limits](docs/request-bodies.md). Middleware, static-file serving, WebSockets, and several advanced `Request` accessors remain unimplemented.

Generated class objects have tracing GC with automatic collection and explicit
`Gc.collect()`, `Gc.objects()`, and `Gc.reclaimed()` diagnostics. Runtime-resource
ownership and conservative escape pins still limit long-running reclamation;
see [memory behavior](docs/memory.md) and [stabilization status](docs/stabilization.md).

### 22. Native JSON and XML APIs (`Json`, `Xml`, `XmlOptions`, and document handles)

- **Automatic serialization**: Objects, lists, and maps provide `.toJson()` and `.toXml()` without custom serializer methods.
- **XML defaults and customization**: Class names become object roots, fields become child elements, lists use `<item>` children, maps use keyed `<entry>` elements, and nulls use `xsi:nil="true"`. `XmlOptions` customizes roots, attributes, field names, list item names, and namespaces.
- **Document parsing**: `Json.toObject(text)` and `Xml.toObject(text)` return real document handles for traversal. App classes are constructed explicitly; automatic typed reconstruction and schema-based strict/partial mapping are not implemented.
- **XML document building**: `Xml.create('root')`, `document.root().element('name').text('Alice')`, and `document.toXml()` build and serialize explicit XML structures.
- **Catchable format errors**: Parsing failures raise `JsonParseError` or `XmlParseError`; unsupported values, invalid customization, and traversal errors raise `JsonError` or `XmlError`.

See [JSON](docs/json.md), [XML mapping rules and limits](docs/xml.md), and the [runnable XML example](examples/xml/README.md).

### 23. String Literals & Text Blocks

One-line strings and character values use single quotes: `'Hello'`, `'A'`, and `''`. A character is represented as a one-character string; there is no separate `char` type. Single-quoted strings support escapes such as `\n`, `\t`, `\r`, `\`, and `\'`, and existing `${...}` interpolation. A physical line break inside a single-quoted literal is an error.

Backticks delimit raw text blocks, inline or across multiple lines. They preserve all content exactly, including whitespace, line endings, double quotes, backslashes, and `${...}`. There is no escape processing, interpolation, indentation removal, or implicit trimming. The next backtick always closes the block, even after a backslash; a backtick cannot appear inside it.

Double quotes are reserved for JSON syntax and are rejected as standalone Ube string delimiters. JSON payloads may contain ordinary double quotes inside a backtick block, then be passed to `Json.toObject`. This does not introduce a separate JSON literal type.

```ube
final ordinary = 'Hello\nWorld'
final character = 'A'

final json = `{"name":"Ube","active":true,"path":"C:\\tmp"}`

final sql = `select id, name
from users
where active = true`

final html = `<article class="welcome">
    <h1>Welcome</h1>
</article>`
```

The first block below has the value `Hello\nWorld`. The second includes a final newline. Any indentation before content or a closing delimiter is part of the value.

```ube
final message = `Hello
World`

final withTrailingNewline = `Hello
World
`
```

To include a backtick in a one-line string, write it between single quotes. Double quotes and backslashes inside raw blocks need no Ube escaping; JSON's own escaping rules still apply to JSON data.

### 24. Unified SQL (`SQL`, `SQLParams`, `SQLTransaction`, `SQLResult`, `SQLQuery`)
- **Shared database execution**: `SQL.open(url)` supports PostgreSQL, MySQL/MariaDB, and SQLite. Network drivers provide pools and prepared statements; SQLite retains one connection per client.
- **Typed bindings and results**: `SQLParams.create()` holds bound scalar values; `db.query(sql, params)` provides checked typed getters and explicit NULL handling. Values are never concatenated into SQL.
- **Transactions**: `db.begin()` reserves a connection. Use `tx.commit()` and `tx.close()` in `finally`; savepoints and rollback are supported. Failures during execution invalidate and attempt to roll back the transaction.
- **Model-free queries**: `db.table('users').select('name').where('active', '=', true).orderBy('name', 'asc').limit(20)` builds a SELECT without a model class. Execute with `all()`, `first()`, or `count()`; use `tx.table(...)` inside a transaction. See [the query builder API](docs/query-builder.md) and [runnable example](examples/query-builder/App.ube).
- **Builder/ORM foundation**: The query builder and ORM share this execution API, dialect helpers, parameters, results, and transaction context. Live-schema validation, migration diffs, streaming, and async SQL APIs are future work.
- See [the complete SQL API and limitations](docs/sql.md) and [the runnable example](examples/sql/App.ube). The existing [SQLite-specific API](docs/sqlite.md) remains available.

### 25. Typed ORM (`Orm`, `OrmModel<User>`, `OrmQuery<User>`, `OrmProjection<User>`, `OrmRows<User>`)
- **Ordinary classes**: `Orm.model(db, User, 'users')` maps scalar constructor fields to columns, with an application-assigned `int` or `long` primary key named `id`.
- **Explicit CRUD**: `users.create(user)`, `users.save(user)`, `users.get(id)`, `users.find(id)`, and `users.delete(id)` operate on typed objects. `migrate()` creates a missing table without altering existing schemas.
- **Fluent typed queries**: `users.query().where('active', '=', true).orderBy('id', 'asc').limit(20)` binds values and preserves the result model type. Literal column names and predicate types are checked during compilation.
- **Grouped OR predicates**: `orWhere(...)`, `orWhereNull(...)`, and `orWhereNotNull(...)` support alternatives. `whereGroup(query)` and `orWhereGroup(query)` snapshot another query's predicates inside parentheses, including nested groups.
- **Projections and aggregates**: `query.select('name').select('email')` creates an `OrmProjection<User>` snapshot returning ordinary `SQLResult` rows. `sum`, `avg`, `min`, `max`, and `countColumn` operate on filtered/joined rows. Empty sums, averages, and extrema return null. See [the runnable query example](examples/orm-queries/App.ube).
- **Related-table joins**: `query.join(orders, 'o', 'id', 'userId')` and `leftJoin(...)` preserve typed base-model results, support related-column filters, and return a new query. See [the join example](examples/orm-joins/App.ube).
- **Shared SQL transactions**: `users.using(tx)` pins ORM operations to a `SQLTransaction`, including raw SQL and savepoints. Changes are persisted explicitly with `save()`.
- See [the ORM API and limitations](docs/orm.md) and [the runnable example](examples/orm/App.ube).

---

## VS Code Extension & Syntax Highlighting

Ube includes an official VS Code extension in [`editors/vscode`](editors/vscode) providing `.ube` file recognition, syntax highlighting, bracket matching, string interpolation highlighting, code folding, and document formatting. Use **Format Document** (Shift+Alt+F on Linux/Windows, Shift+Option+F on macOS) to apply the structural indentation and multiline parameter and argument rules in [`FORMATTER.md`](editors/vscode/FORMATTER.md). Multiline closing parentheses stay after the final value, while structural closing braces remain on their own lines.

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

Ube prohibits semicolons as statement terminators or separators. Write statements without them. A bare `return` ends at a newline or `}`; start a return value on the same line as `return` (parentheses allow the value to continue across lines). Semicolons inside string, text-block, and character literals or comments are ordinary content.

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

#### Function values

Use `Func(parameterTypes) -> returnType` for a function value. Lambdas have typed parameters and an inferred result. Use an expression body or a Java-style block with explicit `return`:

```ube
Func(int) -> int makeAdder(int amount) {
    return (int value) -> {
        return value + amount
    }
}
```

Call the returned value with parentheses: `final addFive = this.makeAdder(5)`, then `addFive(10)`. A `Func() -> Async<int>` can be called and awaited as `first().await()`. Closures capture current local values, and captured object references remain alive through garbage collection. See [function values](docs/functions.md) and the [runnable example](examples/functions/App.ube).

Function-typed locals may use parentheses to visually group long signatures: `(Func(int, string) -> bool) validator = (int id, string name) -> id > 0`.

### 2. Structural `trait` Subtyping (No `implements` Keyword)

In Ube, `trait` provides behavioral contracts and default method bodies. Classes satisfy traits **implicitly** without an `implements` keyword:

```java
trait Printable {
    // Abstract method signature (implicit void return type)
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

class App {
    render(Printable item) {
        log(item.getHeader())
        item.printDetails()
    }

    start() {
        final user = User(1000L, 'Lem')
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
class App {
    start() {
        final bool isConnected = false
        final string input = '' // Empty string (falsy)
        final Object data = 42

        // 1. Logical negation of boolean values
        if (not isConnected) {
            log('System is disconnected') // Executes because not false is true
        }

        // 2. Truthiness negation (falsy check)
        if (not input) {
            log('No input provided') // Executes because empty string is falsy
        }

        // 3. Negated type check ('is not')
        if (data is not string) {
            log('Data is not a text string') // Executes because data is an int
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
        log('Engine started (${this._horsepower} HP)')
    }
}

// Component Class 2: Logger
class Logger(string prefix) {
    info(string message) {
        log('[${this._prefix}] ${message}')
    }
}

// Use Case 1: Component Embedding & Delegation (Replaces class Inheritance)
class Car(string model, Engine engine, Logger logger) {
    startCar() {
        this._logger.info('Starting vehicle ${this._model}...')
        this._engine.start() // Explicit forwarding delegation
    }
}

// Use Case 2: Behavior Injection via Primary Constructor (Strategy Pattern)
class DataExporter(Logger logger) {
    exportData(string data) {
        this._logger.info('Exporting data: ${data}')
    }
}

class App {
    start() {
        // Instantiate reusable components
        final logger = Logger('APP')
        final v8Engine = Engine(450)

        // Compose components into higher-level objects
        final mustang = Car('Mustang GT', v8Engine, logger)
        mustang.startCar()

        final exporter = DataExporter(logger)
        exporter.exportData('User Record #1000')
    }
}
```

### 5. App-Scoped Sharing via Constructor Injection

Create a shared instance explicitly at startup and pass it into dependent services. Constructor injection also supports separate instances; it does not make a class a singleton or prevent additional construction:

```java
class DatabaseConnection(string connectionUrl) {
    query(string sql) {
        log('Executing query on ${this._connectionUrl}: ${sql}')
    }
}

class UserService(DatabaseConnection db) {
    findUser(long id) {
        this._db.query('SELECT * FROM users WHERE id = ${id}')
    }
}

class OrderService(DatabaseConnection db) {
    findOrders(long userId) {
        this._db.query('SELECT * FROM orders WHERE user_id = ${userId}')
    }
}

class App {
    start() {
        // One shared instance for these services; additional instances are still allowed
        final sharedDatabase = DatabaseConnection('postgres://localhost:5432/main')

        // Injected into dependent service instances
        final userService = UserService(sharedDatabase)
        final orderService = OrderService(sharedDatabase)

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

class App {
    start() {
        // 1. Simple Unit Enum Usage
        final dir = Direction.North
        final dirName = switch (dir) {
            case North -> 'Heading North'
            case South -> 'Heading South'
            case East  -> 'Heading East'
            case West  -> 'Heading West'
        }
        log(dirName)

        // 2. Destructuring Payload Variant
        final status = OrderStatus.Approved(10042L, 250.00)
        final statusText = switch (status) {
            case Pending -> 'Order Pending'
            case Approved(id, amount) -> 'Order #${id} Approved for \$${amount}'
            case Rejected(reason) -> 'Order Rejected: ${reason}'
        }
        log(statusText)

        // 3. Enum Method Call
        final level = LogLevel.Error
        if (level.isCritical()) {
            log('Critical log level detected!')
        }
    }
}
```

### 7. All `switch` Control Flow Use Cases

In Ube, `switch` is a first-class expression and statement supporting 6 core pattern-matching use cases (no fall-through, no `break` required inside cases):

```java
class App {
    start() {
        // Case 1: Constant Value Matching
        final statusCode = 200
        final statusText = switch (statusCode) {
            case 200 -> 'OK'
            case 404 -> 'Not Found'
            case 500 -> 'Internal Error'
            default -> 'Unknown Code'
        }
        log(statusText)

        // Case 2: Variant Matching with Payload Destructuring
        final status = OrderStatus.Approved(10042L, 99.99)
        final orderMessage = switch (status) {
            case Pending -> 'Waiting'
            case Approved(id, amt) -> 'Approved ID: ${id}'
            case Rejected(r) -> 'Rejected: ${r}'
        }
        log(orderMessage)

        // Case 3: Type Pattern Matching ('is')
        final Object val = 'Hello World'
        final typeDesc = switch (val) {
            case is string -> 'Text String'
            case is int -> '32-bit Integer'
            default -> 'Other Type'
        }
        log(typeDesc)

        // Case 4: Guard Clauses ('if' guards)
        final score = 85
        final grade = switch (score) {
            case s if (s >= 90) -> 'Grade A'
            case s if (s >= 80) -> 'Grade B'
            case s if (s >= 70) -> 'Grade C'
            default -> 'Grade F'
        }
        log(grade)

        // Case 5: Multiple Value Or-Patterns (Comma-separated)
        final day = 'Saturday'
        final isWeekend = switch (day) {
            case 'Saturday', 'Sunday' -> true
            default -> false
        }
        log('Is Weekend: ${isWeekend}')

        // Case 6: Anonymous Object Structural Matching
        final userObj = { id: 1000, role: 'Admin' }
        final accessLevel = switch (userObj) {
            case { role: 'Admin' } -> 'Full Administrative Access'
            case { id: uid } if (uid > 0) -> 'Standard User Access'
            default -> 'Guest Access'
        }
        log(accessLevel)
    }
}
```

### 8. All `for` Loop Use Cases (With `break` and `continue`)

Ube uses `for` as its **sole loop keyword** (no `while` or `do-while`).

```java
class App {
    start() {
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
            log('Infinite loop step: ${counter}')
        }

        // Case 2: Conditional Condition Loop (Replaces while(condition))
        var remaining = 3
        for (remaining) {
            log('Remaining: ${remaining}')
            remaining = remaining - 1
        }

        // Case 3: Element Iteration Over List/Array
        final names = ['Lem', 'Rhea', 'Alex']
        for (name in names) {
            if (name == 'Rhea') {
                continue
            }
            log('Name: ${name}')
        }

        // Case 4: Index & Item Destructuring Iteration
        for (index, name in names) {
            log('${index}: ${name}')
        }

        // Case 5: Map Key-Value Destructuring Iteration
        final scores = [1000L: 95, 1001L: 88]
        for (id, score in scores) {
            log('ID ${id} -> Score ${score}')
        }

        // Case 6: Numeric Range Iteration
        for (i in 0..5) {
            if (i == 3) {
                break
            }
            log('Range index: ${i}')
        }

        // Case 7: for / else 0-Iteration Fallback Clause
        final List<string> emptyItems = []
        for (item in emptyItems) {
            log('Item: ${item}')
        } else {
            log('No items found (else executed for 0 iterations)')
        }
    }
}
```

#### Zero-iteration fallback

**Read `for ... else` as: “if the body never starts, run this fallback.”** Reaching the body once is enough to disable the fallback. It does not mean “no break” or “no matching item.”

| What happened? | Does `else` run? |
| --- | --- |
| The collection was empty | Yes |
| The condition was false before the first iteration | Yes |
| The body ran and the loop finished normally | No |
| The body ran and immediately executed `break` | No |
| The body ran but every iteration executed `continue` | No |
| A `return` or unhandled error exits the loop | No; it follows the normal return/error path |

Each example below can be placed inside `App.start()`.

**1. Empty collection: fallback runs**

```ube
final items = []
for (item in items) {
    log(item)
} else {
    log('no items')
}
```

Output:

```text
no items
```

**2. Nonempty collection: fallback does not run**

```ube
final items = [10, 20]
for (item in items) {
    log(item)
} else {
    log('no items')
}
```

Output:

```text
10
20
```

**3. Immediate break: fallback does not run**

```ube
for (item in [10, 20]) {
    log(item)
    break
} else {
    log('no items')
}
log('after loop')
```

Output:

```text
10
after loop
```

The first iteration has already started when `break` executes. There is no fallback after it.

**4. Every iteration continues: fallback does not run**

```ube
for (item in [10, 20]) {
    continue
} else {
    log('no items')
}
log('done')
```

Output:

```text
done
```

Skipping the remaining statements with `continue` still counts as entering the body.

**5. Condition loops: only the initial condition matters**

```ube
for (false) {
    log('body')
} else {
    log('initially false')
}

var remaining = 1
for (remaining > 0) {
    log(remaining)
    remaining = remaining - 1
} else {
    log('never entered')
}
log('finished')
```

Output:

```text
initially false
1
finished
```

**6. Searching: track whether a match was found explicitly**

```ube
var found = false
for (item in [1, 3, 5]) {
    if (item == 2) {
        found = true
        break
    }
}
if (not found) {
    log('no match')
}
```

Output:

```text
no match
```

A nonempty search with no match still entered its body. Use a flag and a separate `if`, rather than a loop `else`, to report search failure.


### 9. Truthy & Falsy Control Flow with Word Booleans

In Ube, conditions in `if (condition)` statements evaluate **truthiness and falsiness** without requiring explicit boolean comparisons:

```java
class App {
    start() {
        final User? optionalUser = User(1000, 'Lem', 35)
        final int attempts = 3
        final string input = '' // Empty string (falsy)

        // 1. Truthy object evaluation & flow-sensitive narrowing
        if (optionalUser) {
            // Inside this block, 'optionalUser' is automatically narrowed from User? to non-null User
            log('User loaded: ${optionalUser.name()}')
        }

        // 2. Non-zero number (truthy) & empty string (falsy) using word booleans
        if (attempts and not input) {
            log('System ready: ${attempts} attempts remaining and no pending input.')
        }
    }
}
```

### 10. Word-Based Logical Operators (`not`, `and`, `or`)

Ube uses `not`, `and`, and `or` for boolean logic. Precedence is `not`, then `and`, then `or`; parentheses make grouping explicit.

```ube
class App {
    start() {
        final bool isReady = true
        final bool isConnected = false

        if (not isConnected) {
            log('System is currently disconnected')
        }

        final bool bothActive = isReady and isConnected // false
        final bool eitherActive = isReady or isConnected // true
        final bool neitherActive = not (isReady or isConnected) // false

        log(bothActive)
        log(eitherActive)
        log(neitherActive)
    }
}
```

### 11. Concurrency Runtime and Current Language Boundary

For Ube task ownership, cancellation, safe sharing, and settled-result examples,
see [Concurrency in Ube](docs/concurrency.md). Async task bodies and the program's
application scope cancel and join unfinished child tasks before completing. No new
syntax is required. `Async.allSettled` now returns `TaskOutcomes`, with explicit
success/error status and checked scalar accessors instead of replacing failures
with zero. Built-in list/map operations and generated scalar field accesses are
synchronized; multi-step updates still require coordination.

The runtime uses a shared multithreaded Tokio executor. Eligible async methods
now compile to resumable native poll functions: pending awaits release their
worker, and a heap frame preserves locals and rooted references. This first slice
covers channel/task waits and known nonblocking control flow. Methods containing
ordinary calls, object construction, nested async submissions, or potentially
blocking runtime operations retain generated callbacks on Tokio's blocking pool.
See [yielding support and limits](docs/concurrency.md#yielding-native-methods-first-supported-slice)
and the [benchmark](benchmarks/README.md). `await` blocks a synchronous Ube caller; the Rust API's `join` is available to async Rust callers. `Task<T>` carries `Result<T, TaskError>` rather than turning failures into successful values. `T` must be cloneable and thread-safe because task results can be observed by multiple waiters.

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

The compiler now schedules calls to declared `async` methods and infers the awaited result type. Async method results are transported through a two-word ABI value (64-bit integer/boolean/decimal bits or pointer, plus presence and scalar-kind metadata); compiler-generated callbacks own their capture context, and task handles are cleaned up with the runtime. `Async.all`, `Async.any`, `Async.race`, and `Async.allSettled` also return tasks; the language wrappers cancel and join remaining workers before completing.

The compiler transports async method throws and runtime task failures to an enclosing `try`/`catch`; use `err.message()` to inspect the message and `err.stacktrace()` for captured Ube call frames. Unhandled errors also print those frames. See [error stack traces](docs/stacktraces.md) for location and async limits. Awaiting a failure without a matching `catch` propagates it from an async method or `async` expression to its awaiter; anywhere else the program stops with `Ube error: unhandled exception ...` (exit code 1). Generated async methods poll cancellation at basic-block boundaries and raise a catchable cancellation error so cleanup runs; a currently executing indivisible operation cannot be interrupted. `async expr` captures visible locals when submitted and evaluates the expression on a worker. Language channels support explicit element types (`Channel<T>(capacity)`) and `close()`; `send(value)` returns `Task<bool>` and `receive()` returns `Task<T>`, with the channel wait performed on Tokio. `send` completes with false if the channel is closed; after buffered values are drained, `receive` fails with a catchable channel-closed error. `task.timeout(ms)` wraps any task (including a channel operation) with a deadline; `Async.race` waits on several channels at once. The language channel bridge supports primitive values, pointer-sized values, and nullable scalars (`int?`, `long?`, `byte?`, `bool?`, `decimal?`). Scalar presence is stored separately, so null remains distinct from zero and false across locals, calls, fields, captures, tasks, and channels. Task errors and channel closure use the exception path, not a null payload. Untyped `List`/`Map` storage still uses single-word elements: storing nullable scalars or tasks with nullable scalar results is diagnosed, including lists passed to task combinators. Typed nullable collections/combinators remain Phase 1 follow-up work. Compiler and runtime must be rebuilt together for the new ABI.

Cancellation is delivered once per worker callback, either at a cancellation poll or while awaiting. After delivery, subsequent awaits, including those in `finally`, wait normally so async cleanup can finish. Errors and timeouts from cleanup operations still propagate; the original cancellation remains pending unless handled or replaced by a cleanup failure.

### 12. Built-in Collections (`List<T>`, `Map<K, V>`, Indexing & Bounds Protection)

```java
class App {
    start() {
        // 1. List literal, indexing, and methods
        final numbers = [10, 20, 30]
        numbers.add(40)
        log('List len: ${numbers.len()}, first element: ${numbers[0]}')

        // 2. Map literal, key lookup, and methods
        final scores = [1000L: 95, 1001L: 88]
        log('Map len: ${scores.len()}, score for 1000L: ${scores[1000L]}')

        // 3. Out-of-bounds error handling
        try {
            log(numbers[10]) // Out of bounds!
        } catch (Error err) {
            log('Caught error: ${err.message()}')
        }
    }
}
```

### 13. Structured Exception Model (`try` / `catch` / `finally` & `throw`)

Throw a string for a `GenericError`, or throw an object that provides `string message()`. Error classes use structural typing and require no inheritance. Concrete catches match the original class; trait catches match the methods that the object provides. Handlers run in source order, and `catch (Error err)` is the general fallback.

```ube
class RequestError(string message, int code)

class App {
    start() {
        try {
            throw RequestError('Invalid user request', 400)
        } catch (RequestError err) if (err.code() == 400) {
            log(err.message())
            log(err.code())
        } catch (Error err) {
            log(err.message())
        } finally {
            log('Cleanup executed')
        }
    }
}
```

A catch condition is evaluated after the error has been matched and narrowed to the catch type. If the condition is false, matching continues with the next handler. Put general handlers last; the compiler rejects handlers already covered by an earlier unconditioned catch.

`try` requires at least one `catch` or a `finally` block. Cleanup runs on success, propagated errors, `return`, `break`, and `continue`. If cleanup throws while an error is pending, its error becomes active and the original is retained as suppressed. A return from `finally` overrides a pending return.

Failures propagate through method calls and `await`; async errors retain their original class and fields. Unhandled errors print their type, message, and recorded location, then exit with status 1.

File and directory failures are catchable, including `FileNotFoundError`, `PermissionError`, and a fallback `FileError`. Empty file content and stream EOF remain successful results. Invalid or closed streams raise errors. Server startup failures, including invalid ports and bind failures, raise catchable `GenericError` values.

```ube
class App {
    start() {
        final file = File.open('data.csv')
        try {
            log(file.read())
        } catch (FileNotFoundError err) {
            log('File does not exist')
        } catch (Error err) {
            log(err.message())
        } finally {
            file.close()
        }
    }
}
```

### 14. Unified `DateTime` (`Instant` + Timezone) & `Duration` Standard Library

```java
class App {
    start() {
        // 1. Instant + Timezone Creation
        final manila = DateTime.of(2026, 8, 31, 17, 0, 'Asia/Manila')
        final tokyo = manila.in('Asia/Tokyo')

        // 2. Instant Invariance (Timestamps match 100%, displayed clock hours differ)
        log('Timestamps equal: ${manila.timestamp() == tokyo.timestamp()}') // true!
        log('Manila hour: ${manila.hour()}, Tokyo hour: ${tokyo.hour()}')     // 17 vs 18

        // 3. Unix Timestamps & Formatting
        final dt = DateTime.fromUnix(1788159600)
        log('Formatted: ${dt.format('yyyy-MM-dd HH:mm:ss')}')

        // 4. Time Amounts with Duration
        final start = DateTime.now()
        final timeout = Duration.seconds(30)
        final expires = start.addDuration(timeout)
        log('Expires in: ${expires.diffSeconds(start)} seconds')
    }
}
```

### 15. Financial Standard Library (`Money`, `Currency`, `Exchange`, `Percent`, `Rate`)

```java
class App {
    start() {
        // 1. Immutable Financial Money Creation
        final price = Money.php(1250.75)
        final dollar = Money.usd(100.00)
        log('Price amount: ${price.amount()}, Currency: ${price.currency().code()}')

        // 2. Currency-Safe Arithmetic
        final shipping = Money.php(100.00)
        final total = price.add(shipping)
        log('Total PHP amount: ${total.amount()}')

        // 3. Explicit Currency Conversion via Exchange
        final converted = Exchange.convert(dollar, Currency.PHP, 58.50)
        log('100 USD converted to PHP: ${converted.amount()}')

        // 4. Financial Percentages & Taxes
        final vat = Percent(12)
        final tax = total.multiply(vat)
        log('VAT Tax amount: ${tax.amount()}')
    }
}
```

Currency mismatches can be handled without terminating the process:

```ube
try {
    final total = Money.php(10.0).add(Money.usd(5.0))
} catch (CurrencyMismatchError error) {
    print(error.message())
}
```

The same error applies to `subtract()`. Normal exception propagation, `finally`,
and `catch (Error error)` work with this error. See [financial runtime behavior
and audit notes](docs/financial.md).

### 16. Native Filesystem & `finally` Cleanup

```java
class App {
    start() {
        // 1. One-shot static conveniences
        File.write('logs/app.txt', 'Server initialized\n')
        final content = File.read('logs/app.txt')
        log('Log content: ${content}')

        // 2. Object-oriented handle with cleanup
        final file = File.open('data.csv')
        try {
            final data = file.read()
            log('Read ${data.len()} bytes')
        } catch (Error err) {
            log('File error: ${err.message()}')
        } finally {
            file.close()
        }
    }
}
```

### 17. High-Performance Native Web Server (`HTTP/1.1`, `HTTP/2`, `HTTP/3 QUIC`)

```java
class App {
    start() {
        final server = Server()

        server.get('/', request -> Response.ok('Hello from Ube!'))

        server.start(8080)
    }
}
```

### 18. Native JSON and XML Serialization

Each class belongs in its matching source file:

```ube
// KitchenOrderScan.ube
class KitchenOrderScan(string restaurantID, string orderType)
```

```ube
// App.ube
import KitchenOrderScan

class App {
    start() {
        final json = Json.toObject('{"restaurantID":"0603","orderType":"DineIn"}')
        final scan = KitchenOrderScan(json.getString('restaurantID'), json.getString('orderType'))
        Console.printLine(scan.toJson())

        // Fields become elements by default.
        Console.printLine(scan.toXml())
        // <KitchenOrderScan><restaurantID>0603</restaurantID><orderType>DineIn</orderType></KitchenOrderScan>

        // Choose a root name and store one field as an attribute.
        final options = XmlOptions().root('order').attribute('restaurantID')
        final xml = scan.toXml(options)
        final document = Xml.toObject(xml)
        Console.printLine(document.root().getAttribute('restaurantID')) // 0603
        Console.printLine(document.root().child('orderType').text())    // DineIn

        Console.printLine([1, 2].toXml())
        // <list><item>1</item><item>2</item></list>
        Console.printLine(['mode': 'dine-in'].toXml())
        // <map><entry key="mode">dine-in</entry></map>
    }
}
```

`Xml.toObject(xml)` returns an `XmlDocument`, not a reconstructed `KitchenOrderScan`. See [XML](docs/xml.md) for namespaces, nulls, escaping, attributes, traversal, and resource limits.

The automated XML end-to-end suite compiles and executes native Ube programs, checks the shipped example through CLI `run` and `build`, and exchanges XML files between independent writer/reader processes. It also verifies corrupted-file failures, customization, and async behavior:

```bash
cargo build -p ube-runtime -p ube-compiler
./target/debug/ube-compiler test examples/xml/tests
cargo test -p ube-compiler --test xml_e2e
```

### 19. Zero Initialization & Explicit Construction

A typed variable declared without an initializer receives a zero value. This does not evaluate constructor defaults or run user code. Explicit constructor calls still evaluate defaults. The runtime initializes the entry-point `App` object using the same zero-initialization path, so its constructor defaults are also ignored at startup. Put configuration and resource creation explicitly inside `start()`.

```ube
class User(string name = 'Guest', var int age = 18)

class App {
    start() {
        User zero
        log(zero.name()) // Empty string
        log(zero.age())  // 0

        final configured = User()
        log(configured.name()) // Guest
        log(configured.age())  // 18

        User? absent
        log(absent == null) // true
    }
}
```

| Declared type | Zero value |
| --- | --- |
| `int`, `long`, `byte`, `decimal` | `0` |
| `bool` | `false` |
| `string` | `''` |
| Nullable type, such as `User?` | `null` |
| User-defined class | A new object with recursively zero-initialized fields |

Each declaration creates independent class values; it does not share a singleton. Non-nullable nested class fields are recursively allocated without evaluating defaults. Recursive non-nullable layouts such as `class Node(Node next)` cannot be zero-initialized: use a nullable link (`Node? next`) to terminate the layout. Runtime objects such as servers, channels, and streams, and variables typed as traits, require an explicit initializer or a nullable declaration. A plain declaration never calls their creation APIs or acquires their resources.

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
Ube check src/App.ube

# 3. Fast compile & execute immediately (1-step execution)
Ube run src/App.ube

# 4. Build standalone native release executable (with -O3 & LTO optimization)
Ube build --release src/App.ube -o my_app
./my_app

# 5. (Optional) Inspect generated textual LLVM IR for compiler debugging
Ube emit-llvm src/App.ube -o App.ll
```

---

## Architecture & Project Structure

For detailed compiler pipeline design, module layout, object layout headers, memory management, and compiler architecture, see:
- [Compiler & Workspace Architecture (`ARCHITECTURE.md`)](ARCHITECTURE.md#2-project--workspace-directory-structure)

---


## SQLite

SQLite is bundled with the runtime. Open a file with `SQLite.open('app.db')` or
use `SQLite.open(':memory:')` for an in-memory database. Connections support
prepared statements, typed and named parameter binding, transactions, SQL
scripts, BLOBs via hex strings, read-only mode, busy timeouts, and backup/restore.
Results provide typed getters and column metadata; failures raise `SQLiteError`.

This complete example saves, reads, updates, and deletes one record:

```ube
class App {
    start() {
        // Store records in a local file that persists between runs.
        final db = SQLite.open('items.db')
        try {
            db.execute('CREATE TABLE IF NOT EXISTS items (id INTEGER PRIMARY KEY, name TEXT NOT NULL)')

            // Save a new record. Bind values instead of interpolating SQL.
            final save = db.prepare('INSERT INTO items (name) VALUES (?)')
            try {
                save.bindString(1, 'Ube')
                save.execute()
            } finally {
                save.close()
            }
            final id = db.lastInsertRowId()

            // Read the record by its generated ID.
            final read = db.prepare('SELECT id, name FROM items WHERE id = ?')
            try {
                read.bindLong(1, id)
                final rows = read.query()
                try {
                    for (rows.next()) {
                        log(rows.getLong(0))
                        log(rows.getString(1)) // Ube
                    }
                } finally {
                    rows.close()
                }
            } finally {
                read.close()
            }

            // Update the same record.
            final update = db.prepare('UPDATE items SET name = ? WHERE id = ?')
            try {
                update.bindString(1, 'Purple yam')
                update.bindLong(2, id)
                log(update.execute()) // 1 affected row
            } finally {
                update.close()
            }

            // Delete only the record created above.
            final delete = db.prepare('DELETE FROM items WHERE id = ?')
            try {
                delete.bindLong(1, id)
                log(delete.execute()) // 1 affected row
            } finally {
                delete.close()
            }
        } finally {
            db.close()
        }
    }
}
```

See the [complete SQLite API reference](docs/sqlite.md) and
[runnable example](examples/sqlite/App.ube).

---

## Observability

Ube has optional metrics and health endpoints on a separate localhost listener inside the application process:

```ube
Observability.start(9090)
Health.ready(true)
Metrics.increment('orders_created_total')
Metrics.gauge('orders_pending', 12)
Metrics.record('order_processing_seconds', 0.025)
```

`GET /health/live`, `GET /health/ready`, and `GET /metrics` expose liveness, readiness, and Prometheus metrics. Readiness starts false (HTTP 503). Runtime measurements include uptime, task counts, GC counts, and resident memory on Linux. The listener is disabled until explicitly started, binds only to `127.0.0.1`, and stops during runtime cleanup. Call `Observability.stop()` to stop it earlier. Keep the application running after starting it.

See [Observability](docs/observability.md) for the API, validation rules, supported measurements, and current limits.

## Command-line arguments

Keep `start()` parameterless and read application arguments through `Console.args()`:

```ube
// App.ube
class App {
    start() {
        final args = Console.args()
        for (arg in args) {
            Console.printLine(arg)
        }
    }
}
```

```bash
ube run App.ube -- hello 'two words' --verbose
# Or run a built executable:
./App hello 'two words' --verbose
```

`Console.args()` returns a read-only `ConsoleArgs` collection, excluding the executable name. Use `args.len()`, `args.get(0)`, or `args[0]`; elements are strings. Empty arguments, whitespace, Unicode, and argument order are preserved. Invalid indexes raise catchable errors. With no arguments, the collection is empty. The CLI forwards everything after `--` unchanged as application arguments; with a project manifest, `ube run -- hello` works too. Native argument bytes that are not valid UTF-8 become Unicode replacement characters. The API exposes arguments only; it does not parse flags or read environment variables.

## Persistent JSON records with KeyValueStore

`KeyValueStore` retains records until explicitly changed or deleted. It has no cache TTL, eviction, or entry budget, and reads records from disk as needed. Durable SQLite WAL writes are the default; buffered writes require `openBuffered` explicitly.

```ube
// Inside an application method with a data-class instance named profile:
final store = KeyValueStore.open('data/records.db')
try {
    store.set('user:42', profile.toJson())
    final saved = Json.toObject(store.get('user:42') ?? '{}')
    Console.printLine(saved.getString('name'))
} finally {
    store.close()
}
```

Create the parent directory first. Concrete data objects have a default `.toJson()` method for their declared fields, nested data objects, lists, maps, scalars, and nullable values. Cycles, non-finite numbers, runtime resources, and unsupported map keys raise clear errors rather than silently losing data. JSON text is stored; automatic reconstruction into a Ube class is not implemented.

The native end-to-end suite covers 5,000 records checked after reopen, four tasks writing 1,000 additional records and performing 1,000 atomic increments, three forced-kill runs recovering every acknowledged 1,000-record prefix, garbage collection during stress, and an injected write failure preserving the old record. These are correctness/process-crash tests, not physical power-loss or production-capacity claims.

Read the [KeyValueStore reference](docs/key-value-store.md), [JSON serialization rules](docs/json.md), and [complete runnable object example](examples/key-value-store/README.md).

## Shared and persistent cache

Ube has one built-in `Cache` API for local memory, durable disk-backed storage, and clients of a shared server. Keys and values are strings; missing or expired entries return `null`.

```ube
// App.ube
class App {
    start() {
        final cache = Cache.memory(67108864)
        try {
            cache.set('greeting', 'hello', 60000)
            Console.printLine(
              cache.get('greeting') ?? 'missing')
            cache.add('visits', '0', 0)
            Console.printLine(cache.increment('visits', 1))
            Console.printLine(cache.delete('greeting'))
        } finally {
            cache.close()
        }
    }
}
```

This prints `hello`, `1`, and `true`. TTLs are milliseconds; zero means no time-based expiration. Reads do not extend the TTL. `add` inserts only when absent, and `increment` is atomic across tasks and shared-server clients.

| Backend | Creation | Lifetime |
| --- | --- | --- |
| Embedded memory | `Cache.memory(bytes)` | Data lives inside the application process |
| Embedded durable | `Cache.open(path, bytes)` | Unexpired, unevicted entries recover from SQLite WAL on reopen |
| Embedded buffered | `Cache.openBuffered(path, bytes)` | Persistent atomic writes with possible recent-write loss after host/power failure |
| Shared client | `Cache.connect('127.0.0.1:6380', token)` | Independent applications share a running cache server |

A local cache can also call `cache.listen(address, token)` to expose its store from the application process. That listener stops when its application exits. Use the standalone CLI when cache and application restarts should be independent.

### Start a standalone cache

From the example project, with a matching Ube version installed:

```bash
cd examples/cache
mkdir -p data
read -rsp 'Cache token (at least 16 bytes): ' UBE_CACHE_TOKEN
printf '\n'
export UBE_CACHE_TOKEN
ube cache --bind 127.0.0.1:6380 --memory 67108864 \
  --disk data/shared.db --metrics 9090
```

In another terminal, enter the same directory and supply/export the same token. The two applications below communicate through that cache:

```bash
ube run writer/App.ube
ube run reader/App.ube
```

On a fresh cache, the writer prints `1`; the reader prints `hello from another process` and `1`. The greeting expires after 60 seconds. Restarting either application leaves the server running. Restarting the persistent server retains the visit counter and any greeting whose TTL has not elapsed and which has not been evicted.

The default cache listener is localhost port 6380. `--metrics 9090` exposes `/health/live`, `/health/ready`, and `/metrics` on a separate localhost HTTP listener. Cache metrics include hits, misses, writes, evictions, expirations, item count, and accounted bytes.

### Cache examples and reference

[The runnable example guide](examples/cache/README.md) includes source-checkout build instructions, exact commands, and expected outputs for:

- [Memory operations and LRU eviction](examples/cache/memory/App.ube)
- [Persistence across application restarts](examples/cache/persistent/App.ube)
- [Expiration during downtime](examples/cache/ttl/App.ube)
- [Concurrent atomic counters](examples/cache/counter/App.ube)
- [Separate writer](examples/cache/writer/App.ube) and [reader](examples/cache/reader/App.ube) applications
- [Embedded sharing with health and metrics](examples/cache/server/App.ube)
- [Cache-aside source fallback and error handling](examples/cache/cache-aside/App.ube)

All cache calls are synchronous and can raise catchable errors. The byte limit accounts for entries, not total process RSS; entries can be evicted even with TTL zero. Persistent data should live outside `.build` when it must survive `ube clean`. The shared protocol authenticates requests but has no built-in TLS; use an encrypted tunnel or TLS proxy across untrusted networks. It is Ube-specific, not Memcached wire-compatible.

Read the [complete API and semantics](docs/cache.md), [application recipes](docs/cache-recipes.md), [deployment/recovery/troubleshooting guide](docs/cache-deployment.md), and [wire protocol](docs/cache-protocol.md).

---

## Documentation Links

- [Documentation index and guides](docs/README.md)
- [Cache reference and examples](docs/cache.md)
- [Language Specification Baseline (`LANGUAGE_SPEC_DRAFT.md`)](LANGUAGE_SPEC_DRAFT.md)
- [Compiler & Runtime Architecture (`ARCHITECTURE.md`)](ARCHITECTURE.md)
- [Phased Milestones & Roadmap (`MILESTONES.md`)](MILESTONES.md)

---

## License

Ube is open-source software licensed under the [MIT License](LICENSE).
