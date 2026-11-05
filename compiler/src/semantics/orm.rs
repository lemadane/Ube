impl TypeChecker {
    fn orm_column_type(
        &self,
        model: &str,
        joins: &[(String, String)],
        column: &str,
    ) -> Result<Type, String> {
        let (model, column) = if let Some((alias, column)) = column.split_once('.') {
            if alias == "base" {
                (model, column)
            } else {
                (
                    joins
                        .iter()
                        .find(|(a, _)| a.eq_ignore_ascii_case(alias))
                        .map(|(_, m)| m.as_str())
                        .ok_or_else(|| format!("Unknown ORM join alias '{alias}'"))?,
                    column,
                )
            }
        } else {
            (model, column)
        };
        let declaration = self
            .orm_declarations
            .get(model)
            .ok_or_else(|| format!("Unknown ORM model '{model}'"))?;
        crate::orm::fields(declaration)?
            .into_iter()
            .find(|(name, _)| name == column)
            .map(|(_, ty)| ty)
            .ok_or_else(|| format!("Unknown ORM column '{column}' on {model}"))
    }
    fn check_orm_call(
        &mut self,
        receiver: &Type,
        object: &Expr,
        member: &str,
        args: &[Expr],
        actual: &[Type],
        span: crate::diagnostics::Span,
    ) -> Option<Option<Type>> {
        use crate::orm::{handle, Kind};
        let error = |this: &mut Self, message: String| {
            this.diagnostics
                .push(Diagnostic::error(message, Some(span)));
            Some(None)
        };
        if matches!(receiver, Type::Class { name } if name == "Orm") {
            let static_call = matches!(&object.kind, ExprKind::Identifier(n) if n == "Orm" && self.symbol_table.lookup(n).is_none());
            if !static_call || member != "model" || args.len() != 3 || actual.len() != 3 {
                return error(
                    self,
                    "Use Orm.model(db, ModelClass, table) with exactly three arguments".into(),
                );
            }
            if actual[0] != (Type::Class { name: "SQL".into() }) || actual[2] != Type::String {
                return error(
                    self,
                    "Orm.model requires a SQL connection and a non-null table string".into(),
                );
            }
            let ExprKind::Identifier(model) = &args[1].kind else {
                return error(self, "ORM model must be a class name".into());
            };
            if self.symbol_table.lookup(model).is_some() {
                return error(
                    self,
                    "ORM model must be a class name, not an instance".into(),
                );
            }
            let Some(declaration) = self.orm_declarations.get(model) else {
                return error(self, "ORM model must be a user-defined class".into());
            };
            if let Err(message) = crate::orm::fields(declaration) {
                return error(self, message);
            }
            return Some(Some(handle(model, Kind::Model)));
        }
        let Type::Orm { model, kind, joins } = receiver else {
            return None;
        };
        let Some(declaration) = self.orm_declarations.get(model) else {
            return error(self, format!("Unknown ORM model '{model}'"));
        };
        let fields = match crate::orm::fields(declaration) {
            Ok(fields) => fields,
            Err(e) => return error(self, e),
        };
        let entity = Type::Class {
            name: model.clone(),
        };
        let id = fields
            .iter()
            .find(|(name, _)| name == "id")
            .unwrap()
            .1
            .clone();
        let nullable = Type::Nullable(Box::new(entity.clone()));
        let (expected, result) = match (kind, member) {
            (Kind::Model, "create" | "save") => (vec![entity.clone()], Type::Long),
            (Kind::Model, "find") => (vec![id.clone()], nullable.clone()),
            (Kind::Model, "get") => (vec![id.clone()], entity.clone()),
            (Kind::Model, "delete") => (vec![id], Type::Long),
            (Kind::Model, "migrate" | "close") => (vec![], Type::Void),
            (Kind::Model, "query") => (vec![], handle(model, Kind::Query)),
            (Kind::Model, "using") => (
                vec![Type::Class {
                    name: "SQLTransaction".into(),
                }],
                receiver.clone(),
            ),
            (Kind::Query, "join" | "leftJoin") => {
                if args.len() != 4 || actual.len() != 4 {
                    return error(self,"ORM join expects a model repository, literal alias, source column, and target column".into());
                }
                let Type::Orm {
                    model: target,
                    kind: Kind::Model,
                    ..
                } = &actual[0]
                else {
                    return error(self, "ORM join requires an OrmModel repository".into());
                };
                let (ExprKind::StringLiteral(alias) | ExprKind::TextBlock(alias)) = &args[1].kind
                else {
                    return error(self, "ORM join alias must be a string literal".into());
                };
                if !crate::orm::alias_valid(alias)
                    || joins.iter().any(|(a, _)| a.eq_ignore_ascii_case(alias))
                {
                    return error(self,"ORM join alias must be unique, use 1..63 ASCII identifier characters, and cannot be base".into());
                }
                if joins.len() >= 16 {
                    return error(self, "ORM queries support at most 16 joins".into());
                }
                let source =
                    if let ExprKind::StringLiteral(c) | ExprKind::TextBlock(c) = &args[2].kind {
                        match self.orm_column_type(model, joins, c) {
                            Ok(t) => Some(t),
                            Err(e) => return error(self, e),
                        }
                    } else {
                        None
                    };
                let target_ty =
                    if let ExprKind::StringLiteral(c) | ExprKind::TextBlock(c) = &args[3].kind {
                        match self.orm_column_type(target, &[], c) {
                            Ok(t) => Some(t),
                            Err(e) => return error(self, e),
                        }
                    } else {
                        None
                    };
                if let (Some(a), Some(b)) = (source, target_ty) {
                    let a = if let Type::Nullable(i) = a { *i } else { a };
                    let b = if let Type::Nullable(i) = b { *i } else { b };
                    if a != b
                        && !(matches!(a, Type::Int | Type::Long)
                            && matches!(b, Type::Int | Type::Long))
                    {
                        return error(self, "ORM join columns must have compatible types".into());
                    }
                }
                (
                    vec![actual[0].clone(), Type::String, Type::String, Type::String],
                    crate::orm::joined(receiver, target, alias),
                )
            }
            (Kind::Query, "where") => {
                if args.len() != 3 || actual.len() != 3 {
                    return error(
                        self,
                        "ORM where expects a column, an operator, and a non-null scalar value"
                            .into(),
                    );
                }
                if !matches!(
                    &actual[2],
                    Type::Int | Type::Long | Type::Decimal | Type::Bool | Type::String
                ) {
                    return error(
                        self,
                        "ORM where requires a non-null scalar value; use whereNull for SQL NULL"
                            .into(),
                    );
                }
                if let ExprKind::StringLiteral(column) | ExprKind::TextBlock(column) = &args[0].kind
                {
                    let field_ty = match self.orm_column_type(model, joins, column) {
                        Ok(ty) => ty,
                        Err(e) => return error(self, e),
                    };
                    let field_ty = match &field_ty {
                        Type::Nullable(inner) => inner.as_ref(),
                        ty => ty,
                    };
                    if &actual[2] != field_ty
                        && !(actual[2] == Type::Int && field_ty == &Type::Long)
                    {
                        return error(
                            self,
                            format!(
                                "ORM column '{column}' expects {field_ty}, got {}",
                                actual[2]
                            ),
                        );
                    }
                }
                (
                    vec![Type::String, Type::String, actual[2].clone()],
                    receiver.clone(),
                )
            }
            (Kind::Query, "whereNull" | "whereNotNull") => (vec![Type::String], receiver.clone()),
            (Kind::Query, "orderBy") => (vec![Type::String, Type::String], receiver.clone()),
            (Kind::Query, "limit" | "offset") => (vec![Type::Int], receiver.clone()),
            (Kind::Query, "all") => (vec![], handle(model, Kind::Rows)),
            (Kind::Query, "first") => (vec![], nullable),
            (Kind::Query, "count") => (vec![], Type::Long),
            (Kind::Query, "close") => (vec![], Type::Void),
            (Kind::Rows, "next") => (vec![], Type::Bool),
            (Kind::Rows, "get") => (vec![], entity),
            (Kind::Rows, "rowCount") => (vec![], Type::Long),
            (Kind::Rows, "close" | "reset") => (vec![], Type::Void),
            _ => return error(self, format!("Unknown {} method '{member}'", kind.class())),
        };
        if args.len() != expected.len() || actual.len() != expected.len() {
            return error(
                self,
                format!(
                    "{}.{member} expects {} arguments",
                    kind.class(),
                    expected.len()
                ),
            );
        }
        if !actual
            .iter()
            .zip(&expected)
            .all(|(a, e)| a == e || a == &Type::Int && e == &Type::Long)
        {
            return error(
                self,
                format!("Invalid ORM argument types for {}.{member}", kind.class()),
            );
        }
        if *kind == Kind::Query && matches!(member, "whereNull" | "whereNotNull" | "orderBy") {
            if let Some(Expr {
                kind: ExprKind::StringLiteral(column) | ExprKind::TextBlock(column),
                ..
            }) = args.first()
            {
                if let Err(e) = self.orm_column_type(model, joins, column) {
                    return error(self, e);
                }
            }
        }
        Some(Some(result))
    }
}
