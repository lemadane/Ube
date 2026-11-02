use crate::diagnostics::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Keywords
    Class,
    Trait,
    Final,
    Var,
    If,
    Else,
    For,
    Break,
    Continue,
    Switch,
    Case,
    Default,
    Yield,
    Return,
    Import,
    As,
    Is,
    Not,
    And,
    Nand,
    Or,
    Nor,
    Xor,
    Xnor,
    Null,
    True,
    False,
    This,
    With,
    In,
    Print,
    Guard,
    Check,
    Throw,
    Catch,
    Exit,
    Async,
    Await,
    After,

    // Primitive Type Keywords
    StringType,
    IntType,
    DecimalType,
    ByteType,
    LongType,
    BoolType,

    // Literals & Identifiers
    IntLiteral(i64),
    LongLiteral(i64),
    DecimalLiteral(f64),
    StringLiteral(String),
    Identifier(String),

    // String Interpolation Tokens
    InterpolationStart, // ${
    InterpolationEnd,   // }

    // Operators & Punctuation
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Equal,
    EqualEqual,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Question,
    QuestionDot,           // ?.
    QuestionQuestion,      // ??
    QuestionQuestionEqual, // ??=
    Arrow,                 // ->
    Colon,
    Semicolon,
    Comma,
    Dot,
    DotDot, // ..
    OpenParen,
    CloseParen,
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,

    // Special
    Eof,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}
