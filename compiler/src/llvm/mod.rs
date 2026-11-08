pub mod textual_emitter;

pub use textual_emitter::TextualLlvmEmitter;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::IrBuilder;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::semantics::TypeChecker;

    #[test]
    fn start_is_the_entry_point_and_main_is_an_ordinary_method() {
        for modifier in ["", "static "] {
            let source = format!(
                "class Engine(Engine recursive) {{ start() {{}} }} class App {{ main() {{}} {modifier}start() {{}} }}"
            );
            let mut parser = Parser::new(Lexer::new(&source));
            let program = parser.parse_program();
            assert!(parser.diagnostics.is_empty());
            let mut checker = TypeChecker::new();
            assert!(checker.check_program(&program), "{:?}", checker.diagnostics);
            let mut builder = IrBuilder::new();
            let ir = builder.build_program(&program);
            assert!(builder.diagnostics.is_empty(), "{:?}", builder.diagnostics);
            let llvm = TextualLlvmEmitter::new().emit_program(&ir);
            let wrapper = llvm.split("define i32 @main(").nth(1).unwrap();
            assert!(wrapper.contains("call void @App_start("));
            assert!(!wrapper.contains("call void @App_main("));
            assert_eq!(wrapper.contains("@__ube.internal_zero("), modifier.is_empty());
        }

        let mut parser = Parser::new(Lexer::new("class App { main() {} }"));
        let program = parser.parse_program();
        let ir = IrBuilder::new().build_program(&program);
        let llvm = TextualLlvmEmitter::new().emit_program(&ir);
        assert!(!llvm.contains("define i32 @main("));
    }

    #[test]
    fn test_llvm_ir_emission_ube() {
        let code = r#"
            class User(long id, string name, var int age = 0) {
                void birthday() {
                    this._age = this._age + 1
                }
            }

            class App {
                void start() {
                    final user = User(1000, 'Lem', 35)
                    user.birthday()
                }
            }
        "#;

        let lexer = Lexer::new(code);
        let mut parser = Parser::new(lexer);
        let program = parser.parse_program();

        let mut checker = TypeChecker::new();
        checker.check_program(&program);

        let mut builder = IrBuilder::new();
        let ir_program = builder.build_program(&program);

        let mut emitter = TextualLlvmEmitter::new();
        let llvm_ir = emitter.emit_program(&ir_program);

        assert!(llvm_ir.contains("%User = type {"));
        assert!(llvm_ir.contains("define void @User_birthday(%User* %0)"));
        assert!(llvm_ir.contains("define i32 @main(i32 %argc, i8** %argv)"));
    }
}
