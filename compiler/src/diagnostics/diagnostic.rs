use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Position {
    pub line: usize,
    pub column: usize,
    pub index: usize,
}

impl Position {
    pub fn new(line: usize, column: usize, index: usize) -> Self {
        Self {
            line,
            column,
            index,
        }
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Span {
    pub start: Position,
    pub end: Position,
}

impl Span {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    pub fn combine(start: Span, end: Span) -> Self {
        Self {
            start: start.start,
            end: end.end,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Error,
    Warning,
    Note,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
    pub span: Option<Span>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            level: DiagnosticLevel::Error,
            message: message.into(),
            span,
        }
    }

    pub fn warning(message: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            level: DiagnosticLevel::Warning,
            message: message.into(),
            span,
        }
    }
}
