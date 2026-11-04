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
    fn test_llvm_ir_emission_ube() {
        let code = r#"
            class User(long id, string name, var int age = 0) {
                void birthday() {
                    this._age = this._age + 1
                }
            }

            class Application {
                void main() {
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
