# Ube Unresolved Design Decisions & Recommendations

This document analyzes the 40 unresolved language features for **Ube**. For each feature, alternatives and tradeoffs are evaluated, followed by a concrete design recommendation.

---

### 1. Final Nullable-Type Syntax
- **Alternatives**: (A) Postfix `Type?` syntax. (B) Generic `Option<T>` wrapper class.
- **Tradeoffs**: Option wrappers introduce allocation and verbosity; `Type?` is clean and directly integrates with flow-sensitive narrowing.
- **Recommendation**: Adopt **Postfix `Type?`** (`User?`).

### 2. Class Equality vs. Object Identity
- **Alternatives**: (A) `==` evaluates structural value equality; `===` evaluates pointer identity. (B) `==` evaluates pointer identity.
- **Tradeoffs**: Pure OOP records intuitively represent data; structural default for `==` prevents bugs.
- **Recommendation**: **`==` performs structural equality** by default; **`===` performs reference identity**.

### 3. Equality & Hashing for Classes Containing `var` Fields
- **Alternatives**: (A) Exclude `var` fields from generated `equals`/`hash`. (B) Include `var` fields in structural `equals`/`hash` and document mutability in collections.
- **Tradeoffs**: Mutating an object while stored in a hash map can disrupt bucket indexing.
- **Recommendation**: **Include `var` fields in equality/hashing**, and issue a static warning when using a class with `var` fields as a hash map key.

### 4. Error Handling Model
- **Alternatives**: (A) Exception hierarchy (`try`/`catch`/`throw`). (B) Algebraic `Result<T, E>` union type with syntax sugar.
- **Tradeoffs**: Exceptions break control flow and require inheritance/interfaces. `Result` preserves pure OOP without inheritance.
- **Recommendation**: **`Result<T, E>` union types** with an explicit error-propagation operator `?`.

### 5. `try`, `throw`, `catch`, and `finally`
- **Alternatives**: (A) Include standard exception keywords. (B) Omit `try`/`catch`/`throw` in favor of `Result<T, E>` and block-scoped resource handling.
- **Tradeoffs**: Exceptions violate the "no inheritance" rule for exception classes.
- **Recommendation**: **Prohibit `try`, `throw`, `catch`, and `finally`**.

### 6. Catch-All Errors Without Inheritance
- **Alternatives**: (A) Tagged error classes. (B) Structural error types.
- **Tradeoffs**: Tagged error types allow matching without requiring nominal interface hierarchies.
- **Recommendation**: Use **Structural Error Classes** with pattern matching on error tags.

### 7. Resource Management
- **Alternatives**: (A) Java-style `try-with-resources`. (B) Go-style `defer` statement.
- **Tradeoffs**: `defer` runs cleanup at method exit without complex block nesting.
- **Recommendation**: Adopt **`defer` statements** (`defer file.close()`).

### 8–13. Collections Architecture (Arrays, Lists, Maps, Sets, Slices)
- **Alternatives**: (A) Interface hierarchy (`List`, `Collection`). (B) Concrete built-in generic classes (`Array<T>`, `List<T>`, `Map<K, V>`, `Set<T>`, `Slice<T>`).
- **Tradeoffs**: Since Ube has no interfaces, collections must be concrete generic classes.
- **Recommendation**: Provide built-in generic classes: **`Array<T>`**, **`List<T>`**, **`Map<K, V>`**, **`Set<T>`**, and **`Slice<T>`**.

### 14. Collection Literals
- **Alternatives**: (A) `[1, 2, 3]` for `List<T>`, `{"a": 1}` for `Map<K, V>`. (B) Constructor calls `List(1, 2, 3)`.
- **Tradeoffs**: Literals improve readability.
- **Recommendation**: Support **`[1, 2, 3]`** for `List` and **`[key: value]`** for `Map`.

### 15–16. Enums & Closed Tagged Alternatives
- **Alternatives**: (A) Java-style enum constants. (B) Algebraic Sum Types (`class` tagged unions).
- **Tradeoffs**: Algebraic sum types allow payloads without requiring inheritance.
- **Recommendation**: Support **Sum Classes** (`class Status { case Pending; case Approved(long id) }`).

### 17. Pattern Matching
- **Alternatives**: (A) Type tests only. (B) Structural destructuring in `switch`.
- **Tradeoffs**: Structural destructuring makes `switch` powerful for classes and anonymous objects.
- **Recommendation**: Support **Structural Destructuring in `switch`**.

### 18–19. Callbacks & Lambda Syntax
- **Alternatives**: (A) Anonymous classes. (B) Single-method lambda syntax `{ (a, b) -> a + b }`.
- **Tradeoffs**: Lambdas are lowered by the compiler into synthetic single-method closure classes.
- **Recommendation**: Adopt **Brace Lambdas `{ (x) -> x + 1 }`**.

### 20. Method References
- **Alternatives**: (A) `this.methodName`. (B) Not supported.
- **Tradeoffs**: `this.methodName` allows easy passing of instance methods as callbacks.
- **Recommendation**: Support **Method References `this.methodName`**.

### 21. Method Overloading
- **Alternatives**: (A) Disallow overloading (distinct method names required). (B) Allow parameter-type overloading.
- **Tradeoffs**: Disallowing overloading simplifies symbol resolution and API clarity (like Go).
- **Recommendation**: **Prohibit method overloading**. Every method in a class must have a unique name.

### 22. Operator Overloading
- **Alternatives**: (A) Allow arbitrary operator overloading. (B) Prohibit operator overloading.
- **Tradeoffs**: Operator overloading reduces code predictability.
- **Recommendation**: **Prohibit operator overloading**.

### 23. Default & Named Constructor Arguments
- **Alternatives**: (A) Positional arguments only. (B) Support default parameter values and named argument calls (`User(id: 10, name: "Lem")`).
- **Tradeoffs**: Named arguments eliminate the need for secondary constructors or builder patterns.
- **Recommendation**: Support **Default and Named Constructor Arguments**.

### 24. Secondary Constructors
- **Alternatives**: (A) Allow multiple constructor declarations. (B) Primary constructor only with default arguments and factory methods.
- **Tradeoffs**: Single primary constructor keeps the unified `class` model crisp.
- **Recommendation**: **Prohibit secondary constructors** in favor of factory instance methods.

### 25. Constructor Validation
- **Alternatives**: (A) Validation inside method bodies. (B) An `init` block inside `class`.
- **Tradeoffs**: `init` blocks run immediately after primary constructor assignment.
- **Recommendation**: Support **`init { ... }` validation blocks**.

### 26. Annotations
- **Alternatives**: (A) Full runtime annotation reflection. (B) Compile-time attributes (`@deprecated`, `@test`).
- **Tradeoffs**: Compile-time attributes avoid runtime reflection overhead.
- **Recommendation**: **Compile-time attributes only** (`@attribute`).

### 27–28. Reflection & Serialization
- **Alternatives**: (A) Heavy runtime reflection API. (B) Compiler-generated serialization serializers without reflection.
- **Tradeoffs**: Compiler-generated serializers preserve direct LLVM execution speed and binary size optimization.
- **Recommendation**: **Compile-time generated serialization** (`User.toJSON()`).

### 29–31. Native Interoperability & C ABI
- **Alternatives**: (A) Complex C++ bindings. (B) `class` wrapper with `extern` method markers and `unsafe` blocks.
- **Tradeoffs**: Explicit `unsafe` blocks isolate raw pointer manipulation.
- **Recommendation**: Provide **`extern` C-ABI wrappers** and **`unsafe { ... }` blocks**.

### 32. Compile-Time Constants
- **Alternatives**: (A) Top-level `const`. (B) `const` class fields.
- **Tradeoffs**: Must fit pure OOP without static fields.
- **Recommendation**: Class-scoped **`const` value getters** (`const int MaxSize = 100`).

### 33. Documentation Comments
- **Alternatives**: (A) `/** ... */`. (B) `///` triple-slash markdown comments.
- **Tradeoffs**: Triple-slash markdown is modern standard (Rust/Dart style).
- **Recommendation**: Use **`///` Triple-Slash Markdown Comments**.

### 34. Testing Syntax
- **Alternatives**: (A) Test functions. (B) Test classes with `@test` annotated instance methods.
- **Tradeoffs**: Fits pure OOP paradigm.
- **Recommendation**: **Test Classes with `@test` methods**.

### 35. Semicolon Policy
- **Alternatives**: (A) Mandatory. (B) Prohibited. (C) Optional with newline insertion rules.
- **Tradeoffs**: Optional semicolons with Go/Kotlin-style automatic newline insertion provide clean syntax.
- **Recommendation**: **Optional Semicolons** (newlines act as implicit statement terminators).

### 36. Formatting Rules
- **Alternatives**: (A) Unformatted. (B) Official `ube fmt` tool.
- **Tradeoffs**: Eliminates formatting disputes across teams.
- **Recommendation**: **Canonical `ube fmt` tool** enforcing 4-space indents and standard brace placement.

### 37. Source Encoding
- **Alternatives**: (A) Configurable encodings. (B) Strict UTF-8.
- **Tradeoffs**: UTF-8 is universal standard.
- **Recommendation**: **Strict UTF-8**.

### 38. Conditional Compilation
- **Alternatives**: (A) C preprocessor `#ifdef`. (B) Target-filter attributes `@target(os = "linux")`.
- **Tradeoffs**: Attribute-based target filtering is clean and statically checked.
- **Recommendation**: **`@target(...)` attributes**.

### 39. Standard Library Architecture
- **Alternatives**: (A) Monolithic standard library. (B) Modular packages (`ube.io`, `ube.sys`, `ube.math`, `ube.json`).
- **Tradeoffs**: Modular packages support dead-code elimination.
- **Recommendation**: **Modular Package Namespaces**.

### 40. Global State Management
- **Alternatives**: (A) Static variables. (B) Prohibit global state; require application-scoped dependency injection instances.
- **Tradeoffs**: Eliminates data races and hidden state mutations.
- **Recommendation**: **Prohibit global state**. State must be passed via class instances.
