pub mod ast;
pub mod parser;

pub use ast::*;
pub use parser::Parser;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    #[test]
    fn semicolons_are_rejected_in_statements_and_declarations() {
        for source in [
            "; class Application { main() {} }",
            "class Application { main() { Console.writeLine('hello'); } }",
            "class Application { main() { final value = 1; } }",
            "class Application { main() { return; } }",
            "class Application { main() { for (true) { break; } } }",
            "class Application { main() { for (true) { continue; } } }",
            "trait Printable { printDetails(); }",
            "class Application { main(); }",
            "class Application { main() { server.use('/api') { request -> Console.writeLine('hello'); } } }",
            "class Application { main() {} };",
        ] {
            let mut parser = Parser::new(Lexer::new(source));
            parser.parse_program();
            assert!(parser.diagnostics.iter().any(|diagnostic|
                diagnostic.message == "Semicolons are not allowed in Ube; remove ';'"
            ), "Missing semicolon diagnostic for {source}: {:?}", parser.diagnostics);
        }
    }

    #[test]
    fn newline_ends_a_bare_return() {
        let mut parser = Parser::new(Lexer::new(
            "class Application { main() { return\n Console.writeLine('unreachable') } }"
        ));
        let program = parser.parse_program();
        assert!(parser.diagnostics.is_empty(), "{:?}", parser.diagnostics);
        let Item::Class(class) = &program.items[0] else { panic!("Expected class"); };
        assert!(matches!(class.methods[0].body[0].kind, StmtKind::Return(None)));
        assert_eq!(class.methods[0].body.len(), 2);
    }

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
                    final user = User(1000, 'Lem', 35)
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
