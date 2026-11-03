use super::token::{Token, TokenKind};
use crate::diagnostics::{Position, Span};

pub struct Lexer<'a> {
    source: &'a str,
    chars: std::str::CharIndices<'a>,
    current_char: Option<(usize, char)>,
    peek_char: Option<(usize, char)>,
    line: usize,
    column: usize,
    previous_was_cr: bool,
    in_string_interpolation: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        let mut chars = source.char_indices();
        let current_char = chars.next();
        let peek_char = chars.next();
        Self {
            source,
            chars,
            current_char,
            peek_char,
            line: 1,
            column: 1,
            previous_was_cr: false,
            in_string_interpolation: 0,
        }
    }

    fn advance(&mut self) -> Option<(usize, char)> {
        let prev = self.current_char;
        if let Some((_, ch)) = prev {
            if ch == '\r' {
                self.line += 1;
                self.column = 1;
            } else if ch == '\n' {
                if !self.previous_was_cr {
                    self.line += 1;
                }
                self.column = 1;
            } else {
                self.column += 1;
            }
            self.previous_was_cr = ch == '\r';
        }
        self.current_char = self.peek_char;
        self.peek_char = self.chars.next();
        prev
    }

    fn peek(&self) -> Option<char> {
        self.current_char.map(|(_, ch)| ch)
    }

    fn peek_next(&self) -> Option<char> {
        self.peek_char.map(|(_, ch)| ch)
    }

    fn is_multiline_string_opener(&self) -> bool {
        let Some((index, _)) = self.current_char else {
            return false;
        };
        let mut chars = self.source[index + 1..].chars();
        loop {
            match chars.next() {
                Some(' ' | '\t' | '\u{000c}') => {}
                Some('\n' | '\r') => return true,
                _ => return false,
            }
        }
    }

    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace_and_comments();

        let start_pos = Position::new(
            self.line,
            self.column,
            self.current_char
                .map(|(i, _)| i)
                .unwrap_or(self.source.len()),
        );

        let ch = match self.peek() {
            Some(c) => c,
            None => {
                let end_pos = Position::new(self.line, self.column, self.source.len());
                return Token::new(TokenKind::Eof, Span::new(start_pos, end_pos));
            }
        };

        let kind = match ch {
            '\'' => self.read_single_quoted_string(),
            '"' if self.is_multiline_string_opener() => self.read_multiline_string(),
            '"' => self.read_double_quoted_string(),
            '`' => self.read_backtick_char_literal(),
            '+' => {
                self.advance();
                TokenKind::Plus
            }
            '-' => {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::Arrow
                } else {
                    TokenKind::Minus
                }
            }
            '*' => {
                self.advance();
                TokenKind::Star
            }
            '/' => {
                self.advance();
                TokenKind::Slash
            }
            '%' => {
                self.advance();
                TokenKind::Percent
            }
            '=' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::EqualEqual
                } else {
                    TokenKind::Equal
                }
            }
            '!' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::BangEqual
                } else {
                    TokenKind::Error(
                        "Symbol '!' is not a valid operator in Ube. Use 'not' keyword.".into(),
                    )
                }
            }
            '?' => {
                self.advance();
                if self.peek() == Some('.') {
                    self.advance();
                    TokenKind::QuestionDot
                } else if self.peek() == Some('?') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::QuestionQuestionEqual
                    } else {
                        TokenKind::QuestionQuestion
                    }
                } else {
                    TokenKind::Question
                }
            }
            '<' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::LessEqual
                } else {
                    TokenKind::Less
                }
            }
            '>' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::GreaterEqual
                } else {
                    TokenKind::Greater
                }
            }
            ':' => {
                self.advance();
                TokenKind::Colon
            }
            ';' => {
                self.advance();
                TokenKind::Error("Semicolons are not allowed in Ube; remove ';'".into())
            }
            ',' => {
                self.advance();
                TokenKind::Comma
            }
            '.' => {
                self.advance();
                if self.peek() == Some('.') {
                    self.advance();
                    TokenKind::DotDot
                } else {
                    TokenKind::Dot
                }
            }
            '(' => {
                self.advance();
                TokenKind::OpenParen
            }
            ')' => {
                self.advance();
                TokenKind::CloseParen
            }
            '{' => {
                self.advance();
                if self.in_string_interpolation > 0 {
                    self.in_string_interpolation += 1;
                }
                TokenKind::OpenBrace
            }
            '}' => {
                self.advance();
                if self.in_string_interpolation > 0 {
                    self.in_string_interpolation -= 1;
                }
                TokenKind::CloseBrace
            }
            '[' => {
                self.advance();
                TokenKind::OpenBracket
            }
            ']' => {
                self.advance();
                TokenKind::CloseBracket
            }
            '0'..='9' => self.read_number(),
            c if c.is_alphabetic() || c == '_' => self.read_identifier(),
            unexpected => {
                self.advance();
                TokenKind::Error(format!("Unexpected character '{}'", unexpected))
            }
        };

        let end_pos = Position::new(
            self.line,
            self.column,
            self.current_char
                .map(|(i, _)| i)
                .unwrap_or(self.source.len()),
        );
        Token::new(kind, Span::new(start_pos, end_pos))
    }

    fn skip_whitespace_and_comments(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.advance();
            } else if ch == '/' && self.peek_next() == Some('/') {
                while let Some(c) = self.peek() {
                    self.advance();
                    if c == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    fn read_single_quoted_string(&mut self) -> TokenKind {
        self.advance();
        let mut content = String::new();

        while let Some(ch) = self.peek() {
            match ch {
                '\'' => {
                    self.advance();
                    return TokenKind::StringLiteral(content);
                }
                '\\' => {
                    self.advance();
                    match self.peek() {
                        Some('\'') => content.push('\''),
                        Some('"') => content.push('"'),
                        Some('\\') => content.push('\\'),
                        Some('n') => content.push('\n'),
                        Some('t') => content.push('\t'),
                        Some('r') => content.push('\r'),
                        Some('$') => content.push('$'),
                        Some(other) => content.push(other),
                        None => break,
                    }
                    self.advance();
                }
                _ => {
                    content.push(ch);
                    self.advance();
                }
            }
        }

        TokenKind::Error("Unterminated single-quoted string literal".into())
    }

    fn read_double_quoted_string(&mut self) -> TokenKind {
        self.advance();
        let mut content = String::new();

        while let Some(ch) = self.peek() {
            match ch {
                '"' => {
                    self.advance();
                    return TokenKind::StringLiteral(content);
                }
                '\\' => {
                    self.advance();
                    match self.peek() {
                        Some('"') => content.push('"'),
                        Some('\\') => content.push('\\'),
                        Some('n') => content.push('\n'),
                        Some('t') => content.push('\t'),
                        Some('r') => content.push('\r'),
                        Some('$') => content.push('$'),
                        Some(other) => content.push(other),
                        None => break,
                    }
                    self.advance();
                }
                '\n' | '\r' => {
                    return TokenKind::Error(
                        "Unescaped line break in single-line string literal".into(),
                    )
                }
                _ => {
                    content.push(ch);
                    self.advance();
                }
            }
        }

        TokenKind::Error("Unterminated double-quoted string literal".into())
    }

    fn read_multiline_string(&mut self) -> TokenKind {
        self.advance();
        while matches!(self.peek(), Some(' ' | '\t' | '\u{000c}')) {
            self.advance();
        }
        match self.peek() {
            Some('\n') => {
                self.advance();
            }
            Some('\r') => {
                self.advance();
                if self.peek() == Some('\n') {
                    self.advance();
                }
            }
            _ => {
                return TokenKind::Error(
                    "Text block opening delimiter must be followed by a line break".into(),
                )
            }
        }

        let mut content = String::new();
        while self.peek().is_some() {
            if self.peek() == Some('"')
                && content.chars().rev().take_while(|ch| *ch == '\\').count() % 2 == 0
            {
                self.advance();
                let normalized = Self::normalize_multiline_string(&content);
                return Self::decode_multiline_escapes(&normalized);
            }
            if let Some(ch) = self.advance().map(|(_, ch)| ch) {
                content.push(ch);
            }
        }

        if content.ends_with('\\') {
            TokenKind::Error(
                "Unterminated multi-line string literal after an incomplete escape sequence"
                    .into(),
            )
        } else {
            TokenKind::Error("Unterminated multi-line string literal".into())
        }
    }

    fn normalize_multiline_string(content: &str) -> String {
        let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
        let lines: Vec<&str> = normalized.split('\n').collect();
        let indentation = |line: &str| {
            line.chars()
                .take_while(|ch| matches!(ch, ' ' | '\t' | '\u{000c}'))
                .count()
        };
        let indent = lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| indentation(line))
            .chain(lines.last().map(|line| indentation(line)))
            .min()
            .unwrap_or(0);

        let mut result = String::new();
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                result.push('\n');
            }
            if line.trim().is_empty() {
                continue;
            }
            let skip = line
                .char_indices()
                .nth(indent.min(indentation(line)))
                .map(|(byte_index, _)| byte_index)
                .unwrap_or(line.len());
            result.push_str(&line[skip..]);
        }

        result
    }

    fn decode_multiline_escapes(content: &str) -> TokenKind {
        let chars: Vec<char> = content.chars().collect();
        let mut result = String::new();
        let mut index = 0;
        while index < chars.len() {
            let ch = chars[index];
            index += 1;
            if ch != '\\' {
                result.push(ch);
                continue;
            }

            let Some(escape) = chars.get(index).copied() else {
                return TokenKind::Error(
                    "Multi-line string ends with an incomplete escape sequence".into(),
                );
            };
            index += 1;
            match escape {
                '\n' => {}
                'b' => result.push('\u{0008}'),
                't' => result.push('\t'),
                'n' => result.push('\n'),
                'f' => result.push('\u{000c}'),
                'r' => result.push('\r'),
                '"' => result.push('"'),
                '\'' => result.push('\''),
                '\\' => result.push('\\'),
                's' => result.push(' '),
                '0'..='7' => {
                    let mut value = escape.to_digit(8).unwrap();
                    let max_digits = if escape <= '3' { 3 } else { 2 };
                    let mut digits = 1;
                    while digits < max_digits
                        && chars
                            .get(index)
                            .is_some_and(|next| matches!(next, '0'..='7'))
                    {
                        value = value * 8 + chars[index].to_digit(8).unwrap();
                        index += 1;
                        digits += 1;
                    }
                    let Some(decoded) = char::from_u32(value) else {
                        return TokenKind::Error("Invalid octal escape sequence".into());
                    };
                    result.push(decoded);
                }
                'u' => {
                    let Some(code_point) = Self::read_hex_escape(&chars, &mut index, 4) else {
                        return TokenKind::Error(
                            "Unicode escape must contain exactly four hexadecimal digits".into(),
                        );
                    };
                    let code_point = if (0xD800..=0xDBFF).contains(&code_point) {
                        if chars.get(index..index + 2) != Some(&['\\', 'u'][..]) {
                            return TokenKind::Error(
                                "High surrogate Unicode escape must be followed by a low surrogate"
                                    .into(),
                            );
                        }
                        index += 2;
                        let Some(low) = Self::read_hex_escape(&chars, &mut index, 4) else {
                            return TokenKind::Error("Invalid low-surrogate Unicode escape".into());
                        };
                        if !(0xDC00..=0xDFFF).contains(&low) {
                            return TokenKind::Error(
                                "High surrogate Unicode escape must be followed by a low surrogate"
                                    .into(),
                            );
                        }
                        0x10000 + ((code_point - 0xD800) << 10) + (low - 0xDC00)
                    } else {
                        code_point
                    };
                    let Some(decoded) = char::from_u32(code_point) else {
                        return TokenKind::Error("Invalid Unicode escape sequence".into());
                    };
                    result.push(decoded);
                }
                other => {
                    return TokenKind::Error(format!(
                        "Invalid escape sequence '\\{}' in multi-line string",
                        other
                    ))
                }
            }
        }

        TokenKind::StringLiteral(result)
    }

    fn read_hex_escape(chars: &[char], index: &mut usize, digits: usize) -> Option<u32> {
        let end = index.checked_add(digits)?;
        let hex: String = chars.get(*index..end)?.iter().collect();
        if !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return None;
        }
        *index = end;
        u32::from_str_radix(&hex, 16).ok()
    }

    fn read_backtick_char_literal(&mut self) -> TokenKind {
        self.advance();
        let mut content = String::new();

        while let Some(ch) = self.peek() {
            match ch {
                '`' => {
                    self.advance();
                    return TokenKind::StringLiteral(content);
                }
                '\\' => {
                    self.advance();
                    match self.peek() {
                        Some('`') => {
                            self.advance();
                            content.push('`');
                        }
                        Some('\\') => {
                            self.advance();
                            content.push('\\');
                        }
                        Some('n') => {
                            self.advance();
                            content.push('\n');
                        }
                        Some('t') => {
                            self.advance();
                            content.push('\t');
                        }
                        Some('r') => {
                            self.advance();
                            content.push('\r');
                        }
                        Some('$') => {
                            self.advance();
                            content.push('$');
                        }
                        Some(other) => {
                            self.advance();
                            content.push(other);
                        }
                        None => break,
                    }
                }
                _ => {
                    content.push(ch);
                    self.advance();
                }
            }
        }

        TokenKind::Error("Unterminated backtick character literal".into())
    }

    fn read_number(&mut self) -> TokenKind {
        let start_idx = self.current_char.map(|(i, _)| i).unwrap_or(0);
        let mut is_decimal = false;
        let mut is_long = false;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() || ch == '_' {
                self.advance();
            } else if ch == '.'
                && !is_decimal
                && self.peek_next().map_or(false, |c| c.is_ascii_digit())
            {
                is_decimal = true;
                self.advance();
            } else if (ch == 'L' || ch == 'l') && !is_decimal {
                is_long = true;
                self.advance();
                break;
            } else {
                break;
            }
        }

        let end_idx = self
            .current_char
            .map(|(i, _)| i)
            .unwrap_or(self.source.len());
        let raw_num_str = &self.source[start_idx..end_idx];
        let cleaned_num_str = raw_num_str
            .replace('_', "")
            .trim_end_matches(['L', 'l'])
            .to_string();

        if is_decimal {
            match cleaned_num_str.parse::<f64>() {
                Ok(val) => TokenKind::DecimalLiteral(val),
                Err(_) => TokenKind::Error(format!("Invalid decimal literal '{}'", raw_num_str)),
            }
        } else if is_long {
            match cleaned_num_str.parse::<i64>() {
                Ok(val) => TokenKind::LongLiteral(val),
                Err(_) => TokenKind::Error(format!("Invalid long literal '{}'", raw_num_str)),
            }
        } else {
            match cleaned_num_str.parse::<i64>() {
                Ok(val) => TokenKind::IntLiteral(val),
                Err(_) => TokenKind::Error(format!("Invalid integer literal '{}'", raw_num_str)),
            }
        }
    }

    fn read_identifier(&mut self) -> TokenKind {
        let start_idx = self.current_char.map(|(i, _)| i).unwrap_or(0);

        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }

        let end_idx = self
            .current_char
            .map(|(i, _)| i)
            .unwrap_or(self.source.len());
        let ident_str = &self.source[start_idx..end_idx];

        match ident_str {
            "class" => TokenKind::Class,
            "trait" => TokenKind::Trait,
            "final" => TokenKind::Final,
            "var" => TokenKind::Var,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "for" => TokenKind::For,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "switch" => TokenKind::Switch,
            "case" => TokenKind::Case,
            "default" => TokenKind::Default,
            "yield" => TokenKind::Yield,
            "return" => TokenKind::Return,
            "import" => TokenKind::Import,
            "as" => TokenKind::As,
            "is" => TokenKind::Is,
            "not" => TokenKind::Not,
            "and" => TokenKind::And,
            "nand" => TokenKind::Nand,
            "or" => TokenKind::Or,
            "nor" => TokenKind::Nor,
            "xor" => TokenKind::Xor,
            "xnor" => TokenKind::Xnor,
            "null" => TokenKind::Null,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "this" => TokenKind::This,
            "with" => TokenKind::With,
            "in" => TokenKind::In,
            "print" => TokenKind::Print,
            "try" => TokenKind::Try,
            "check" => TokenKind::Check,
            "throw" => TokenKind::Throw,
            "catch" => TokenKind::Catch,
            "finally" => TokenKind::Finally,
            "async" => TokenKind::Async,
            "await" => TokenKind::Await,
            "string" => TokenKind::StringType,
            "int" => TokenKind::IntType,
            "decimal" => TokenKind::DecimalType,
            "byte" => TokenKind::ByteType,
            "long" => TokenKind::LongType,
            "bool" => TokenKind::BoolType,
            _ => TokenKind::Identifier(ident_str.to_string()),
        }
    }
}
