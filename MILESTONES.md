# Timawa Compiler Phased Execution Milestones

This document details the 14 implementation phases for the **Talas** compiler, including deliverables, dependencies, technical risks, engineer-week estimates, objective exit criteria, and parallelization opportunities.

---

## Phased Timeline & Estimates Overview

Total Estimated Duration: **74 Engineer-Weeks**

```text
Phase 0: Specification Baseline          [ 2 wks ]
Phase 1: Compiler Skeleton             [ 3 wks ]
Phase 2: Semantic Frontend              [ 6 wks ]
Phase 3: Minimal LLVM Vertical Slice    [ 5 wks ] ──► MVP Milestone
Phase 4: Complete Class Model           [ 6 wks ]
Phase 5: Anonymous Objects              [ 4 wks ]
Phase 6: Control Flow                   [ 5 wks ]
Phase 7: Generics & Composition         [ 8 wks ]
Phase 8: Nullability                    [ 5 wks ]
Phase 9: Errors & Resources             [ 4 wks ]
Phase 10: Runtime & Garbage Collection  [ 8 wks ]
Phase 11: Concurrency                   [ 8 wks ]
Phase 12: Developer Tooling             [ 6 wks ]
Phase 13: Self-Hosting Assessment       [ 4 wks ]
```

---

## Phase Breakdowns

### Phase 0: Specification Baseline
- **Deliverables**: Finalized EBNF grammar, language spec baseline (`LANGUAGE_SPEC_DRAFT.md`), language decision records (LDRs), and positive/negative syntax test suites.
- **Dependencies**: None.
- **Risks**: Syntax ambiguities in backtick string interpolation and word booleans.
- **Estimate**: 2 Engineer-Weeks.
- **Exit Criteria**: 100% of approved syntax features documented with zero grammatical ambiguities in LALR/Pratt grammar rules.
- **Parallelization**: Grammar specification and test-case drafting can run in parallel.

---

### Phase 1: Compiler Skeleton
- **Deliverables**: Cargo workspace repository, `timawa` CLI scaffolding, source file loader, source location span tracker (`Position`, `Span`), diagnostics emitter, hand-written Pratt parser, AST data structures, and parser snapshot tests.
- **Dependencies**: Phase 0.
- **Risks**: AST data structure bloat or inefficient span tracking.
- **Estimate**: 3 Engineer-Weeks.
- **Exit Criteria**: Parser achieves 100% test coverage on valid/invalid syntax test fixtures with accurate diagnostic spans.
- **Parallelization**: CLI scaffolding and Lexer/Parser can proceed concurrently.

---

### Phase 2: Semantic Frontend
- **Deliverables**: Lexical scoping symbol table (`SymbolTable`), package and import resolver, type system representations, mutability validator, default initialization checks, explicit `this` enforcement, naming-based visibility checks, flow-sensitive `is` type narrowing.
- **Dependencies**: Phase 1.
- **Risks**: Complex flow-sensitive narrowing logic invalidation across nested blocks.
- **Estimate**: 6 Engineer-Weeks.
- **Exit Criteria**: All frontend semantic passes pass 100% negative compilation test fixtures.
- **Parallelization**: Type checking and name resolution can be implemented in parallel modules.

---

### Phase 3: Minimal LLVM Vertical Slice (MVP Milestone)
- **Deliverables**: Custom Typed Timawa IR builder, textual LLVM IR emitter (`TextualLlvmEmitter`), driver for native linker (`lld`), and `timawa-runtime` static library with basic I/O (`print_i64`, `print_str`).
- **Dependencies**: Phase 2.
- **Risks**: Platform-specific C-ABI calling conventions and linker flag differences.
- **Estimate**: 5 Engineer-Weeks.
- **Exit Criteria**: **MVP Milestone**: A valid Timawa "Hello World" application (`class Application { void main() { ... } }`) compiles to a native executable and runs with exit code 0.
- **Parallelization**: LLVM IR generator and `timawa-runtime` library can be built in parallel.

---

### Phase 4: Complete Class Model
- **Deliverables**: `var` field mutation validation, immutable-field protection, primary constructor argument lowering, class memory layout generation, package-private member access checks, and instance method dispatch.
- **Dependencies**: Phase 3.
- **Risks**: Incorrect struct padding and alignment in LLVM layout generation.
- **Estimate**: 6 Engineer-Weeks.
- **Exit Criteria**: Complex classes with `var` fields, explicit `this`, and constructor defaults compile and execute correctly natively.
- **Parallelization**: Field access validation and LLVM class struct generation can be split.

---

### Phase 5: Anonymous Objects
- **Deliverables**: Literal object parser, structural type inference, canonical field ordering algorithm, generated getter methods, structural `equals` and `hash` generation, and `with` expression updates.
- **Dependencies**: Phase 4.
- **Risks**: Name mangling collisions for structurally identical anonymous object types across modules.
- **Estimate**: 4 Engineer-Weeks.
- **Exit Criteria**: Anonymous object literals compile, update via `with`, and pass structural equality native tests.
- **Parallelization**: Structural type checking and `with` LLVM lowering can proceed in parallel.

---

### Phase 6: Control Flow
- **Deliverables**: Collection-based `for` loop lowering, indexed iteration, condition loops, infinite loops, `break` and `continue`, `switch` statements and expressions with `case` arrows, `if` guards, `yield`, and exhaustiveness validation.
- **Dependencies**: Phase 4.
- **Risks**: Complex control flow graph basic block splitting in nested switch expressions.
- **Estimate**: 5 Engineer-Weeks.
- **Exit Criteria**: All `for` loop variants and `switch` expressions pass exhaustive codegen verification.
- **Parallelization**: `for` loop lowering and `switch` expression lowering can be parallelized.

---

### Phase 7: Generics & Composition
- **Deliverables**: Generic class and method declaration handling, monomorphization pipeline, structural constraint validation, generic type inference, and binary size optimization passes.
- **Dependencies**: Phase 6.
- **Risks**: Monomorphization code bloat and recursive generic instantiation infinite loops.
- **Estimate**: 8 Engineer-Weeks.
- **Exit Criteria**: Generic data structures (`List<T>`, `Map<K, V>`) compile without interfaces and execute natively.
- **Parallelization**: Constraint validation and monomorphization codegen can proceed in parallel.

---

### Phase 8: Nullability
- **Deliverables**: Nullable types (`Type?`), `null` literal, flow-sensitive non-null narrowing, optional chaining operator `?.`, null coalescing `??`, null-coalescing assignment `??=`.
- **Dependencies**: Phase 7.
- **Risks**: Flaws in flow-sensitive narrowing during complex short-circuit boolean expressions.
- **Estimate**: 5 Engineer-Weeks.
- **Exit Criteria**: All nullability operators compile cleanly to branchless LLVM instructions or basic blocks.
- **Parallelization**: Parser/type checker rules and LLVM lowering can be parallelized.

---

### Phase 9: Errors & Resources
- **Deliverables**: `Result<T, E>` compiler integration, error propagation operator `?`, `defer` statement lowering, resource cleanup block injection.
- **Dependencies**: Phase 8.
- **Risks**: Unhandled resource leak paths during early return inside `defer` blocks.
- **Estimate**: 4 Engineer-Weeks.
- **Exit Criteria**: Programs using `defer` clean up file handles and memory reliably across all return branches.
- **Parallelization**: `defer` stack tracking and `Result` error operator can proceed in parallel.

---

### Phase 10: Runtime & Garbage Collection
- **Deliverables**: Thread-local heap allocators, LLVM `statepoint` / stack map generation, precise object header management, incremental mark-and-sweep GC, and escape analysis pass.
- **Dependencies**: Phase 9.
- **Risks**: GC root discovery crashes or dangling pointer race conditions during collection.
- **Estimate**: 8 Engineer-Weeks.
- **Exit Criteria**: Long-running allocation stress tests execute continuously without memory leaks or corrupt pointers.
- **Parallelization**: Allocator development and LLVM stack map generation can proceed in parallel.

---

### Phase 11: Concurrency
- **Deliverables**: Task representation, user-space scheduler, `async` and `await` lowering, task cancellation, channels, and async I/O runtime integration.
- **Dependencies**: Phase 10.
- **Risks**: Deadlocks or data races in scheduler context switching.
- **Estimate**: 8 Engineer-Weeks.
- **Exit Criteria**: 100,000 concurrent tasks spawn, communicate via channels, and complete natively.
- **Parallelization**: Scheduler development and `async`/`await` state-machine transformation can be split.

---

### Phase 12: Developer Tooling
- **Deliverables**: `timawa fmt` formatter, Language Server Protocol (`timawa-lsp`), syntax highlighter, DWARF debug information generator, and package manager (`timawa pkg`).
- **Dependencies**: Phase 11.
- **Risks**: LSP latency during live incremental type checking.
- **Estimate**: 6 Engineer-Weeks.
- **Exit Criteria**: VS Code extension provides full auto-complete, diagnostics, formatting, and step debugging via LLDB.
- **Parallelization**: Formatter, LSP, and Package Manager can be built concurrently by separate engineers.

---

### Phase 13: Self-Hosting Assessment
- **Deliverables**: Assessment report measuring language maturity, specification stability, compiler performance, and a staged bootstrapping roadmap for rewriting `timawa` compiler in Timawa.
- **Dependencies**: Phase 12.
- **Risks**: Immature language features hindering compiler self-implementation.
- **Estimate**: 4 Engineer-Weeks.
- **Exit Criteria**: Complete specification review confirming Timawa is capable of hosting its own frontend and IR builder.
- **Parallelization**: Benchmark analysis and bootstrap architecture drafting can run concurrently.
