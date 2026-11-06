# Ube Compiler & Runtime Architecture

This document defines the system architecture, compiler pipeline, directory layout, memory management model, bootstrapping design, and host implementation language evaluation for the **Ube** programming language compiler.

---

## 1. Compiler Pipeline Architecture

The current CLI lexes and parses Ube, runs semantic checks, builds typed IR, emits textual LLVM IR, and invokes `clang` for native compilation and linking. The diagram below also includes planned resolver, LLVM binding, optimization, and linker integrations; it is not a claim that every stage is implemented.

```text
Ube Source Code (.ube)
         ↓
  [ Lexer Scanner ]  → Tokens with Line/Column Spans
         ↓
 [ Parser (Pratt) ]  → Abstract Syntax Tree (AST)
         ↓
[ Package & Symbol Resolver ] → Symbol Tables & Scope Hierarchies
         ↓
[ Semantic & Type Analyzer ] → Fully Typed AST & Flow Narrowing
         ↓
 [ Typed Ube IR Builder ] → Control Flow Graph (CFG) in SSA Form
         ↓
  [ LLVM IR Generator ] → Textual .ll / Inkwell In-Memory LLVM Module
         ↓
 [ LLVM Optimizer Pass Manager ] → Target-Specific LLVM IR Optimization (-O2/-O3)
         ↓
[ LLVM Code Generator (llc) ] → Native Target Object Code (.o / .obj)
         ↓
  [ Native Linker (lld) ] → Bundled with ube-runtime
         ↓
   Native Executable
```

---

## 2. Project & Workspace Directory Structure

The Ube workspace is structured as a multi-crate Rust project managed by Cargo, with clear architectural separation between compiler frontend, SSA IR transformation, LLVM code generation, and the native C-ABI runtime.

```text
Ube/
├── Cargo.toml                      # Workspace root definition
├── README.md                       # Main project README
├── LANGUAGE_SPEC_DRAFT.md          # Baseline language specification
├── ARCHITECTURE.md                 # Compiler & runtime architecture
├── MILESTONES.md                   # 14 phased milestones & roadmap
├── compiler/                       # Crate: ube-compiler
│   ├── Cargo.toml                  # Compiler crate manifest
│   └── src/
│       ├── main.rs                 # ube CLI driver (check, emit-ir, emit-llvm, build)
│       ├── lib.rs                  # Compiler library exports
│       ├── diagnostics/            # Span tracking & colorized diagnostic emitter
│       ├── lexer/                  # Single-quoted strings/characters & raw backtick text blocks
│       ├── parser/                 # Unified class Pratt parser & AST nodes
│       ├── resolver/               # Scope & symbol resolution
│       ├── types/                  # Type system representations
│       ├── semantics/              # Types, class-private fields, accessor calls & zero-initialization checks
│       ├── ir/                     # CFG, shared zero initialization & exception routing
│       └── llvm/                   # Textual LLVM IR emitter & C-main builder
├── runtime/                        # Crate: ube-runtime (Native C-ABI static library)
│   ├── Cargo.toml                  # Runtime crate manifest
│   └── src/
│       ├── lib.rs                  # C-ABI runtime init & cleanup
│       ├── memory.rs               # Tracked heap allocations & shutdown cleanup
│       ├── strings.rs              # UTF-8 string runtime primitives
│       ├── json.rs                 # Shared typed data traversal and JSON encoding
│       ├── xml.rs                  # XML documents, options, parsing, and encoding
│       ├── io.rs                   # File, stream, and server I/O wrappers
│       ├── logging.rs              # Severity filtering and Console/Log output
│       ├── exceptions.rs           # Pending errors, structural identity & suppression
│       ├── concurrency.rs          # Tokio tasks/channels & one-shot cancellation
│       ├── gc.rs                   # Precise tracing heap, method roots, and pointer barriers
│       └── collections.rs          # Runtime collection array/map representations
└── standard-library/               # Core Ube standard library packages
    └── Console.ube               # Standard library prelude definitions
```

---

## Nullable Payload ABI

The compiler/runtime task and channel bridge uses `#[repr(C)] AbiValue { payload: i64, meta: u64 }`, represented in LLVM as `{ i64, i64 }`. Nullable scalar values use metadata 0 for absent and 1 for present; non-null scalar/pointer values use metadata 1. Nullable references retain their null-pointer representation. Static types determine how payload bits are decoded; metadata is not a dynamic type ID.

Within generated code, nullable scalars use `{ i1, T }` with explicit presence and an inline scalar payload. Locals, fields, accessors, calls/returns, async captures, and awaited results use this representation. Boundary encoding preserves integer and floating-point bits independently of presence. The tagged C entry points return the two-word aggregate and accept payload/metadata as separate arguments. Rebuild compiler and runtime together; single-word legacy exports remain adapters for non-null payload consumers.

Tasks and channels hold inline `AbiValue` copies. Captures are owned by the callback context; cloning a task or repeatedly awaiting it retains the same shared result. Sending, buffering, cancelling, timing out, and discarding scalar payloads require no separate payload allocation or deallocation. Existing pointer/object retention rules remain in force. Task failures, cancellation, and channel closure use `TaskError` and pending exceptions, independently of payload presence.

Untyped `List`/`Map` elements still use single-word storage. Nullable scalars and tasks returning nullable scalars are diagnosed at collection insertion, including task-combinator input lists. Typed collection/combinator support, wider target validation, and ownership/leak baselines remain [Phase 1 follow-ups](MILESTONES.md#phase-1-blocker-nullable-scalars-across-async-boundaries).

## Proposed Core and Package Boundary

The proposed three-phase scope is in [Roadmap v2](MILESTONES.md). The current compiler still has a Cargo dependency on `ube-runtime` and domain-specific registrations and lowering for financial, JSON, XML, and server APIs. The runtime currently includes optional-domain dependencies such as TLS and QUIC libraries; the dependency boundary below has not been implemented.

The target is a compiler that understands ordinary language constructs and a documented native ABI, with `Money`, `Json`, and `Server` provided by separately linked packages. A minimal local module/package loader, type metadata, symbol resolution, and native linking must exist before those APIs can be extracted. Moving files or repositories alone is insufficient.

The core runtime would retain only required allocation, string/collection, exception, and task primitives. Proposed `ube-money`, `ube-json`, `ube-io`, and `ube-http` packages would supply domain behavior; `ube-std` could aggregate them. Compiler bootstrap and core-only application builds should avoid linking these optional packages. Names and distribution layout remain proposed until the extraction path is exercised.

---

## 3. Host Implementation Language Evaluation

We evaluated **Java**, **Rust**, and **C++** as candidate implementation languages for the initial Ube compiler.

### 3.1 Comparison Matrix

| Evaluation Dimension | Java | Rust | C++ |
| :--- | :--- | :--- | :--- |
| **1. Parser Implementation** | Moderate (ANTLR / Parser combinators) | High (Hand-written recursive descent / `nom`) | High (Hand-written recursive descent / `bison`) |
| **2. LLVM Integration** | Poor (JNI / Java-LLVM bindings lag behind) | High (`inkwell` / `llvm-sys` official C API) | Outstanding (Native official LLVM C++ APIs) |
| **3. Development Speed** | High (Rapid prototyping) | High (Cargo ecosystem, robust type system) | Moderate (Slower builds, header management) |
| **4. Memory Safety** | High (JVM managed, GC pause overhead) | Outstanding (Compile-time borrow checker, 0-cost) | Low (Manual memory errors, dangling pointers) |
| **5. Cross-Platform** | High (Requires JVM runtime on host) | High (Single native binary distribution) | High (Native binary distribution) |
| **6. Testing Strategy** | High (JUnit / Spock) | Outstanding (Cargo built-in unit, integration, snapshot) | Moderate (GoogleTest / Catch2 setup) |
| **7. Debugging Tools** | High (JDB, IntelliJ debugger) | High (GDB, LLDB, Rust-analyzer) | High (CLion, VS Code, Visual Studio) |
| **8. IDE Tooling** | High (IntelliJ, Eclipse) | Outstanding (Rust-analyzer, VS Code) | High (CLion, VS Code, Visual Studio) |
| **9. Maintenance** | Moderate (Refactoring AST without pattern match) | Outstanding (Enums, algebraic types, pattern matching) | Moderate (Slower refactoring, header dependencies) |
| **10. Packaging & Dist** | Poor (Fat JARs or bundled JRE needed) | Outstanding (Single, zero-dependency static executable) | High (Single executable, build setup complex) |
| **11. Native Integration**| Poor (JNI overhead and C-ABI glue) | Outstanding (Direct `extern "C"` ABI compatibility) | High (Direct C/C++ ABI) |
| **12. Self-Hosting Path** | Moderate (Requires JVM bootstrap phase) | Outstanding (Clean conceptual shift to Ube typed IR) | Moderate (Requires C++ toolchain bootstrap) |

### 3.2 Final Recommendation & Justification
**Recommendation**: **Rust** is selected as the implementation language for the Ube compiler.

**Justification**:
1. **Direct LLVM C API Access**: Inkwell and `llvm-sys` provide first-class, safe bindings to LLVM C API, allowing pinned versioning (LLVM 12-22) via Cargo feature flags.
2. **Algebraic Data Types & Pattern Matching**: Compiler ASTs and SSA IRs are naturally expressed via Rust `enum` variants, making parser and semantic passes concise, memory-safe, and refactorable.
3. **Distribution & Single Binary**: Rust compiles down to a single native compiler binary (`ube`) without requiring host JRE dependencies or complex C++ CMake tooling.

---

## 4. Pure Object-Oriented Bootstrapping Model

Ube prohibits free functions, static methods, and package-level functions. The entire system bootstraps strictly through class instantiations and instance method invocations.

### 4.1 Application Entry Point
The application entry point is defined as a method named `main()` (with implicit void return type, as there is no `void` keyword in Ube) on a class such as `Main` or `Application` (in `Main.ube` or `Application.ube`, respectively):

```java
class Application {
    main() {
        final service = UserService()
        service.run()
    }
}
```

#### Synthetic Bootstrap Code
The compiler generates a native C-ABI `main(int argc, char** argv)` function during LLVM IR generation:

```llvm
define i32 @main(i32 %argc, i8** %argv) {
entry:
    call void @ube_runtime_init()
    %app_ptr = call %Application* @__ube.internal_zero(i8* null)
    call void @Application_main(%Application* %app_ptr)
    call void @ube_exception_fail_unhandled()
    call void @ube_runtime_cleanup()
    ret i32 0
}
```

### Shared Zero Initialization

The IR builder generates `__ube.internal_zero` for startup using the same zero-value builder as typed local declarations. `User user` creates a fresh zero-valued object; `User()` is explicit construction and applies constructor defaults. Startup ignores `Application` defaults, including expressions with side effects. Resource creation belongs explicitly inside `main()`.

Numbers become `0`, booleans `false`, strings `''`, and nullable references `null`. Non-nullable user-class fields are recursively zero-initialized without default evaluation. Non-nullable cycles are diagnosed; nullable links terminate recursion. Runtime-only types and traits require explicit initializers unless nullable. These are allocated class objects, not allocation-free embedded values.

### Loop Fallback State

Each `for` / `else` lowering allocates a boolean body-entry flag initialized to false and sets it to true at the start of the body, before user statements. The fallback checks this flag when the initial collection or condition permits no iterations. Normal completion after body entry skips `else`; `break` exits directly and `continue` still counts as an entered iteration. Each nested loop owns an independent flag. This tracks zero iterations, not search success or absence of `break`.

### Field Access, Syntax & Errors

Generated backing fields such as `_age` are class-private. Public access requires `user.age()`; a `var` field also accepts `user.age(value)`. Direct `user.age` reads and assignments are rejected. One-line strings and characters use single quotes; backticks delimit raw text blocks with literal content. Double quotes are reserved for JSON content. Semicolons in code are errors; literals and comments may contain them.

The compiler lowers `try` / typed `catch` / `finally` / `throw` into explicit control flow and a runtime pending-error channel, without unwinding through Rust. Cleanup runs across returns, loop exits, and propagated exceptions. Cleanup failures preserve pending errors as suppressed. Awaited failures retain their error objects. Cancellation is delivered once per worker so later cleanup awaits can complete; those awaits can still fail or time out.

### 4.2 Built-in Logging and Console Output

The parser rewrites `log(value)` into `Log.info(value)`. `Log` and `Console`
are compiler-recognized built-ins, with static call, arity, and value-type checks.
The parser also rewrites `print(value)` into `Console.printLine(value)`. The compiler lowers supported values to
one of the runtime output functions:

- `ube_output_str(i32 mode, i8* value)`
- `ube_output_i64(i32 mode, i64 value)`
- `ube_output_f64(i32 mode, double value)`
- `ube_output_bool(i32 mode, i1 value)`

Modes 0–3 are DEBUG, INFO, WARN, and ERROR, written to stderr with a severity
label and newline. Modes 4–5 implement `Console.print` and `Console.printLine`
on stdout, independently of logging filters. Nullable scalar output branches on
presence; `Error` values dispatch through `message()`. Arguments are evaluated
once even when a message is filtered out.

`ube_log_set_level(i8*)` updates a process-wide atomic threshold, defaulting to
INFO. Invalid settings and output failures use the pending-exception channel.
Each output call locks its stream to avoid interleaved records across tasks.
This API implements console severity logging, without Java's handler hierarchy,
file rotation, or automatic exception stack-trace formatting.

MDC operations lower to `ube_log_put`, `ube_log_get`, `ube_log_remove`, and
`ube_log_clear`. The runtime stores context in a thread-local ordered string map.
`get` returns a tracked copy or NULL, matching Ube's `string?` ABI. Log records
include a JSON-escaped context object when the map is nonempty.

The native task bridge captures a context snapshot before scheduling a callback.
Each blocking worker installs that snapshot with an RAII guard, which restores
its previous context when the callback exits. This isolates child and sibling
tasks while supporting inheritance across nested task creation. Yielding task
callbacks restore and save context for each poll, so context follows the task
across worker migration. Context is released at runtime cleanup; the process-wide
severity threshold is independent of context.

---

### Automatic Data Serialization

The compiler supplies type and field metadata for automatic `.toJson()` and `.toXml()` calls. The runtime shares a typed traversal for concrete objects, lists, maps, and supported scalars. Collection entries retain serialization metadata; methods and native object identity are not serialized. This adds no cached XML fields to user objects. Traversal roots managed references and rejects cycles, unsupported resources, non-finite values, and excessive size or nesting.

XML encoding lowers through `XmlEncode`, including optional `XmlOptions`, and uses normal exception propagation. `runtime/src/xml.rs` uses `roxmltree` for bounded parsing and owns document trees, element handles, and options until runtime cleanup. `Xml.toObject()` returns a document rather than reconstructing user classes. Namespace-aware traversal, explicit attributes, nulls, lists, and map entries follow the [XML rules](docs/xml.md). The parser dependency remains part of the current runtime; package extraction is proposed work.

[Native end-to-end tests](docs/testing.md#serialization-end-to-end-regressions) compile and run serialization programs, exercise CLI run/build, and exchange XML files between independent processes. Serialization calls are synchronous and use the blocking async backend when required.

## 5. Memory Management & Garbage Collector Architecture

Generated class objects use a precise mark-and-sweep heap with compiler-emitted pointer descriptors, method roots, return-root transfer, automatic collection, and serialized pointer-field barriers. Native and generated concurrent-mutator regressions exercise cycle collection and root handoff. Opaque runtime escapes are conservatively pinned; resource registries and several native object types retain explicit/shutdown lifetimes. See [the tracing heap boundary](docs/memory.md). Generated objects contain their declared fields directly and do not have the proposed object header below. Whole-process memory bounds and production service reliability are not established.

```text
  [ Ube Heap Allocator ]
             │
             ├──► Small Objects (Bump-pointer / Thread-local pools)
             └──► Large Objects (Page-aligned direct mmap allocations)
```

### 5.1 Proposed Object Layout Header
The following 16-byte header is a planned design, not the current object ABI:

```text
 0                   4                   8                   12                  16 Bytes
┌───────────────────┬───────────────────┬───────────────────┬───────────────────┐
│     ClassID       │    GC Metadata    │    Mark Bit /     │   Field Pointer   │
│     (32-bit)      │     (32-bit)      │   Forwarding Ptr  │      Offset       │
└───────────────────┴───────────────────┴───────────────────┴───────────────────┘
```

### 5.2 Planned Incremental GC Strategy
- **Phase 1 (MVP)**: Stop-the-world, mark-and-sweep collector using shadow stacks or LLVM `statepoint` / `gc.root` intrinsics.
- **Phase 2 (Production)**: Tri-color generational collector with thread-local allocation buffers (TLABs) and read/write barriers.
- **Stack Allocation Optimization**: Escape analysis pass in Ube IR identifies non-escaping objects and replaces heap allocation with stack `alloca`.

---

## 6. Custom Typed Ube IR Design

Before emitting LLVM IR, the compiler builds a typed control-flow graph with registers and stack slots. The following is a simplified sketch; authoritative IR definitions are in `compiler/src/ir/nodes.rs`.

```rust
pub struct IrProgram {
    pub classes: Vec<IrClass>,
    pub functions: Vec<IrFunction>,
}

pub struct IrFunction {
    pub class_name: String,
    pub receiver: (Register, Type),
    pub is_async: bool,
    pub name: String,
    pub params: Vec<(Register, Type)>,
    pub return_type: Type,
    pub blocks: Vec<BasicBlock>,
}

pub enum IrInstruction {
    Alloca { dest: Register, ty: Type },
    Load { dest: Register, ptr: Register, ty: Type },
    Store { val: IrOperand, ptr: Register },
    Binary { dest: Register, op: BinaryOp, left: IrOperand, right: IrOperand, ty: Type },
    CallMethod { dest: Option<Register>, receiver: IrOperand, method: String, args: Vec<IrOperand> },
    CallRuntime { dest: Option<Register>, symbol: String, args: Vec<IrOperand>, ret_ty: Type },
    Branch { cond: IrOperand, then_block: BlockId, else_block: BlockId },
    Jump { target: BlockId },
    Return { val: Option<IrOperand> },
}
```
