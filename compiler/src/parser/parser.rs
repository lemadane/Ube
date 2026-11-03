use super::ast::*;
use crate::builtins::canonical_builtin_class_name;
use crate::diagnostics::{Diagnostic, Span};
use crate::lexer::{Lexer, Token, TokenKind};

#[derive(PartialEq, PartialOrd, Clone, Copy)]
pub enum Precedence {
    None,
    Assignment,     // =, ??=
    With,           // with
    NullCoalescing, // ??
    Or,             // or, nor
    Xor,            // xor, xnor
    And,            // and, nand
    Equality,       // ==, !=, is, is not
    Comparison,     // <, <=, >, >=
    Range,          // ..
    Term,           // +, -
    Factor,         // *, /, %
    Unary,          // not, -
    Call,           // (), ., ?.
}

impl Precedence {
    fn from_token(kind: &TokenKind) -> Precedence {
        match kind {
            TokenKind::Equal | TokenKind::QuestionQuestionEqual => Precedence::Assignment,
            TokenKind::With => Precedence::With,
            TokenKind::QuestionQuestion => Precedence::NullCoalescing,
            TokenKind::Or | TokenKind::Nor => Precedence::Or,
            TokenKind::Xor | TokenKind::Xnor => Precedence::Xor,
            TokenKind::And | TokenKind::Nand => Precedence::And,
            TokenKind::EqualEqual | TokenKind::BangEqual | TokenKind::Is => Precedence::Equality,
            TokenKind::Less
            | TokenKind::LessEqual
            | TokenKind::Greater
            | TokenKind::GreaterEqual => Precedence::Comparison,
            TokenKind::DotDot => Precedence::Range,
            TokenKind::Plus | TokenKind::Minus => Precedence::Term,
            TokenKind::Star | TokenKind::Slash | TokenKind::Percent => Precedence::Factor,
            TokenKind::OpenParen
            | TokenKind::OpenBracket
            | TokenKind::Dot
            | TokenKind::QuestionDot => Precedence::Call,
            _ => Precedence::None,
        }
    }
}

pub struct Parser<'a> {
    lexer: Lexer<'a>,
    in_trait: bool,
    current_token: Token,
    peek_token: Token,
    pub diagnostics: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    pub fn new(mut lexer: Lexer<'a>) -> Self {
        let mut diagnostics = Vec::new();
        let current_token = Self::read_token(&mut lexer, &mut diagnostics);
        let peek_token = Self::read_token(&mut lexer, &mut diagnostics);
        Self {
            lexer,
            in_trait: false,
            current_token,
            peek_token,
            diagnostics,
        }
    }

    fn parse_lambda_block(&mut self) -> Result<Expr, ()> {
        let start = self.expect(&TokenKind::OpenBrace, "{")?.span;
        let parameter = match self.current_token.kind.clone() {
            TokenKind::Identifier(name) => {
                self.advance();
                name
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    "Expected a lambda parameter before '->'",
                    Some(self.current_token.span),
                ));
                return Err(());
            }
        };
        self.expect(&TokenKind::Arrow, "->")?;

        let mut body = Vec::new();
        while !self.check(&TokenKind::CloseBrace) && !self.check(&TokenKind::Eof) {
            body.push(self.parse_stmt()?);
        }
        let end = self.expect(&TokenKind::CloseBrace, "}")?.span;
        Ok(Expr {
            kind: ExprKind::Lambda { parameter, body },
            span: Span::combine(start, end),
        })
    }

    fn skip_trailing_block(&mut self) -> Result<Span, ()> {
        let start = self.expect(&TokenKind::OpenBrace, "{")?.span;
        let mut depth = 1;
        let mut end = start;
        while depth > 0 && !self.check(&TokenKind::Eof) {
            if self.check(&TokenKind::OpenBrace) {
                depth += 1;
                self.advance();
            } else if self.check(&TokenKind::CloseBrace) {
                depth -= 1;
                end = self.current_token.span;
                self.advance();
            } else {
                self.advance();
            }
        }
        if depth != 0 {
            self.diagnostics
                .push(Diagnostic::error("Unclosed trailing block", Some(start)));
            return Err(());
        }
        Ok(Span::combine(start, end))
    }

    fn read_token(lexer: &mut Lexer<'a>, diagnostics: &mut Vec<Diagnostic>) -> Token {
        let token = lexer.next_token();
        if let TokenKind::Error(message) = &token.kind {
            diagnostics.push(Diagnostic::error(message.clone(), Some(token.span)));
        }
        token
    }

    fn advance(&mut self) {
        let next = Self::read_token(&mut self.lexer, &mut self.diagnostics);
        self.current_token = std::mem::replace(&mut self.peek_token, next);
    }

    fn check(&self, kind: &TokenKind) -> bool {
        &self.current_token.kind == kind
    }

    fn match_token(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind, msg: &str) -> Result<Token, ()> {
        if self.check(kind) {
            let tok = self.current_token.clone();
            self.advance();
            Ok(tok)
        } else {
            self.diagnostics.push(Diagnostic::error(
                format!("Expected {}, found {:?}", msg, self.current_token.kind),
                Some(self.current_token.span),
            ));
            Err(())
        }
    }

    pub fn parse_program(&mut self) -> Program {
        let mut items = Vec::new();

        while self.current_token.kind != TokenKind::Eof {
            if let Ok(item) = self.parse_item() {
                items.push(item);
            } else {
                self.synchronize();
            }
        }

        Program { items }
    }

    fn synchronize(&mut self) {
        self.advance();
        while self.current_token.kind != TokenKind::Eof {
            match self.current_token.kind {
                TokenKind::Class
                | TokenKind::Trait
                | TokenKind::Import
                | TokenKind::If
                | TokenKind::For
                | TokenKind::Return => return,
                _ => self.advance(),
            }
        }
    }

    fn parse_item(&mut self) -> Result<Item, ()> {
        match self.current_token.kind {
            TokenKind::Class => self.parse_class_decl().map(Item::Class),
            TokenKind::Trait => self.parse_trait_decl().map(Item::Trait),
            TokenKind::Import => self.parse_import_decl().map(Item::Import),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    format!(
                        "Unexpected top-level item starting with {:?}",
                        self.current_token.kind
                    ),
                    Some(self.current_token.span),
                ));
                Err(())
            }
        }
    }

    fn parse_trait_decl(&mut self) -> Result<TraitDecl, ()> {
        let start_span = self.current_token.span;
        self.expect(&TokenKind::Trait, "trait keyword")?;

        let trait_name = match self.current_token.kind.clone() {
            TokenKind::Identifier(name) => {
                self.advance();
                name
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    "Expected trait name",
                    Some(self.current_token.span),
                ));
                return Err(());
            }
        };

        self.expect(&TokenKind::OpenBrace, "{")?;
        let mut methods = Vec::new();
        self.in_trait = true;
        while !self.check(&TokenKind::CloseBrace) && !self.check(&TokenKind::Eof) {
            let method = self.parse_method_decl();
            if method.is_err() { self.in_trait = false; }
            methods.push(method?);
        }
        self.in_trait = false;
        let end_tok = self.expect(&TokenKind::CloseBrace, "}")?;

        Ok(TraitDecl {
            name: trait_name,
            methods,
            span: Span::combine(start_span, end_tok.span),
        })
    }

    fn parse_class_decl(&mut self) -> Result<ClassDecl, ()> {
        let start_span = self.current_token.span;
        self.expect(&TokenKind::Class, "class keyword")?;

        let class_name = match self.current_token.kind.clone() {
            TokenKind::Identifier(name) => {
                self.advance();
                name
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    "Expected class name",
                    Some(self.current_token.span),
                ));
                return Err(());
            }
        };

        let mut primary_params = Vec::new();
        if self.match_token(&TokenKind::OpenParen) {
            if !self.check(&TokenKind::CloseParen) {
                loop {
                    let param_span = self.current_token.span;
                    let is_var = self.match_token(&TokenKind::Var);
                    let type_spec = self.parse_type_spec()?;
                    let name = match self.current_token.kind.clone() {
                        TokenKind::Identifier(n) => {
                            self.advance();
                            n
                        }
                        _ => {
                            self.diagnostics.push(Diagnostic::error(
                                "Expected field name",
                                Some(self.current_token.span),
                            ));
                            return Err(());
                        }
                    };

                    let default_value = if self.match_token(&TokenKind::Equal) {
                        Some(self.parse_expr(Precedence::None)?)
                    } else {
                        None
                    };

                    primary_params.push(ConstructorParam {
                        name,
                        type_spec,
                        is_var,
                        default_value,
                        span: param_span,
                    });

                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::CloseParen, ")")?;
        }

        let mut cases = Vec::new();
        let mut methods = Vec::new();

        if self.match_token(&TokenKind::OpenBrace) {
            while !self.check(&TokenKind::CloseBrace) && !self.check(&TokenKind::Eof) {
                if self.check(&TokenKind::Case) {
                    cases.push(self.parse_case_decl()?);
                } else {
                    methods.push(self.parse_method_decl()?);
                }
            }
            let end_tok = self.expect(&TokenKind::CloseBrace, "}")?;
            Ok(ClassDecl {
                name: class_name,
                primary_params,
                cases,
                methods,
                span: Span::combine(start_span, end_tok.span),
            })
        } else {
            Ok(ClassDecl {
                name: class_name,
                primary_params,
                cases,
                methods,
                span: start_span,
            })
        }
    }

    fn parse_case_decl(&mut self) -> Result<CaseDecl, ()> {
        let start_span = self.current_token.span;
        self.expect(&TokenKind::Case, "case")?;

        let case_name = match self.current_token.kind.clone() {
            TokenKind::Identifier(n) => {
                self.advance();
                n
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    "Expected case name",
                    Some(self.current_token.span),
                ));
                return Err(());
            }
        };

        let mut params = Vec::new();
        if self.match_token(&TokenKind::OpenParen) {
            if !self.check(&TokenKind::CloseParen) {
                loop {
                    let param_span = self.current_token.span;
                    let param_type = self.parse_type_spec()?;
                    let param_name = match self.current_token.kind.clone() {
                        TokenKind::Identifier(n) => {
                            self.advance();
                            n
                        }
                        _ => {
                            self.diagnostics.push(Diagnostic::error(
                                "Expected parameter name",
                                Some(self.current_token.span),
                            ));
                            return Err(());
                        }
                    };
                    params.push(Param {
                        name: param_name,
                        type_spec: param_type,
                        span: param_span,
                    });
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::CloseParen, ")")?;
        }

        Ok(CaseDecl {
            name: case_name,
            params,
            span: Span::combine(start_span, self.current_token.span),
        })
    }

    fn parse_method_decl(&mut self) -> Result<MethodDecl, ()> {
        let start_span = self.current_token.span;
        let is_async = self.match_token(&TokenKind::Async);

        // Check if return type is omitted: identifier followed directly by '('
        let (return_type, method_name) = match (&self.current_token.kind, &self.peek_token.kind) {
            (TokenKind::Identifier(name), TokenKind::OpenParen) => {
                let mname = name.clone();
                self.advance();
                (None, mname)
            }
            _ => {
                let ret_type = self.parse_type_spec()?;
                let mname = match self.current_token.kind.clone() {
                    TokenKind::Identifier(name) => {
                        self.advance();
                        name
                    }
                    _ => {
                        self.diagnostics.push(Diagnostic::error(
                            "Expected method name",
                            Some(self.current_token.span),
                        ));
                        return Err(());
                    }
                };
                (Some(ret_type), mname)
            }
        };

        self.expect(&TokenKind::OpenParen, "(")?;
        let mut params = Vec::new();
        if !self.check(&TokenKind::CloseParen) {
            loop {
                let param_span = self.current_token.span;
                let param_type = self.parse_type_spec()?;
                let param_name = match self.current_token.kind.clone() {
                    TokenKind::Identifier(n) => {
                        self.advance();
                        n
                    }
                    _ => {
                        self.diagnostics.push(Diagnostic::error(
                            "Expected parameter name",
                            Some(self.current_token.span),
                        ));
                        return Err(());
                    }
                };
                params.push(Param {
                    name: param_name,
                    type_spec: param_type,
                    span: param_span,
                });
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::CloseParen, ")")?;

        let mut body = Vec::new();
        let end_span = if self.in_trait && !self.check(&TokenKind::OpenBrace) {
            self.current_token.span
        } else {
            self.expect(&TokenKind::OpenBrace, "{")?;
            while !self.check(&TokenKind::CloseBrace) && !self.check(&TokenKind::Eof) {
                body.push(self.parse_stmt()?);
            }
            self.expect(&TokenKind::CloseBrace, "}")?.span
        };

        Ok(MethodDecl {
            name: method_name,
            params,
            return_type,
            body,
            is_async,
            span: Span::combine(start_span, end_span),
        })
    }

    fn parse_type_spec(&mut self) -> Result<TypeSpec, ()> {
        let tok = self.current_token.clone();
        self.advance();
        let mut base_type = match tok.kind {
            TokenKind::IntType => TypeSpec::Int,
            TokenKind::DecimalType => TypeSpec::Decimal,
            TokenKind::ByteType => TypeSpec::Byte,
            TokenKind::LongType => TypeSpec::Long,
            TokenKind::BoolType => TypeSpec::Bool,
            TokenKind::StringType => TypeSpec::String,
            TokenKind::Identifier(name) => TypeSpec::Class(
                canonical_builtin_class_name(&name)
                    .unwrap_or(&name)
                    .to_string(),
            ),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    format!("Expected type specifier, found {:?}", tok.kind),
                    Some(tok.span),
                ));
                return Err(());
            }
        };

        if matches!(base_type, TypeSpec::Class(_)) && self.match_token(&TokenKind::Less) {
            let TypeSpec::Class(name) = base_type else {
                unreachable!()
            };
            let element = self.parse_type_spec()?;
            self.expect(&TokenKind::Greater, ">")?;
            base_type = TypeSpec::Generic(name, Box::new(element));
        }

        if self.match_token(&TokenKind::Question) {
            Ok(TypeSpec::Nullable(Box::new(base_type)))
        } else {
            Ok(base_type)
        }
    }

    fn parse_import_decl(&mut self) -> Result<ImportDecl, ()> {
        let start_span = self.current_token.span;
        self.expect(&TokenKind::Import, "import")?;

        let mut path = Vec::new();
        match self.current_token.kind.clone() {
            TokenKind::Identifier(part) => {
                self.advance();
                path.push(part);
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    "Expected package path identifier",
                    Some(self.current_token.span),
                ));
                return Err(());
            }
        }

        while self.match_token(&TokenKind::Dot) {
            match self.current_token.kind.clone() {
                TokenKind::Identifier(part) => {
                    self.advance();
                    path.push(part);
                }
                _ => break,
            }
        }

        let mut symbols = Vec::new();
        if self.match_token(&TokenKind::OpenBrace) {
            while !self.check(&TokenKind::CloseBrace) && !self.check(&TokenKind::Eof) {
                let name = match self.current_token.kind.clone() {
                    TokenKind::Identifier(n) => {
                        self.advance();
                        n
                    }
                    _ => break,
                };
                let alias = if self.match_token(&TokenKind::As) {
                    match self.current_token.kind.clone() {
                        TokenKind::Identifier(a) => {
                            self.advance();
                            Some(a)
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                symbols.push((name, alias));
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(&TokenKind::CloseBrace, "}")?;
        }

        let package_alias = if self.match_token(&TokenKind::As) {
            match self.current_token.kind.clone() {
                TokenKind::Identifier(a) => {
                    self.advance();
                    Some(a)
                }
                _ => None,
            }
        } else {
            None
        };

        Ok(ImportDecl {
            path,
            symbols,
            package_alias,
            span: start_span,
        })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ()> {
        match self.current_token.kind {
            TokenKind::Final | TokenKind::Var => self.parse_local_decl(),
            TokenKind::If => self.parse_if_stmt(),
            TokenKind::For => self.parse_for_stmt(),
            TokenKind::Break => self.parse_break_stmt(),
            TokenKind::Continue => self.parse_continue_stmt(),
            TokenKind::Return => self.parse_return_stmt(),
            TokenKind::Yield => self.parse_yield_stmt(),
            TokenKind::Try => self.parse_try_stmt(),
            TokenKind::Throw => self.parse_throw_stmt(),
            _ => self.parse_expr_or_typed_decl_stmt(),
        }
    }

    fn parse_exception_body(&mut self) -> Result<Vec<Stmt>, ()> {
        self.expect(&TokenKind::OpenBrace, "{")?;
        let mut body = Vec::new();
        while !self.check(&TokenKind::CloseBrace) && !self.check(&TokenKind::Eof) {
            body.push(self.parse_stmt()?);
        }
        self.expect(&TokenKind::CloseBrace, "}")?;
        Ok(body)
    }

    fn parse_try_stmt(&mut self) -> Result<Stmt, ()> {
        let start = self.current_token.span;
        self.expect(&TokenKind::Try, "try")?;
        let body = self.parse_exception_body()?;
        let mut catches = Vec::new();
        while self.match_token(&TokenKind::Catch) {
            let span = self.current_token.span;
            self.expect(&TokenKind::OpenParen, "(")?;
            let type_name = self.parse_exception_identifier()?;
            let variable = self.parse_exception_identifier()?;
            self.expect(&TokenKind::CloseParen, ")")?;
            let condition = if self.match_token(&TokenKind::If) {
                self.expect(&TokenKind::OpenParen, "(")?;
                let expr = self.parse_expr(Precedence::None)?;
                self.expect(&TokenKind::CloseParen, ")")?;
                Some(expr)
            } else { None };
            let handler_body = self.parse_exception_body()?;
            catches.push(CatchClause { type_name, variable, condition, body: handler_body, span });
        }
        let finally_body = if self.match_token(&TokenKind::Finally) {
            Some(self.parse_exception_body()?)
        } else { None };
        if catches.is_empty() && finally_body.is_none() {
            self.diagnostics.push(Diagnostic::error("try requires catch or finally", Some(start)));
            return Err(());
        }
        Ok(Stmt { kind: StmtKind::Try { body, catches, finally_body }, span: Span::combine(start, self.current_token.span) })
    }

    fn parse_exception_identifier(&mut self) -> Result<String, ()> {
        if let TokenKind::Identifier(name) = self.current_token.kind.clone() {
            self.advance();
            Ok(name)
        } else {
            self.diagnostics.push(Diagnostic::error("Expected a catch type and variable: catch (Error err)", Some(self.current_token.span)));
            Err(())
        }
    }

    fn parse_throw_stmt(&mut self) -> Result<Stmt, ()> {
        let start_span = self.current_token.span;
        self.expect(&TokenKind::Throw, "throw")?;
        let expr = if self.match_token(&TokenKind::OpenParen) {
            let inner = self.parse_expr(Precedence::None)?;
            self.expect(&TokenKind::CloseParen, ")")?;
            inner
        } else {
            self.parse_expr(Precedence::None)?
        };
        let end_span = self.current_token.span;

        Ok(Stmt {
            kind: StmtKind::Throw(expr),
            span: Span::combine(start_span, end_span),
        })
    }

    fn parse_break_stmt(&mut self) -> Result<Stmt, ()> {
        let start_span = self.current_token.span;
        self.expect(&TokenKind::Break, "break")?;

        Ok(Stmt {
            kind: StmtKind::Break,
            span: start_span,
        })
    }

    fn parse_continue_stmt(&mut self) -> Result<Stmt, ()> {
        let start_span = self.current_token.span;
        self.expect(&TokenKind::Continue, "continue")?;

        Ok(Stmt {
            kind: StmtKind::Continue,
            span: start_span,
        })
    }

    fn parse_local_decl(&mut self) -> Result<Stmt, ()> {
        let start_span = self.current_token.span;
        let is_mutable = self.match_token(&TokenKind::Var) || !self.match_token(&TokenKind::Final);

        let name = match self.current_token.kind.clone() {
            TokenKind::Identifier(n) => {
                self.advance();
                n
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    "Expected variable name",
                    Some(self.current_token.span),
                ));
                return Err(());
            }
        };

        let initializer = if self.match_token(&TokenKind::Equal) {
            Some(self.parse_expr(Precedence::None)?)
        } else {
            None
        };

        let end_span = self.current_token.span;

        Ok(Stmt {
            kind: StmtKind::LocalDecl {
                name,
                is_mutable,
                type_annotation: None,
                initializer,
            },
            span: Span::combine(start_span, end_span),
        })
    }

    fn is_type_specifier(&self, kind: &TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::IntType
                | TokenKind::DecimalType
                | TokenKind::ByteType
                | TokenKind::LongType
                | TokenKind::BoolType
                | TokenKind::StringType
                | TokenKind::Identifier(_)
        )
    }

    fn is_typed_decl_start(&self) -> bool {
        if self.is_type_specifier(&self.current_token.kind) {
            if matches!(self.peek_token.kind, TokenKind::Identifier(_)) {
                return true;
            }
            if matches!(self.peek_token.kind, TokenKind::Question | TokenKind::Less) {
                return true;
            }
        }
        false
    }

    fn parse_expr_or_typed_decl_stmt(&mut self) -> Result<Stmt, ()> {
        let start_span = self.current_token.span;

        if self.is_typed_decl_start() {
            let type_annotation = self.parse_type_spec()?;
            if let TokenKind::Identifier(name) = self.current_token.kind.clone() {
                self.advance();
                let initializer = if self.match_token(&TokenKind::Equal) {
                    Some(self.parse_expr(Precedence::None)?)
                } else {
                    None
                };
                let end_span = self.current_token.span;

                return Ok(Stmt {
                    kind: StmtKind::LocalDecl {
                        name,
                        is_mutable: false,
                        type_annotation: Some(type_annotation),
                        initializer,
                    },
                    span: Span::combine(start_span, end_span),
                });
            }
        }

        let expr = self.parse_expr(Precedence::None)?;
        let end_span = self.current_token.span;

        Ok(Stmt {
            kind: StmtKind::Expr(expr),
            span: Span::combine(start_span, end_span),
        })
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, ()> {
        let start_span = self.current_token.span;
        self.expect(&TokenKind::If, "if")?;
        self.expect(&TokenKind::OpenParen, "(")?;
        let condition = self.parse_expr(Precedence::None)?;
        self.expect(&TokenKind::CloseParen, ")")?;

        self.expect(&TokenKind::OpenBrace, "{")?;
        let mut then_branch = Vec::new();
        while !self.check(&TokenKind::CloseBrace) && !self.check(&TokenKind::Eof) {
            then_branch.push(self.parse_stmt()?);
        }
        let mut end_span = self.expect(&TokenKind::CloseBrace, "}")?.span;

        let else_branch = if self.match_token(&TokenKind::Else) {
            if self.check(&TokenKind::If) {
                let else_if = self.parse_if_stmt()?;
                end_span = else_if.span;
                Some(vec![else_if])
            } else {
                self.expect(&TokenKind::OpenBrace, "{")?;
                let mut else_stmts = Vec::new();
                while !self.check(&TokenKind::CloseBrace) && !self.check(&TokenKind::Eof) {
                    else_stmts.push(self.parse_stmt()?);
                }
                end_span = self.expect(&TokenKind::CloseBrace, "}")?.span;
                Some(else_stmts)
            }
        } else {
            None
        };

        Ok(Stmt {
            kind: StmtKind::If {
                condition,
                then_branch,
                else_branch,
            },
            span: Span::combine(start_span, end_span),
        })
    }

    fn parse_for_stmt(&mut self) -> Result<Stmt, ()> {
        let start_span = self.current_token.span;
        self.expect(&TokenKind::For, "for")?;

        let mut loop_var = None;
        let mut index_var = None;
        let mut condition_or_iterable = None;

        if self.match_token(&TokenKind::OpenParen) {
            // Check for iteration: (item in list) or (index, item in list)
            if let TokenKind::Identifier(var1) = &self.current_token.kind {
                let var1_name = var1.clone();
                if self.peek_token.kind == TokenKind::In {
                    self.advance(); // consume var1
                    self.advance(); // consume 'in'
                    loop_var = Some(var1_name);
                    condition_or_iterable = Some(self.parse_expr(Precedence::None)?);
                } else if self.peek_token.kind == TokenKind::Comma {
                    self.advance(); // consume var1
                    self.advance(); // consume ','
                    if let TokenKind::Identifier(var2) = &self.current_token.kind {
                        let var2_name = var2.clone();
                        self.advance(); // consume var2
                        self.expect(&TokenKind::In, "in")?;
                        index_var = Some(var1_name);
                        loop_var = Some(var2_name);
                        condition_or_iterable = Some(self.parse_expr(Precedence::None)?);
                    }
                } else {
                    condition_or_iterable = Some(self.parse_expr(Precedence::None)?);
                }
            } else {
                condition_or_iterable = Some(self.parse_expr(Precedence::None)?);
            }
            self.expect(&TokenKind::CloseParen, ")")?;
        }

        self.expect(&TokenKind::OpenBrace, "{")?;
        let mut body = Vec::new();
        while !self.check(&TokenKind::CloseBrace) && !self.check(&TokenKind::Eof) {
            body.push(self.parse_stmt()?);
        }
        let mut end_span = self.expect(&TokenKind::CloseBrace, "}")?.span;

        let else_branch = if self.match_token(&TokenKind::Else) {
            self.expect(&TokenKind::OpenBrace, "{")?;
            let mut else_stmts = Vec::new();
            while !self.check(&TokenKind::CloseBrace) && !self.check(&TokenKind::Eof) {
                else_stmts.push(self.parse_stmt()?);
            }
            end_span = self.expect(&TokenKind::CloseBrace, "}")?.span;
            Some(else_stmts)
        } else {
            None
        };

        Ok(Stmt {
            kind: StmtKind::For {
                loop_var,
                index_var,
                condition_or_iterable,
                body,
                else_branch,
            },
            span: Span::combine(start_span, end_span),
        })
    }

    fn parse_return_stmt(&mut self) -> Result<Stmt, ()> {
        let start_span = self.current_token.span;
        self.expect(&TokenKind::Return, "return")?;
        // A return value must start on the same line as `return`. This lets a
        // bare return precede another statement without a semicolon.
        let expr = if self.current_token.span.start.line == start_span.end.line
            && !self.check(&TokenKind::CloseBrace)
            && !self.check(&TokenKind::Eof)
        {
            Some(self.parse_expr(Precedence::None)?)
        } else {
            None
        };

        Ok(Stmt {
            kind: StmtKind::Return(expr),
            span: start_span,
        })
    }

    fn parse_yield_stmt(&mut self) -> Result<Stmt, ()> {
        let start_span = self.current_token.span;
        self.expect(&TokenKind::Yield, "yield")?;
        let expr = self.parse_expr(Precedence::None)?;

        Ok(Stmt {
            kind: StmtKind::Yield(expr),
            span: start_span,
        })
    }

    pub fn parse_expr(&mut self, prec: Precedence) -> Result<Expr, ()> {
        let mut left = self.parse_prefix()?;

        while prec < Precedence::from_token(&self.current_token.kind) {
            left = self.parse_infix(left)?;
        }

        Ok(left)
    }

    fn parse_prefix(&mut self) -> Result<Expr, ()> {
        let tok = self.current_token.clone();
        self.advance();

        match tok.kind {
            TokenKind::Print => {
                self.expect(&TokenKind::OpenParen, "(")?;
                let expr = self.parse_expr(Precedence::None)?;
                let end_tok = self.expect(&TokenKind::CloseParen, ")")?;
                Ok(Expr {
                    kind: ExprKind::Print {
                        expr: Box::new(expr),
                    },
                    span: Span::combine(tok.span, end_tok.span),
                })
            }
            TokenKind::IntLiteral(val) => Ok(Expr {
                kind: ExprKind::IntLiteral(val),
                span: tok.span,
            }),
            TokenKind::LongLiteral(val) => Ok(Expr {
                kind: ExprKind::LongLiteral(val),
                span: tok.span,
            }),
            TokenKind::DecimalLiteral(val) => Ok(Expr {
                kind: ExprKind::DecimalLiteral(val),
                span: tok.span,
            }),
            TokenKind::StringLiteral(val) => Ok(Expr {
                kind: ExprKind::StringLiteral(val),
                span: tok.span,
            }),
            TokenKind::True => Ok(Expr {
                kind: ExprKind::BoolLiteral(true),
                span: tok.span,
            }),
            TokenKind::False => Ok(Expr {
                kind: ExprKind::BoolLiteral(false),
                span: tok.span,
            }),
            TokenKind::Null => Ok(Expr {
                kind: ExprKind::NullLiteral,
                span: tok.span,
            }),
            TokenKind::This => Ok(Expr {
                kind: ExprKind::This,
                span: tok.span,
            }),
            TokenKind::Identifier(name) => {
                if name == "print" {
                    if self.check(&TokenKind::OpenParen) {
                        self.advance();
                        let expr = self.parse_expr(Precedence::None)?;
                        let end_tok = self.expect(&TokenKind::CloseParen, ")")?;
                        return Ok(Expr {
                            kind: ExprKind::Print {
                                expr: Box::new(expr),
                            },
                            span: Span::combine(tok.span, end_tok.span),
                        });
                    }
                }
                if self.check(&TokenKind::Arrow) {
                    self.advance();
                    let body_expr = self.parse_expr(Precedence::None)?;
                    return Ok(Expr {
                        kind: ExprKind::Lambda {
                            parameter: name,
                            body: vec![Stmt {
                                span: body_expr.span,
                                kind: StmtKind::Expr(body_expr.clone()),
                            }],
                        },
                        span: Span::combine(tok.span, body_expr.span),
                    });
                }
                if let Some(class_name) = canonical_builtin_class_name(&name) {
                    let generic_type = if self.match_token(&TokenKind::Less) {
                        let ty = self.parse_type_spec()?;
                        self.expect(&TokenKind::Greater, ">")?;
                        Some(ty)
                    } else {
                        None
                    };
                    if self.check(&TokenKind::OpenParen) {
                        return self.parse_class_instantiation(
                            tok.span,
                            class_name.to_string(),
                            generic_type,
                        );
                    }
                }
                if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                    let generic_type = if self.match_token(&TokenKind::Less) {
                        let ty = self.parse_type_spec()?;
                        self.expect(&TokenKind::Greater, ">")?;
                        Some(ty)
                    } else {
                        None
                    };
                    if self.check(&TokenKind::OpenParen) {
                        return self.parse_class_instantiation(tok.span, name, generic_type);
                    } else {
                        return Ok(Expr {
                            kind: ExprKind::Identifier(name),
                            span: tok.span,
                        });
                    }
                } else {
                    return Ok(Expr {
                        kind: ExprKind::Identifier(name),
                        span: tok.span,
                    });
                }
            }
            TokenKind::Minus => {
                let operand = self.parse_expr(Precedence::Unary)?;
                let span = Span::combine(tok.span, operand.span);
                Ok(Expr {
                    kind: ExprKind::Unary {
                        op: UnaryOp::Neg,
                        operand: Box::new(operand),
                    },
                    span,
                })
            }
            TokenKind::Not => {
                let operand = self.parse_expr(Precedence::Unary)?;
                let span = Span::combine(tok.span, operand.span);
                Ok(Expr {
                    kind: ExprKind::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(operand),
                    },
                    span,
                })
            }
            TokenKind::OpenParen => {
                let expr = self.parse_expr(Precedence::None)?;
                self.expect(&TokenKind::CloseParen, ")")?;
                Ok(expr)
            }
            TokenKind::OpenBrace => {
                let mut fields = Vec::new();
                if !self.check(&TokenKind::CloseBrace) {
                    loop {
                        let field_name = match self.current_token.kind.clone() {
                            TokenKind::Identifier(f) => {
                                self.advance();
                                f
                            }
                            _ => break,
                        };
                        self.expect(&TokenKind::Colon, ":")?;
                        let val_expr = self.parse_expr(Precedence::None)?;
                        fields.push((field_name, val_expr));
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                let end_tok = self.expect(&TokenKind::CloseBrace, "}")?;
                Ok(Expr {
                    kind: ExprKind::AnonymousObject { fields },
                    span: Span::combine(tok.span, end_tok.span),
                })
            }
            TokenKind::OpenBracket => {
                if self.check(&TokenKind::CloseBracket) {
                    let end_tok = self.expect(&TokenKind::CloseBracket, "]")?;
                    return Ok(Expr {
                        kind: ExprKind::ListLiteral(vec![]),
                        span: Span::combine(tok.span, end_tok.span),
                    });
                }
                let first_expr = self.parse_expr(Precedence::None)?;
                if self.check(&TokenKind::Colon) {
                    self.advance();
                    let first_val = self.parse_expr(Precedence::None)?;
                    let mut pairs = vec![(first_expr, first_val)];
                    while self.match_token(&TokenKind::Comma) {
                        if self.check(&TokenKind::CloseBracket) {
                            break;
                        }
                        let k = self.parse_expr(Precedence::None)?;
                        self.expect(&TokenKind::Colon, ":")?;
                        let v = self.parse_expr(Precedence::None)?;
                        pairs.push((k, v));
                    }
                    let end_tok = self.expect(&TokenKind::CloseBracket, "]")?;
                    Ok(Expr {
                        kind: ExprKind::MapLiteral(pairs),
                        span: Span::combine(tok.span, end_tok.span),
                    })
                } else {
                    let mut items = vec![first_expr];
                    while self.match_token(&TokenKind::Comma) {
                        if self.check(&TokenKind::CloseBracket) {
                            break;
                        }
                        items.push(self.parse_expr(Precedence::None)?);
                    }
                    let end_tok = self.expect(&TokenKind::CloseBracket, "]")?;
                    Ok(Expr {
                        kind: ExprKind::ListLiteral(items),
                        span: Span::combine(tok.span, end_tok.span),
                    })
                }
            }
            TokenKind::Async => {
                let inner = self.parse_expr(Precedence::Unary)?;
                let span = Span::combine(tok.span, inner.span);
                Ok(Expr {
                    kind: ExprKind::AsyncSpawn(Box::new(inner)),
                    span,
                })
            }
            TokenKind::Await => {
                let inner = self.parse_expr(Precedence::Unary)?;
                let span = Span::combine(tok.span, inner.span);
                Ok(Expr {
                    kind: ExprKind::Await(Box::new(inner)),
                    span,
                })
            }
            TokenKind::If => {
                let start_span = tok.span;
                self.expect(&TokenKind::OpenParen, "(")?;
                let condition = self.parse_expr(Precedence::None)?;
                self.expect(&TokenKind::CloseParen, ")")?;

                self.expect(&TokenKind::OpenBrace, "{")?;
                let mut then_branch = Vec::new();
                while !self.check(&TokenKind::CloseBrace) && !self.check(&TokenKind::Eof) {
                    then_branch.push(self.parse_stmt()?);
                }
                let mut end_span = self.expect(&TokenKind::CloseBrace, "}")?.span;

                let else_branch = if self.match_token(&TokenKind::Else) {
                    if self.check(&TokenKind::If) {
                        let else_if = self.parse_if_stmt()?;
                        end_span = else_if.span;
                        Some(vec![else_if])
                    } else {
                        self.expect(&TokenKind::OpenBrace, "{")?;
                        let mut else_stmts = Vec::new();
                        while !self.check(&TokenKind::CloseBrace) && !self.check(&TokenKind::Eof) {
                            else_stmts.push(self.parse_stmt()?);
                        }
                        end_span = self.expect(&TokenKind::CloseBrace, "}")?.span;
                        Some(else_stmts)
                    }
                } else {
                    None
                };

                Ok(Expr {
                    kind: ExprKind::If {
                        condition: Box::new(condition),
                        then_branch,
                        else_branch,
                    },
                    span: Span::combine(start_span, end_span),
                })
            }
            TokenKind::For => {
                let start_span = tok.span;
                let mut loop_var = None;
                let mut index_var = None;
                let mut condition_or_iterable = None;

                if self.match_token(&TokenKind::OpenParen) {
                    if let TokenKind::Identifier(var1) = &self.current_token.kind {
                        let var1_name = var1.clone();
                        if self.peek_token.kind == TokenKind::In {
                            self.advance();
                            self.advance();
                            loop_var = Some(var1_name);
                            condition_or_iterable = Some(self.parse_expr(Precedence::None)?);
                        } else if self.peek_token.kind == TokenKind::Comma {
                            self.advance();
                            self.advance();
                            if let TokenKind::Identifier(var2) = &self.current_token.kind {
                                let var2_name = var2.clone();
                                self.advance();
                                self.expect(&TokenKind::In, "in")?;
                                index_var = Some(var1_name);
                                loop_var = Some(var2_name);
                                condition_or_iterable = Some(self.parse_expr(Precedence::None)?);
                            }
                        } else {
                            condition_or_iterable = Some(self.parse_expr(Precedence::None)?);
                        }
                    } else {
                        condition_or_iterable = Some(self.parse_expr(Precedence::None)?);
                    }
                    self.expect(&TokenKind::CloseParen, ")")?;
                }

                self.expect(&TokenKind::OpenBrace, "{")?;
                let mut body = Vec::new();
                while !self.check(&TokenKind::CloseBrace) && !self.check(&TokenKind::Eof) {
                    body.push(self.parse_stmt()?);
                }
                let mut end_span = self.expect(&TokenKind::CloseBrace, "}")?.span;

                let else_branch = if self.match_token(&TokenKind::Else) {
                    self.expect(&TokenKind::OpenBrace, "{")?;
                    let mut else_stmts = Vec::new();
                    while !self.check(&TokenKind::CloseBrace) && !self.check(&TokenKind::Eof) {
                        else_stmts.push(self.parse_stmt()?);
                    }
                    end_span = self.expect(&TokenKind::CloseBrace, "}")?.span;
                    Some(else_stmts)
                } else {
                    None
                };

                Ok(Expr {
                    kind: ExprKind::For {
                        loop_var,
                        index_var,
                        condition_or_iterable: condition_or_iterable.map(Box::new),
                        body,
                        else_branch,
                    },
                    span: Span::combine(start_span, end_span),
                })
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    format!("Unexpected expression token {:?}", tok.kind),
                    Some(tok.span),
                ));
                Err(())
            }
        }
    }

    fn parse_class_instantiation(
        &mut self,
        start: Span,
        class_name: String,
        generic_type: Option<TypeSpec>,
    ) -> Result<Expr, ()> {
        self.expect(&TokenKind::OpenParen, "(")?;
        let mut args = Vec::new();
        if !self.check(&TokenKind::CloseParen) {
            loop {
                args.push(self.parse_expr(Precedence::None)?);
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }
        let end = self.expect(&TokenKind::CloseParen, ")")?.span;
        Ok(Expr {
            kind: ExprKind::ClassInstantiation {
                class_name,
                generic_type,
                args,
            },
            span: Span::combine(start, end),
        })
    }

    fn parse_infix(&mut self, left: Expr) -> Result<Expr, ()> {
        let tok = self.current_token.clone();
        let prec = Precedence::from_token(&tok.kind);

        match tok.kind {
            TokenKind::Equal => {
                self.advance();
                let value = self.parse_expr(Precedence::Assignment)?;
                let span = Span::combine(left.span, value.span);
                Ok(Expr {
                    kind: ExprKind::Assign {
                        target: Box::new(left),
                        value: Box::new(value),
                    },
                    span,
                })
            }
            TokenKind::QuestionQuestionEqual => {
                self.advance();
                let value = self.parse_expr(Precedence::Assignment)?;
                let span = Span::combine(left.span, value.span);
                Ok(Expr {
                    kind: ExprKind::NullCoalescingAssign {
                        target: Box::new(left),
                        value: Box::new(value),
                    },
                    span,
                })
            }
            TokenKind::Dot | TokenKind::QuestionDot => {
                let is_optional = tok.kind == TokenKind::QuestionDot;
                self.advance();
                let member_name = match self.current_token.kind.clone() {
                    TokenKind::Identifier(m) => {
                        self.advance();
                        m
                    }
                    TokenKind::In => {
                        self.advance();
                        "in".to_string()
                    }
                    TokenKind::Async => {
                        self.advance();
                        "async".to_string()
                    }
                    TokenKind::IntType => {
                        self.advance();
                        "int".to_string()
                    }
                    TokenKind::LongType => {
                        self.advance();
                        "long".to_string()
                    }
                    TokenKind::DecimalType => {
                        self.advance();
                        "decimal".to_string()
                    }
                    TokenKind::StringType => {
                        self.advance();
                        "string".to_string()
                    }
                    TokenKind::BoolType => {
                        self.advance();
                        "bool".to_string()
                    }
                    TokenKind::ByteType => {
                        self.advance();
                        "byte".to_string()
                    }
                    _ => {
                        self.diagnostics.push(Diagnostic::error(
                            "Expected member name after '.'",
                            Some(self.current_token.span),
                        ));
                        return Err(());
                    }
                };

                let has_response_handler = matches!(
                    member_name.as_str(),
                    "get" | "post" | "put" | "patch" | "delete" | "options" | "head"
                );
                let member_expr = Expr {
                    kind: ExprKind::MemberAccess {
                        object: Box::new(left.clone()),
                        member: member_name,
                        is_optional,
                    },
                    span: Span::combine(left.span, self.current_token.span),
                };

                if self.check(&TokenKind::OpenParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if !self.check(&TokenKind::CloseParen) {
                        loop {
                            args.push(self.parse_expr(Precedence::None)?);
                            if !self.match_token(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    let end_tok = self.expect(&TokenKind::CloseParen, ")")?;
                    let mut end_span = end_tok.span;
                    if self.check(&TokenKind::OpenBrace) {
                        if has_response_handler {
                            let lambda = self.parse_lambda_block()?;
                            end_span = lambda.span;
                            args.push(lambda);
                        } else {
                            end_span = self.skip_trailing_block()?;
                            args.push(Expr {
                                kind: ExprKind::IntLiteral(0),
                                span: end_span,
                            });
                        }
                    }
                    Ok(Expr {
                        kind: ExprKind::Call {
                            callee: Box::new(member_expr),
                            args,
                        },
                        span: Span::combine(left.span, end_span),
                    })
                } else if self.check(&TokenKind::OpenBrace) {
                    let (handler, end_span) = if has_response_handler {
                        let lambda = self.parse_lambda_block()?;
                        let span = lambda.span;
                        (lambda, span)
                    } else {
                        let span = self.skip_trailing_block()?;
                        (
                            Expr {
                                kind: ExprKind::IntLiteral(0),
                                span,
                            },
                            span,
                        )
                    };
                    Ok(Expr {
                        kind: ExprKind::Call {
                            callee: Box::new(member_expr),
                            args: vec![handler],
                        },
                        span: Span::combine(left.span, end_span),
                    })
                } else {
                    Ok(member_expr)
                }
            }

            TokenKind::With => {
                self.advance();
                self.expect(&TokenKind::OpenBrace, "{")?;
                let mut updates = Vec::new();
                if !self.check(&TokenKind::CloseBrace) {
                    loop {
                        let field_name = match self.current_token.kind.clone() {
                            TokenKind::Identifier(f) => {
                                self.advance();
                                f
                            }
                            _ => break,
                        };
                        self.expect(&TokenKind::Colon, ":")?;
                        let val_expr = self.parse_expr(Precedence::None)?;
                        updates.push((field_name, val_expr));
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                let end_tok = self.expect(&TokenKind::CloseBrace, "}")?;
                Ok(Expr {
                    kind: ExprKind::WithUpdate {
                        base: Box::new(left.clone()),
                        updates,
                    },
                    span: Span::combine(left.span, end_tok.span),
                })
            }
            TokenKind::Is => {
                self.advance();
                let is_negated = self.match_token(&TokenKind::Not);
                let target_type = self.parse_type_spec()?;
                Ok(Expr {
                    kind: ExprKind::IsCheck {
                        expr: Box::new(left.clone()),
                        target_type,
                        is_negated,
                    },
                    span: Span::combine(left.span, self.current_token.span),
                })
            }
            TokenKind::OpenBracket => {
                self.advance();
                let index_expr = self.parse_expr(Precedence::None)?;
                let end_tok = self.expect(&TokenKind::CloseBracket, "]")?;
                Ok(Expr {
                    kind: ExprKind::IndexAccess {
                        target: Box::new(left.clone()),
                        index: Box::new(index_expr),
                    },
                    span: Span::combine(left.span, end_tok.span),
                })
            }
            _ => {
                self.advance();
                let right = self.parse_expr(prec)?;
                let bin_op = match tok.kind {
                    TokenKind::Plus => BinaryOp::Add,
                    TokenKind::Minus => BinaryOp::Sub,
                    TokenKind::Star => BinaryOp::Mul,
                    TokenKind::Slash => BinaryOp::Div,
                    TokenKind::Percent => BinaryOp::Mod,
                    TokenKind::EqualEqual => BinaryOp::Equal,
                    TokenKind::BangEqual => BinaryOp::NotEqual,
                    TokenKind::Less => BinaryOp::Less,
                    TokenKind::LessEqual => BinaryOp::LessEqual,
                    TokenKind::Greater => BinaryOp::Greater,
                    TokenKind::GreaterEqual => BinaryOp::GreaterEqual,
                    TokenKind::And => BinaryOp::And,
                    TokenKind::Nand => BinaryOp::Nand,
                    TokenKind::Or => BinaryOp::Or,
                    TokenKind::Nor => BinaryOp::Nor,
                    TokenKind::Xor => BinaryOp::Xor,
                    TokenKind::Xnor => BinaryOp::Xnor,
                    TokenKind::QuestionQuestion => BinaryOp::NullCoalescing,
                    TokenKind::DotDot => BinaryOp::Range,
                    _ => {
                        self.diagnostics.push(Diagnostic::error(
                            format!("Unexpected binary operator {:?}", tok.kind),
                            Some(tok.span),
                        ));
                        return Err(());
                    }
                };
                let span = Span::combine(left.span, right.span);
                Ok(Expr {
                    kind: ExprKind::Binary {
                        op: bin_op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    span,
                })
            }
        }
    }
}
