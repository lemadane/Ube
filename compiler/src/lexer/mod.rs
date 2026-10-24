pub mod lexer;
pub mod token;

pub use lexer::Lexer;
pub use token::{Token, TokenKind};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lex_ube_keywords_and_backtick_strings() {
        let code = r#"
            class User(long id, string name, var int age = 0) {
                void birthday() {
                    this._age = this._age + 1
                }
            }

            final message = `Hello, ${user.name()}`
            if (a and not b or c xor d) {
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
        assert!(tokens.contains(&TokenKind::Xor));
        assert!(tokens.contains(&TokenKind::QuestionDot));
    }
}
