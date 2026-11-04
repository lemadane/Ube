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
│       ├── lexer/                  # Single-quoted strings, double-quoted text blocks, backtick characters
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
│       ├── io.rs                   # System I/O wrappers (@ube_print_str, etc.)
│       ├── exceptions.rs           # Pending errors, structural identity & suppression
│       ├── concurrency.rs          # Tokio tasks/channels & one-shot cancellation
│       ├── gc.rs                   # Precise incremental GC stubs
│       └── collections.rs          # Runtime collection array/map representations
└── standard-library/               # Core Ube standard library packages
    └── prelude.ube               # Standard library prelude definitions
```

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
The application entry point is defined as a method named `main()` (with implicit void return type, as there is no `void` keyword in Ube) on a class named `Application`:

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

### Field Access, Syntax & Errors

Generated backing fields such as `_age` are class-private. Public access requires `user.age()`; a `var` field also accepts `user.age(value)`. Direct `user.age` reads and assignments are rejected. One-line strings use single quotes, text blocks use double quotes followed by a line break, and character literals use backticks. Semicolons in code are errors; literals and comments may contain them.

The compiler lowers `try` / typed `catch` / `finally` / `throw` into explicit control flow and a runtime pending-error channel, without unwinding through Rust. Cleanup runs across returns, loop exits, and propagated exceptions. Cleanup failures preserve pending errors as suppressed. Awaited failures retain their error objects. Cancellation is delivered once per worker so later cleanup awaits can complete; those awaits can still fail or time out.

### 4.2 Built-in `print` Directive
Ube provides `print(...)` as the sole built-in output directive (always appends a newline, equivalent to Java's `System.out.println` or Python's `print()`). There is no separate `println` keyword in Ube.

```java
class Application {
    main() {
        final user = User(1000, 'Lem', 35)
        print('Hello, ${user.name()}!') // Built-in print directive (always includes newline)
    }
}
```

System intrinsics for `print` are wired directly to native runtime static library symbols (`ube_runtime.a`):
- `ube_print_str(i8*)`
- `ube_print_i64(i64)`
- `ube_print_f64(double)`
- `ube_print_bool(i1)`

---

## 5. Memory Management & Garbage Collector Architecture

A precise garbage collector is a roadmap goal. Currently `ube_alloc` tracks raw allocations for explicit release or runtime-shutdown cleanup, and the GC entry points are stubs. Generated objects contain their declared fields directly; they do not currently have the proposed object header below. This is not yet a production tracing collector.

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
