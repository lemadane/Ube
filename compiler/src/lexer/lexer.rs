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
            '"' => {
                self.advance();
                TokenKind::Error("Double quotes are reserved for JSON; use single quotes for strings or backticks for text blocks".into())
            }
            '`' => self.read_text_block(),
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
                        Some('\n' | '\r') => return TokenKind::Error("Unescaped line break in single-quoted string; use a backtick text block".into()),
                        Some(other) => content.push(other),
                        None => break,
                    }
                    self.advance();
                }
                '\n' | '\r' => {
                    return TokenKind::Error("Unescaped line break in single-quoted string; use a backtick text block".into())
                }
                _ => {
                    content.push(ch);
                    self.advance();
                }
            }
        }

        TokenKind::Error("Unterminated single-quoted string literal".into())
    }

    fn read_text_block(&mut self) -> TokenKind {
        self.advance();
        let mut content = String::new();
        while let Some(ch) = self.peek() {
            self.advance();
            if ch == '`' { return TokenKind::TextBlock(content); }
            content.push(ch);
        }
        TokenKind::Error("Unterminated backtick text block".into())
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
            "or" => TokenKind::Or,
            "null" => TokenKind::Null,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "this" => TokenKind::This,
            "with" => TokenKind::With,
            "in" => TokenKind::In,
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
