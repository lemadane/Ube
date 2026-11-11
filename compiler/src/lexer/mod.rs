pub mod lexer;
pub mod token;

pub use lexer::Lexer;
pub use token::{Token, TokenKind};

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(source: &str) -> TokenKind { Lexer::new(source).next_token().kind }

    #[test]
    fn use_is_the_dependency_keyword() {
        assert_eq!(lex("use"), TokenKind::Use);
        assert_eq!(lex("import"), TokenKind::Identifier("import".into()));
    }

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
    fn single_quotes_cover_strings_characters_and_escapes() {
        for (source, expected) in [("'Hello'", "Hello"), ("'A'", "A"), ("''", ""),
            (r"'can\'t'", "can't"), (r"'line\nnext'", "line\nnext"), ("'λ'", "λ")] {
            assert_eq!(lex(source), TokenKind::StringLiteral(expected.into()));
        }
    }

    #[test]
    fn text_blocks_preserve_whitespace_line_endings_and_unicode() {
        assert_eq!(lex("`\n  Hello\r\nWorld\rλ\n`"),
            TokenKind::TextBlock("\n  Hello\r\nWorld\rλ\n".into()));
        assert_eq!(lex("``"), TokenKind::TextBlock(String::new()));
        assert_eq!(lex("`inline`"), TokenKind::TextBlock("inline".into()));
    }

    #[test]
    fn text_blocks_preserve_json_escapes_and_interpolation_markers() {
        assert_eq!(lex(r#"`{"name":"Ube","line":"\n","path":"C:\tmp","value":"${name}"}`"#),
            TokenKind::TextBlock(r#"{"name":"Ube","line":"\n","path":"C:\tmp","value":"${name}"}"#.into()));
        assert_eq!(lex(r"`unknown\q\u0041`"), TokenKind::TextBlock(r"unknown\q\u0041".into()));
    }

    #[test]
    fn a_backslash_does_not_escape_a_text_block_delimiter() {
        let mut lexer = Lexer::new(r"`trailing\` 'next'");
        assert_eq!(lexer.next_token().kind, TokenKind::TextBlock("trailing\\".into()));
        assert_eq!(lexer.next_token().kind, TokenKind::StringLiteral("next".into()));
    }

    #[test]
    fn reserved_double_quotes_and_unterminated_literals_report_errors() {
        assert!(matches!(lex("\"json\""), TokenKind::Error(message) if message.contains("reserved for JSON")));
        assert!(matches!(lex("`unfinished"), TokenKind::Error(message) if message.contains("Unterminated backtick")));
        assert!(matches!(lex("'unfinished"), TokenKind::Error(message) if message.contains("Unterminated single-quoted")));
        assert!(matches!(lex("'line\nnext'"), TokenKind::Error(message) if message.contains("Unescaped line break")));
    }

    #[test]
    fn semicolons_are_rejected_as_code_but_allowed_in_literals_and_comments() {
        let mut lexer = Lexer::new(";\n';' `;` // ;\n");
        let token = lexer.next_token();
        assert!(matches!(token.kind, TokenKind::Error(ref message)
            if message == "Semicolons are not allowed in Ube; remove ';'"));
        assert_eq!(token.span.start.line, 1);
        assert_eq!(token.span.start.column, 1);
        assert_eq!(lexer.next_token().kind, TokenKind::StringLiteral(";".into()));
        assert_eq!(lexer.next_token().kind, TokenKind::TextBlock(";".into()));
        assert_eq!(lexer.next_token().kind, TokenKind::Eof);
    }
}
