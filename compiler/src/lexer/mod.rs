pub mod lexer;
pub mod token;

pub use lexer::Lexer;
pub use token::{Token, TokenKind};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_boolean_operator_names_are_plain_identifiers() {
        let mut lexer = Lexer::new("nand nor xor xnor and or not");
        for name in ["nand", "nor", "xor", "xnor"] {
            assert_eq!(lexer.next_token().kind, TokenKind::Identifier(name.into()));
        }
        assert_eq!(lexer.next_token().kind, TokenKind::And);
        assert_eq!(lexer.next_token().kind, TokenKind::Or);
        assert_eq!(lexer.next_token().kind, TokenKind::Not);
    }

    #[test]
    fn semicolons_are_rejected_as_code_but_allowed_in_literals_and_comments() {
        let source = ";\n';' `;` \";\" \"\n  ;\n  \" // ;\n";
        let mut lexer = Lexer::new(source);
        let token = lexer.next_token();
        assert!(matches!(token.kind, TokenKind::Error(ref message)
            if message == "Semicolons are not allowed in Ube; remove ';'"));
        assert_eq!(token.span.start.line, 1);
        assert_eq!(token.span.start.column, 1);
        for expected in [";", ";", ";", ";\n"] {
            assert_eq!(lexer.next_token().kind, TokenKind::StringLiteral(expected.into()));
        }
        assert_eq!(lexer.next_token().kind, TokenKind::Eof);
    }

    #[test]
    fn test_lex_ube_keywords_and_string_literals() {
        let code = r#"
            class User(long id, string name, var int age = 0) {
                void birthday() {
                    this._age = this._age + 1
                }
            }

            final message = 'Hello, world'
            final prompt = "Hello\nworld"
            final initial = `A`
            if (a and not b or c) {
                user?.birthday()
            }
        "#;

        let mut lexer = Lexer::new(code);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token();
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok.kind);
            if is_eof {
                break;
            }
        }

        assert!(tokens.contains(&TokenKind::Class));
        assert!(tokens.contains(&TokenKind::LongType));
        assert!(tokens.contains(&TokenKind::StringType));
        assert!(tokens.contains(&TokenKind::Var));
        assert!(tokens.contains(&TokenKind::IntType));
        assert!(tokens.contains(&TokenKind::This));
        assert!(tokens.contains(&TokenKind::Final));
        assert!(tokens.contains(&TokenKind::And));
        assert!(tokens.contains(&TokenKind::Not));
        assert!(tokens.contains(&TokenKind::Or));
        assert!(tokens.contains(&TokenKind::QuestionDot));
        assert!(tokens.contains(&TokenKind::StringLiteral("Hello, world".to_string())));
        assert!(tokens.contains(&TokenKind::StringLiteral("Hello\nworld".to_string())));
        assert!(tokens.contains(&TokenKind::StringLiteral("A".to_string())));
    }

    #[test]
    fn test_lex_multiline_string_with_escape_and_indentation() {
        let code = "final message = \" \t\n    Hello from Ube!\n        Intentional indent.\n    escaped\\nnewline\n    \"";
        let mut lexer = Lexer::new(code);

        assert_eq!(lexer.next_token().kind, TokenKind::Final);
        assert_eq!(
            lexer.next_token().kind,
            TokenKind::Identifier("message".to_string())
        );
        assert_eq!(lexer.next_token().kind, TokenKind::Equal);
        assert_eq!(
            lexer.next_token().kind,
            TokenKind::StringLiteral(
                "Hello from Ube!\n    Intentional indent.\nescaped\nnewline\n".to_string()
            )
        );
    }

    fn lex_string(source: &str) -> TokenKind {
        Lexer::new(source).next_token().kind
    }

    #[test]
    fn test_multiline_string_newlines_and_closing_delimiter() {
        assert_eq!(
            lex_string("\"\n  first\r\n  second\r  \""),
            TokenKind::StringLiteral("first\nsecond\n".into())
        );
        assert_eq!(
            lex_string("\"\n  first\n  second\""),
            TokenKind::StringLiteral("first\nsecond".into())
        );
        assert_eq!(
            lex_string("\"\n  \n  \" "),
            TokenKind::StringLiteral("\n".into())
        );
    }

    #[test]
    fn test_multiline_string_escapes_and_line_continuation() {
        assert_eq!(
            lex_string("\" \n  {\\\"name\\\": \\\"Ube\\\"}\n  \""),
            TokenKind::StringLiteral("{\"name\": \"Ube\"}\n".into())
        );
        assert_eq!(
            lex_string("\" \n  \\\\\""),
            TokenKind::StringLiteral("\\".into())
        );
        assert_eq!(
            lex_string("\" \n  Hello \\\n  World\n  \""),
            TokenKind::StringLiteral("Hello World\n".into())
        );
        assert_eq!(
            lex_string("\" \n  \\b\\t\\n\\f\\r\\\"\\'\\\\\\s\\101\\u0041\n  \""),
            TokenKind::StringLiteral("\u{0008}\t\n\u{000c}\r\"'\\ AA\n".into())
        );
    }

    #[test]
    fn test_multiline_string_reports_invalid_escapes_and_unterminated_input() {
        assert!(matches!(
            lex_string("\" \n  invalid\\q\n  \""),
            TokenKind::Error(message) if message.contains("Invalid escape sequence")
        ));
        assert!(matches!(
            lex_string("\" \n  invalid\\u12z4\n  \""),
            TokenKind::Error(message) if message.contains("Unicode escape")
        ));
        assert!(matches!(
            lex_string("\" \n  invalid\\ \n  \""),
            TokenKind::Error(message) if message.contains("Invalid escape sequence")
        ));
        assert!(matches!(
            lex_string("\" \n  unterminated"),
            TokenKind::Error(message) if message.contains("Unterminated multi-line string")
        ));
        assert!(matches!(
            lex_string("\" \n  incomplete\\"),
            TokenKind::Error(message) if message.contains("incomplete escape sequence")
        ));
    }

    #[test]
    fn test_multiline_opener_is_unambiguous_with_ordinary_strings() {
        assert_eq!(lex_string("\"\""), TokenKind::StringLiteral("".into()));
        assert_eq!(
            lex_string("\"ordinary\""),
            TokenKind::StringLiteral("ordinary".into())
        );
        assert_eq!(
            lex_string("\"  ordinary\""),
            TokenKind::StringLiteral("  ordinary".into())
        );
        assert!(matches!(
            lex_string("\"   \n"),
            TokenKind::Error(message) if message.contains("Unterminated multi-line string")
        ));
    }
}
