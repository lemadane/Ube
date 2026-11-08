pub mod ast;
pub mod parser;

pub use ast::*;
pub use parser::Parser;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    #[test]
    fn empty_classes_require_omitting_braces() {
        for declaration in ["class Address(string city)", "class Empty", "class Empty()"] {
            for suffix in ["", "\nclass App { start() {} }"] {
                let source = format!("{declaration}{suffix}");
                let mut parser = Parser::new(Lexer::new(&source));
                let program = parser.parse_program();
                assert!(parser.diagnostics.is_empty(), "{source}: {:?}", parser.diagnostics);
                assert_eq!(program.items.len(), if suffix.is_empty() { 1 } else { 2 });
            }
            for body in [" {}", " {\n}", " { // no members\n}"] {
                let source = format!("{declaration}{body}");
                let mut parser = Parser::new(Lexer::new(&source));
                parser.parse_program();
                assert!(parser.diagnostics.iter().any(|diagnostic|
                    diagnostic.message == "Empty class bodies are not allowed; omit '{}'"
                ), "Missing empty class diagnostic for {source}: {:?}", parser.diagnostics);
            }
        }
    }

    #[test]
    fn semicolons_are_rejected_in_statements_and_declarations() {
        for source in [
            "; class App { start() {} }",
            "class App { start() { Console.printLine('hello'); } }",
            "class App { start() { final value = 1; } }",
            "class App { start() { return; } }",
            "class App { start() { for (true) { break; } } }",
            "class App { start() { for (true) { continue; } } }",
            "trait Printable { printDetails(); }",
            "class App { start(); }",
            "class App { start() { server.use('/api') { request -> Console.printLine('hello'); } } }",
            "class App { start() {} };",
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
            "class App { start() { return\n Console.printLine('unreachable') } }"
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
            assert_eq!(app_cls.name, "App");
            assert_eq!(app_cls.methods.len(), 1);
            assert_eq!(app_cls.methods[0].name, "start");
        } else {
            panic!("Expected App class decl");
        }
    }
}
