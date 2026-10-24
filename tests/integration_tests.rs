use ube_compiler::{IrBuilder, Lexer, Parser, TextualLlvmEmitter, TypeChecker};

#[test]
fn test_end_to_end_pipeline() {
    let source_code = r#"
        fn compute(n: i64) -> i64 {
            let mut acc: i64 = 0;
            let mut i: i64 = 1;
            while i <= n {
                acc = acc + i;
                i = i + 1;
            }
            return acc;
        }

        fn main() -> i64 {
            let result: i64 = compute(10);
            print_i64(result);
            return 0;
        }
    "#;

    // 1. Lexing & Parsing
    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.diagnostics.is_empty(), "Parsing failed: {:?}", parser.diagnostics);

    // 2. Type Checking
    let mut checker = TypeChecker::new();
    let typed_program = checker.check_program(&program);
    assert!(checker.diagnostics.is_empty(), "Type checking failed: {:?}", checker.diagnostics);

    // 3. IR Lowering
    let mut builder = IrBuilder::new();
    let ir_program = builder.build_program(&typed_program);
    assert_eq!(ir_program.functions.len(), 2);

    // 4. LLVM IR Generation
    let mut emitter = TextualLlvmEmitter::new();
    let llvm_ir = emitter.emit_program(&ir_program);

    assert!(llvm_ir.contains("define i64 @compute(i64 %0)"));
    assert!(llvm_ir.contains("define i64 @main()"));
    assert!(llvm_ir.contains("call void @ube_print_i64"));
}
