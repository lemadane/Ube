pub mod ast;
pub mod parser;

pub use ast::*;
pub use parser::Parser;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    #[test]
    fn test_parse_ube_class_and_application() {
        let code = r#"
            class User(
                long id,
                string name,
                var int age = 0
            ) {
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

        assert!(
            parser.diagnostics.is_empty(),
            "Parser errors: {:?}",
            parser.diagnostics
        );
        assert_eq!(program.items.len(), 2);

        if let Item::Class(user_cls) = &program.items[0] {
            assert_eq!(user_cls.name, "User");
            assert_eq!(user_cls.primary_params.len(), 3);
            assert_eq!(user_cls.methods.len(), 1);
            assert_eq!(user_cls.methods[0].name, "birthday");
        } else {
            panic!("Expected User class decl");
        }

        if let Item::Class(app_cls) = &program.items[1] {
            assert_eq!(app_cls.name, "Application");
            assert_eq!(app_cls.methods.len(), 1);
            assert_eq!(app_cls.methods[0].name, "main");
        } else {
            panic!("Expected Application class decl");
        }
    }
}
