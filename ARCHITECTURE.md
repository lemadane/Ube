# Timawa Compiler & Runtime Architecture

This document defines the system architecture, compiler pipeline, directory layout, memory management model, bootstrapping design, and host implementation language evaluation for the **Talas** programming language compiler.

---

## 1. Compiler Pipeline Architecture

The Timawa compiler follows a strictly phased compilation pipeline that directly emits LLVM IR without intermediate transpilation to external high-level languages.

```text
Timawa Source Code (.talas)
         ↓
  [ Lexer Scanner ]  → Tokens with Line/Column Spans
         ↓
 [ Parser (Pratt) ]  → Abstract Syntax Tree (AST)
         ↓
[ Package & Symbol Resolver ] → Symbol Tables & Scope Hierarchies
         ↓
[ Semantic & Type Analyzer ] → Fully Typed AST & Flow Narrowing
         ↓
 [ Typed Timawa IR Builder ] → Control Flow Graph (CFG) in SSA Form
         ↓
  [ LLVM IR Generator ] → Textual .ll / Inkwell In-Memory LLVM Module
         ↓
 [ LLVM Optimizer Pass Manager ] → Target-Specific LLVM IR Optimization (-O2/-O3)
         ↓
[ LLVM Code Generator (llc) ] → Native Target Object Code (.o / .obj)
         ↓
  [ Native Linker (lld) ] → Bundled with talas-runtime
         ↓
   Native Executable
```

---

## 2. Project & Workspace Directory Structure

The Timawa workspace is structured as a multi-crate Rust project managed by Cargo, with clear architectural separation between compiler frontend, SSA IR transformation, LLVM code generation, and the native C-ABI runtime.

```text
Timawa/
├── Cargo.toml                      # Workspace root definition
├── README.md                       # Main project README
├── LANGUAGE_SPEC_DRAFT.md          # Baseline language specification
├── ARCHITECTURE.md                 # Compiler & runtime architecture
├── DECISIONS_NEEDED.md             # 40 unresolved feature resolutions
├── MILESTONES.md                   # 14 phased milestones & roadmap
├── IMPLEMENTATION_PLAN.md          # Master implementation plan
├── compiler/                       # Crate: talas-compiler
│   ├── Cargo.toml                  # Compiler crate manifest
│   └── src/
│       ├── main.rs                 # talas CLI driver (check, emit-ir, emit-llvm, build)
│       ├── lib.rs                  # Compiler library exports
│       ├── diagnostics/            # Span tracking & colorized diagnostic emitter
│       ├── lexer/                  # Backtick string scanner & token lexer
│       ├── parser/                 # Unified class Pratt parser & AST nodes
│       ├── resolver/               # Scope & symbol resolution
│       ├── types/                  # Type system representations
│       ├── semantics/              # Explicit 'this' & immutability checker
│       ├── ir/                     # SSA Control Flow Graph IR builder
│       └── llvm/                   # Textual LLVM IR emitter & C-main builder
├── runtime/                        # Crate: talas-runtime (Native C-ABI static library)
│   ├── Cargo.toml                  # Runtime crate manifest
│   └── src/
│       ├── lib.rs                  # C-ABI runtime init & cleanup
│       ├── memory.rs               # Heap allocator primitives & object headers
│       ├── strings.rs              # UTF-16 string runtime primitives
│       ├── io.rs                   # System I/O wrappers (@talas_print_str, etc.)
│       ├── gc.rs                   # Precise incremental GC stubs
│       └── collections.rs          # Runtime collection array/map representations
└── standard-library/               # Core Timawa standard library packages
    └── prelude.talas               # Standard library prelude definitions
```

---

## 3. Host Implementation Language Evaluation

We evaluated **Java**, **Rust**, and **C++** as candidate implementation languages for the initial Timawa compiler.

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
| **12. Self-Hosting Path** | Moderate (Requires JVM bootstrap phase) | Outstanding (Clean conceptual shift to Timawa typed IR) | Moderate (Requires C++ toolchain bootstrap) |

### 3.2 Final Recommendation & Justification
**Recommendation**: **Rust** is selected as the implementation language for the Timawa compiler.

**Justification**:
1. **Direct LLVM C API Access**: Inkwell and `llvm-sys` provide first-class, safe bindings to LLVM C API, allowing pinned versioning (LLVM 12-22) via Cargo feature flags.
2. **Algebraic Data Types & Pattern Matching**: Compiler ASTs and SSA IRs are naturally expressed via Rust `enum` variants, making parser and semantic passes concise, memory-safe, and refactorable.
3. **Distribution & Single Binary**: Rust compiles down to a single native compiler binary (`talas`) without requiring host JRE dependencies or complex C++ CMake tooling.

---

## 4. Pure Object-Oriented Bootstrapping Model

Timawa prohibits free functions, static methods, and package-level functions. The entire system bootstraps strictly through class instantiations and instance method invocations.

### 4.1 Application Entry Point
The application entry point is defined as a method named `main()` (with implicit void return type, as there is no `void` keyword in Timawa) on a class named `Application`:

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
    call void @talas_runtime_init()
    %app_ptr = call %Application* @Application_create()
    call void @Application_main(%Application* %app_ptr)
    call void @talas_runtime_cleanup()
    ret i32 0
}
```

### 4.2 Built-in `print` Directive
Timawa provides `print(...)` as the sole built-in output directive (always appends a newline, equivalent to Java's `System.out.println` or Python's `print()`). There is no separate `println` keyword in Timawa.

```java
class Application {
    main() {
        final user = User(1000, `Lem`, 35)
        print(`Hello, ${user.name()}!`) // Built-in print directive (always includes newline)
    }
}
```

System intrinsics for `print` are wired directly to native runtime static library symbols (`talas_runtime.a`):
- `talas_print_str(i8*)`
- `talas_print_i64(i64)`
- `talas_print_f64(double)`
- `talas_print_bool(i1)`

---

## 5. Memory Management & Garbage Collector Architecture

Timawa uses a precise, garbage-collected runtime.

```text
  [ Timawa Heap Allocator ]
             │
             ├──► Small Objects (Bump-pointer / Thread-local pools)
             └──► Large Objects (Page-aligned direct mmap allocations)
```

### 5.1 Object Layout Header
Every Timawa object allocated on the heap begins with a 16-byte object header:

```text
 0                   4                   8                   12                  16 Bytes
┌───────────────────┬───────────────────┬───────────────────┬───────────────────┐
│     ClassID       │    GC Metadata    │    Mark Bit /     │   Field Pointer   │
│     (32-bit)      │     (32-bit)      │   Forwarding Ptr  │      Offset       │
└───────────────────┴───────────────────┴───────────────────┴───────────────────┘
```

### 5.2 Incremental GC Strategy
- **Phase 1 (MVP)**: Stop-the-world, mark-and-sweep collector using shadow stacks or LLVM `statepoint` / `gc.root` intrinsics.
- **Phase 2 (Production)**: Tri-color generational collector with thread-local allocation buffers (TLABs) and read/write barriers.
- **Stack Allocation Optimization**: Escape analysis pass in Timawa IR identifies non-escaping objects and replaces heap allocation with stack `alloca`.

---

## 6. Custom Typed Timawa IR Design

Before emitting LLVM IR, the compiler translates the Typed AST into a high-level Control Flow Graph (CFG) Intermediate Representation in SSA form.

```rust
pub struct IrProgram {
    pub classes: Vec<IrClass>,
    pub functions: Vec<IrFunction>,
}

pub struct IrFunction {
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
