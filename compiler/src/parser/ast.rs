use crate::diagnostics::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TypeSpec {
    Int,
    Decimal,
    Byte,
    Long,
    Bool,
    String,
    Class(String),
    Generic(String, Box<TypeSpec>),
    Nullable(Box<TypeSpec>),
    Function {
        params: Vec<TypeSpec>,
        return_type: Box<TypeSpec>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstructorParam {
    pub name: String,
    pub type_spec: TypeSpec,
    pub is_var: bool,
    pub default_value: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub type_spec: TypeSpec,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MethodDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeSpec>, // None means implicit void
    pub body: Vec<Stmt>,
    pub is_async: bool,
    pub is_static: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaseDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StaticFieldDecl {
    pub name: String,
    pub type_spec: TypeSpec,
    pub is_mutable: bool,
    pub initializer: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassDecl {
    pub name: String,
    pub primary_params: Vec<ConstructorParam>,
    pub static_fields: Vec<StaticFieldDecl>,
    pub cases: Vec<CaseDecl>,
    pub methods: Vec<MethodDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub path: Vec<String>,
    pub symbols: Vec<(String, Option<String>)>,
    pub package_alias: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitDecl {
    pub name: String,
    pub methods: Vec<MethodDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Class(ClassDecl),
    Trait(TraitDecl),
    Import(ImportDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
    NullCoalescing,
    Range, // ..
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    pub pattern: Expr,
    pub guard: Option<Expr>,
    pub body: Vec<Stmt>,
    pub result_expr: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    IntLiteral(i64),
    LongLiteral(i64),
    DecimalLiteral(f64),
    StringLiteral(String),
    TextBlock(String),
    TemplateLiteral(Vec<Expr>),
    // Context: 0 literal markup, 1 text, 2 quoted attribute, 3 complete URL.
    HtmlTemplate(Vec<(Expr, u8)>),
    SqlTemplate(Vec<(Expr, bool)>),
    // Internal iterable for a for-range loop; never materialized as a collection.
    RangeIterator { start: Box<Expr>, end: Box<Expr>, step: Box<Expr> },
    BoolLiteral(bool),
    NullLiteral,
    Identifier(String),
    This,
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    MemberAccess {
        object: Box<Expr>,
        member: String,
        is_optional: bool,
    },
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    NullCoalescingAssign {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    ClassInstantiation {
        class_name: String,
        generic_type: Option<TypeSpec>,
        args: Vec<Expr>,
    },
    AnonymousObject {
        fields: Vec<(String, Expr)>,
    },
    WithUpdate {
        base: Box<Expr>,
        updates: Vec<(String, Expr)>,
    },
    IsCheck {
        expr: Box<Expr>,
        target_type: TypeSpec,
        is_negated: bool,
    },
    SwitchExpr {
        target: Box<Expr>,
        cases: Vec<SwitchCase>,
        default_case: Option<Box<Expr>>,
    },
    ListLiteral(Vec<Expr>),
    MapLiteral(Vec<(Expr, Expr)>),
    IndexAccess {
        target: Box<Expr>,
        index: Box<Expr>,
    },
    AsyncSpawn(Box<Expr>),
    Await(Box<Expr>),
    Lambda {
        parameter: String,
        body: Vec<Stmt>,
    },
    BlockClosure {
        params: Vec<Param>,
        body: Vec<Stmt>,
        // Shared across AST normalization clones; filled by semantic checking before lowering.
        return_type: std::rc::Rc<std::cell::RefCell<Option<crate::types::Type>>>,
    },
    Closure {
        params: Vec<Param>,
        expression: Box<Expr>,
    },
    Conditional {
        condition: Box<Expr>,
        when_true: Box<Expr>,
        when_false: Box<Expr>,
        result_type: std::rc::Rc<std::cell::RefCell<Option<crate::types::Type>>>,
    },
    If {
        condition: Box<Expr>,
        then_branch: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
    For {
        loop_var: Option<String>,
        index_var: Option<String>,
        condition_or_iterable: Option<Box<Expr>>,
        body: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    LocalDecl {
        name: String,
        is_mutable: bool,
        type_annotation: Option<TypeSpec>,
        initializer: Option<Expr>,
    },
    Expr(Expr),
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
    For {
        loop_var: Option<String>,
        index_var: Option<String>,
        condition_or_iterable: Option<Expr>,
        body: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
    Break,
    Continue,
    Return(Option<Expr>),
    Yield(Expr),
    Try {
        body: Vec<Stmt>,
        catches: Vec<CatchClause>,
        finally_body: Option<Vec<Stmt>>,
    },
    Throw(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause {
    pub type_name: String,
    pub variable: String,
    pub condition: Option<Expr>,
    pub body: Vec<Stmt>,
    pub span: Span,
}
