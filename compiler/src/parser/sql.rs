//! Conservative lexical checks for connection-independent SQL templates.
//! The database remains responsible for SQL grammar and schema validation.
use super::{Expr, ExprKind};

pub(super) fn validate(parts: &[Expr], literals: &[bool]) -> Result<(), String> {
    let mut sql = String::new();
    for (part, literal) in parts.iter().zip(literals) {
        if *literal {
            let ExprKind::TextBlock(value) = &part.kind else {
                unreachable!()
            };
            if value.contains('\0') {
                return Err("NUL is not allowed in SQL templates".into());
            }
            sql.push_str(value);
        } else {
            sql.push('\0');
        }
    }
    if sql.trim().is_empty() {
        return Err("SQL templates must not be empty".into());
    }
    // Validate every supported quoting mode: no connection is known at compile time.
    for mode in [
        Mode::Postgres,
        Mode::Mysql,
        Mode::MysqlNoBackslash,
        Mode::Sqlite,
    ] {
        scan(&sql, mode)?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Postgres,
    Mysql,
    MysqlNoBackslash,
    Sqlite,
}

fn scan(sql: &str, mode: Mode) -> Result<(), String> {
    let b = sql.as_bytes();
    let mut i = 0;
    let mut previous_word = String::new();
    let identifier = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 128;
    while i < b.len() {
        if b[i] == 0 {
            if i > 0 && identifier(b[i - 1]) || b.get(i + 1).is_some_and(|c| identifier(*c)) {
                return Err("SQL interpolation must occupy a complete value position".into());
            }
            if [
                "FROM", "JOIN", "UPDATE", "INTO", "TABLE", "SCHEMA", "DATABASE", "INDEX", "VIEW",
                "COLLATE",
            ]
            .contains(&previous_word.as_str())
            {
                return Err(
                    "SQL identifiers must be static; interpolation binds values only".into(),
                );
            }
            previous_word.clear();
            i += 1;
        } else if b[i..].starts_with(b"--")
            || matches!(mode, Mode::Mysql | Mode::MysqlNoBackslash) && b[i] == b'#'
        {
            let start = i;
            while i < b.len() && !matches!(b[i], b'\n' | b'\r') {
                i += 1;
            }
            static_context(&sql[start..i])?;
        } else if b[i..].starts_with(b"/*") {
            let start = i;
            if b.get(i + 2) == Some(&b'!') || b[i..].starts_with(b"/*M!") {
                return Err("Executable SQL comments are not supported in SQL templates".into());
            }
            i += 2;
            let mut depth = 1;
            while i < b.len() && depth > 0 {
                if !matches!(mode, Mode::Mysql | Mode::MysqlNoBackslash)
                    && b[i..].starts_with(b"/*")
                {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            static_context(&sql[start..i])?;
            if depth != 0 {
                return Err("Unterminated SQL comment".into());
            }
        } else if matches!(b[i], b'\'' | b'"' | b'`') || mode == Mode::Sqlite && b[i] == b'[' {
            let start = i;
            let quote = b[i];
            let end = if quote == b'[' { b']' } else { quote };
            let escapes = mode == Mode::Mysql
                || mode == Mode::Postgres
                    && quote == b'\''
                    && i > 0
                    && matches!(b[i - 1], b'e' | b'E');
            i += 1;
            let mut closed = false;
            while i < b.len() {
                if escapes && b[i] == b'\\' {
                    i = (i + 2).min(b.len());
                } else if b[i] == end {
                    i += 1;
                    if quote != b'[' && b.get(i) == Some(&end) {
                        i += 1;
                    } else {
                        closed = true;
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            static_context(&sql[start..i])?;
            if !closed {
                return Err("Unterminated or dialect-ambiguous SQL quote".into());
            }
        } else if mode == Mode::Postgres
            && b[i] == b'$'
            && b.get(i + 1)
                .is_some_and(|c| *c == b'$' || c.is_ascii_alphabetic() || *c == b'_')
        {
            let start = i;
            i += 1;
            while b
                .get(i)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                i += 1;
            }
            if b.get(i) != Some(&b'$') {
                return Err("Native SQL placeholders are not supported; use ${value}".into());
            }
            let marker = &sql[start..=i];
            i += 1;
            let end = sql[i..]
                .find(marker)
                .ok_or("Unterminated dollar-quoted SQL value")?
                + i
                + marker.len();
            static_context(&sql[start..end])?;
            i = end;
        } else if b[i] == b'?'
            || b[i] == b'$' && b.get(i + 1).is_some_and(u8::is_ascii_digit)
            || matches!(b[i], b':' | b'@')
                && b.get(i + 1).is_some_and(|c| identifier(*c))
                && (i == 0 || b[i - 1] != b':')
        {
            return Err("Native SQL placeholders are not supported; use ${value}".into());
        } else if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            let start = i;
            while b
                .get(i)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                i += 1;
            }
            previous_word = sql[start..i].to_ascii_uppercase();
        } else {
            if !b[i].is_ascii_whitespace() {
                previous_word.clear();
            }
            i += 1;
        }
    }
    Ok(())
}

fn static_context(text: &str) -> Result<(), String> {
    if text.contains('\0') {
        Err("SQL interpolation inside quotes, identifiers, or comments is not supported".into())
    } else {
        Ok(())
    }
}
