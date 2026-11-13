pub mod lexer;
pub mod token;

pub use lexer::Lexer;
pub use token::{Token, TokenKind};

// Find the closing brace using tokens so braces inside strings and nested
// templates cannot terminate an interpolation expression.
pub(crate) fn interpolation_end(source: &str) -> Result<usize, String> {
    let mut lexer = Lexer::new(source);
    let mut depth = 0;
    loop {
        let token = lexer.next_token();
        match token.kind {
            TokenKind::OpenBrace => depth += 1,
            TokenKind::CloseBrace if depth == 0 => return Ok(token.span.start.index),
            TokenKind::CloseBrace => depth -= 1,
            TokenKind::Eof => return Err("Unterminated text block interpolation; expected '}'".into()),
            TokenKind::Error(message) => return Err(message),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(source: &str) -> TokenKind { Lexer::new(source).next_token().kind }

    #[test]
    fn loop_keywords_distinguish_while_each_and_for() {
        assert_eq!(lex("while"), TokenKind::While);
        assert_eq!(lex("each"), TokenKind::Each);
        assert_eq!(lex("for"), TokenKind::For);
        assert_eq!(lex("from"), TokenKind::Identifier("from".into()));
    }

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
    fn text_block_tokens_preserve_escapes_for_template_parsing() {
        assert_eq!(lex(r#"`{"name":"Ube","line":"\n","path":"C:\tmp","value":"${name}"}`"#),
            TokenKind::TextBlock(r#"{"name":"Ube","line":"\n","path":"C:\tmp","value":"${name}"}"#.into()));
        assert_eq!(lex(r"`unknown\q\u0041`"), TokenKind::TextBlock(r"unknown\q\u0041".into()));
    }

    #[test]
    fn a_backslash_escapes_a_text_block_delimiter() {
        let mut lexer = Lexer::new(r"`escaped\` tick` 'next'");
        assert_eq!(lexer.next_token().kind, TokenKind::TextBlock(r"escaped\` tick".into()));
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
