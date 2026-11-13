//! Workspace-level language pipeline example, registered by compiler/Cargo.toml.
use ube_compiler::{IrBuilder, Lexer, Parser, TextualLlvmEmitter, TypeChecker};

#[test]
fn test_end_to_end_pipeline() {
    let source = r#"
class App {
    int compute(int count) {
        var total = 0
        var index = 1
        while (index <= count) {
            total = total + index
            index = index + 1
        }
        return total
    }

    start() {
        final result = Async.start(() -> this.compute(10)).await()
        Console.printLine(result)
    }
}
"#;
    let mut parser = Parser::new(Lexer::new(source));
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "{:?}", parser.diagnostics);
    let mut checker = TypeChecker::new();
    assert!(checker.check_program(&program), "{:?}", checker.diagnostics);
    let mut builder = IrBuilder::new();
    let ir = builder.build_program(&program);
    assert!(builder.diagnostics.is_empty(), "{:?}", builder.diagnostics);
    let llvm = TextualLlvmEmitter::new().emit_program(&ir);
    assert!(llvm.contains("@App_compute"));
    assert!(llvm.contains("@ube_task_"));
    assert!(llvm.contains("define i32 @main(i32 %argc, i8** %argv)"));
    assert!(llvm.contains("@ube_output_i64"));
}
