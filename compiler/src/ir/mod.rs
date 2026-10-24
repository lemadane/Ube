pub mod builder;
pub mod nodes;

pub use builder::IrBuilder;
pub use nodes::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::llvm::TextualLlvmEmitter;
    use crate::parser::Parser;
    use crate::semantics::TypeChecker;
    use crate::types::Type;

    fn build_ir(code: &str) -> IrProgram {
        let lexer = Lexer::new(code);
        let mut parser = Parser::new(lexer);
        let program = parser.parse_program();
        assert!(parser.diagnostics.is_empty(), "{:?}", parser.diagnostics);

        let mut builder = IrBuilder::new();
        let ir = builder.build_program(&program);
        assert!(builder.diagnostics.is_empty(), "{:?}", builder.diagnostics);
        ir
    }

    fn application_main(ir: &IrProgram) -> &IrFunction {
        ir.functions
            .iter()
            .find(|function| function.class_name == "Application" && function.name == "main")
            .expect("Application.main should be generated")
    }

    #[test]
    fn async_method_calls_are_spawned_and_awaited_with_the_declared_result_type() {
        let code = r#"
            class Worker {
                async compute(int value) {
                    return value + 1
                }
            }
            class Application {
                main() {
                    final worker = Worker()
                    final task = worker.compute(41)
                    final results = await Task.all([task, worker.compute(42)])
                    print(results.len())
                }
            }
        "#;
        let lexer = Lexer::new(code);
        let mut parser = Parser::new(lexer);
        let program = parser.parse_program();
        assert!(parser.diagnostics.is_empty(), "{:?}", parser.diagnostics);
        let mut checker = TypeChecker::new();
        assert!(checker.check_program(&program), "{:?}", checker.diagnostics);
        let mut builder = IrBuilder::new();
        let ir = builder.build_program(&program);
        assert!(builder.diagnostics.is_empty(), "{:?}", builder.diagnostics);
        let mut emitter = TextualLlvmEmitter::new();
        let llvm = emitter.emit_program(&ir);
        assert!(llvm.contains("call i8* @ube_task_spawn"));
        assert!(llvm.contains("call i64 @ube_task_context_get"));
        assert!(llvm.contains("call i32 @Worker_compute(%Worker*"));
        assert!(llvm.contains("call i64 @ube_task_await(i8*"));
        assert!(llvm.contains("call i8* @ube_task_all_async(i8*"));
        assert!(llvm.contains("trunc i64"));
    }

    #[test]
    fn test_ir_building_ube_class() {
        let code = r#"
            class User(long id, string name, var int age = 0) {
                void birthday() {
                    this._age = this._age + 1
                }
            }

            class Application {
                void main() {
                    final user = User(1000, `Lem`, 35)
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

        assert_eq!(ir_program.classes.len(), 2);
        assert!(!ir_program.functions.is_empty());
    }

    #[test]
    fn iterable_for_loop_loads_values_and_increments_index() {
        let ir = build_ir(
            r#"
            class Application {
                void main() {
                    for (index, item in [10, 20]) {
                        print(index)
                        print(item)
                    }
                }
            }
            "#,
        );
        let function = application_main(&ir);
        let instructions: Vec<_> = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .collect();

        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            IrInstruction::CallRuntime { symbol, .. } if symbol == "ube_list_len"
        )));
        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            IrInstruction::CallRuntime { symbol, .. } if symbol == "ube_list_get"
        )));
        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            IrInstruction::Binary { op: crate::parser::BinaryOp::Less, .. }
        )));
        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            IrInstruction::Binary {
                op: crate::parser::BinaryOp::Add,
                right: IrOperand::ConstantInt(1),
                ..
            }
        )));
    }

    #[test]
    fn static_calls_and_if_expression_keep_their_types() {
        let ir = build_ir(
            r#"
            class Application {
                void main() {
                    final root = Math.sqrt(4)
                    final value = if (1) {
                        final branchValue = `left`
                        yield branchValue
                    } else {
                        final branchValue = `right`
                        yield branchValue
                    }
                    final date = if (true) {
                        yield DateTime.now()
                    } else {
                        yield DateTime.utc()
                    }
                }
            }
            "#,
        );
        let function = application_main(&ir);
        let instructions: Vec<_> = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .collect();

        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            IrInstruction::CallRuntime { symbol, .. } if symbol == "ube_math_sqrt"
        )));
        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            IrInstruction::Alloca { ty: Type::String, .. }
        )));
        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            IrInstruction::Load { ty: Type::String, .. }
        )));
        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            IrInstruction::Alloca {
                ty: Type::Class { name },
                ..
            } if name == "DateTime"
        )));
        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            IrInstruction::Binary {
                op: crate::parser::BinaryOp::NotEqual,
                result_ty: Type::Bool,
                ..
            }
        )));
        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            IrInstruction::Branch { cond, .. } if cond.get_type() == Type::Bool
        )));
    }

    #[test]
    fn date_and_duration_units_are_converted_to_seconds() {
        let ir = build_ir(
            r#"
            class Application {
                void main() {
                    DateTime.now().addDays(2)
                    DateTime.now().diffDays(DateTime.now())
                    Duration.days(3)
                }
            }
            "#,
        );
        let function = application_main(&ir);
        let instructions: Vec<_> = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .collect();

        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            IrInstruction::Binary {
                op: crate::parser::BinaryOp::Mul,
                right: IrOperand::ConstantInt(86_400),
                ..
            }
        )));
        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            IrInstruction::Binary {
                op: crate::parser::BinaryOp::Div,
                right: IrOperand::ConstantInt(86_400),
                ..
            }
        )));
    }

    #[test]
    fn guarded_list_index_checks_negative_indices() {
        let ir = build_ir(
            r#"
            class Application {
                void main() {
                    guard {
                        final value = [1][-1]
                        catch (err) {
                        }
                    }
                }
            }
            "#,
        );
        let function = application_main(&ir);
        let instructions: Vec<_> = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .collect();

        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            IrInstruction::Binary {
                op: crate::parser::BinaryOp::Less,
                right: IrOperand::ConstantInt(0),
                ..
            }
        )));
        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            IrInstruction::Binary {
                op: crate::parser::BinaryOp::Or,
                operand_ty: Type::Bool,
                ..
            }
        )));
    }
}
