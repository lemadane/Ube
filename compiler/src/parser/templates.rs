use super::{Expr, ExprKind, Parser};
use super::parser::Precedence;
use crate::diagnostics::{Diagnostic, Position, Span};
use crate::lexer::{interpolation_end, Lexer, TokenKind};

impl Parser<'_> {
    pub(super) fn parse_text_block(&mut self, source: &str, span: Span) -> Result<Expr, ()> {
        self.parse_template(source, span).map_err(|message| {
            self.diagnostics.push(Diagnostic::error(message, Some(span)));
        })
    }

    fn parse_template(&mut self, source: &str, span: Span) -> Result<Expr, String> {
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut cursor = 0;
        while cursor < source.len() {
            let ch = source[cursor..].chars().next().unwrap();
            cursor += ch.len_utf8();
            if ch == '\\' {
                let escaped = source[cursor..].chars().next().ok_or("Unterminated text block escape")?;
                cursor += escaped.len_utf8();
                match escaped {
                    'n' => literal.push('\n'),
                    'r' => literal.push('\r'),
                    't' => literal.push('\t'),
                    'b' => literal.push('\u{0008}'),
                    'f' => literal.push('\u{000c}'),
                    'v' => literal.push('\u{000b}'),
                    '\n' => {},
                    '\r' => { if source[cursor..].starts_with('\n') { cursor += 1; } },
                    'x' | 'u' => {
                        let braced = escaped == 'u' && source[cursor..].starts_with('{');
                        let start = cursor + usize::from(braced);
                        let end = if braced {
                            start + source[start..].find('}').ok_or("Unterminated Unicode escape")?
                        } else {
                            start + if escaped == 'x' { 2 } else { 4 }
                        };
                        let digits = source.get(start..end).ok_or("Invalid hexadecimal escape")?;
                        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
                            return Err("Invalid hexadecimal escape".into());
                        }
                        let value = u32::from_str_radix(digits, 16).ok()
                            .and_then(char::from_u32).ok_or("Invalid Unicode scalar in text block escape")?;
                        if value == '\0' { return Err("NUL escapes are not supported in Ube strings".into()); }
                        literal.push(value);
                        cursor = end + usize::from(braced);
                    }
                    '0'..='9' => return Err("Numeric escapes are not supported in Ube text blocks".into()),
                    other => literal.push(other),
                }
            } else if ch == '$' && source[cursor..].starts_with('{') {
                cursor += 1;
                if !literal.is_empty() {
                    parts.push(Expr { kind: ExprKind::TextBlock(std::mem::take(&mut literal)), span });
                }
                let end = cursor + interpolation_end(&source[cursor..])?;
                let origin = template_position(source, cursor, span);
                let mut parser = Parser::new(Lexer::at_position(&source[cursor..end], origin));
                let expression = parser.parse_expr(Precedence::None);
                if let Some(diagnostic) = parser.diagnostics.first() {
                    return Err(format!("Invalid text block interpolation: {}", diagnostic.message));
                }
                let expression = expression.map_err(|_| "Expected an expression inside ${...}")?;
                if parser.current_token.kind != TokenKind::Eof {
                    return Err("Expected one expression inside ${...}".into());
                }
                parts.push(expression);
                cursor = end + 1;
            } else {
                literal.push(ch);
            }
        }
        if parts.is_empty() {
            return Ok(Expr { kind: ExprKind::TextBlock(literal), span });
        }
        if !literal.is_empty() {
            parts.push(Expr { kind: ExprKind::TextBlock(literal), span });
        }
        Ok(Expr { kind: ExprKind::TemplateLiteral(parts), span })
    }
}

fn template_position(source: &str, offset: usize, span: Span) -> Position {
    let mut position = span.start;
    position.column += 1; // Opening backtick.
    position.index += 1 + offset;
    let mut previous_cr = false;
    for ch in source[..offset].chars() {
        if ch == '\r' {
            position.line += 1;
            position.column = 1;
        } else if ch == '\n' {
            if !previous_cr { position.line += 1; }
            position.column = 1;
        } else {
            position.column += 1;
        }
        previous_cr = ch == '\r';
    }
    position
}
