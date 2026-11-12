//! Concrete, compiler-generated implementations keep mocks on the ordinary trait ABI.
use crate::parser::ast::*;
use std::collections::HashSet;

fn expressions(expr: &Expr, requested: &mut HashSet<String>) {
    let mut visit = |expr: &Expr| expressions(expr, requested);
    match &expr.kind {
        ExprKind::Closure { expression, .. } => visit(expression),
        ExprKind::Call { callee, args } => {
            if matches!(&callee.kind, ExprKind::MemberAccess { object, member, .. } if member == "mock" && matches!(&object.kind, ExprKind::Identifier(name) if name == "Mockery"))
            {
                if let [Expr {
                    kind: ExprKind::Identifier(name),
                    ..
                }] = args.as_slice()
                {
                    requested.insert(name.clone());
                }
            }
            expressions(callee, requested);
            for arg in args {
                expressions(arg, requested);
            }
        }
        ExprKind::Binary { left, right, .. } => {
            visit(left);
            visit(right);
        }
        ExprKind::Assign { target, value } | ExprKind::NullCoalescingAssign { target, value } => {
            visit(target);
            visit(value);
        }
        ExprKind::Unary { operand, .. } => visit(operand),
        ExprKind::MemberAccess { object, .. } => visit(object),
        ExprKind::ClassInstantiation { args, .. } | ExprKind::ListLiteral(args) => {
            for arg in args {
                visit(arg);
            }
        }
        ExprKind::AnonymousObject { fields } => {
            for (_, value) in fields {
                visit(value);
            }
        }
        ExprKind::WithUpdate { base, updates } => {
            visit(base);
            for (_, value) in updates {
                visit(value);
            }
        }
        ExprKind::IsCheck { expr, .. } | ExprKind::AsyncSpawn(expr) | ExprKind::Await(expr) => {
            visit(expr)
        }
        ExprKind::SwitchExpr {
            target,
            cases,
            default_case,
        } => {
            expressions(target, requested);
            for case in cases {
                expressions(&case.pattern, requested);
                if let Some(guard) = &case.guard {
                    expressions(guard, requested);
                }
                statements(&case.body, requested);
                if let Some(result) = &case.result_expr {
                    expressions(result, requested);
                }
            }
            if let Some(default) = default_case {
                expressions(default, requested);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (key, value) in entries {
                visit(key);
                visit(value);
            }
        }
        ExprKind::IndexAccess { target, index } => {
            visit(target);
            visit(index);
        }
        ExprKind::Lambda { body, .. } | ExprKind::BlockClosure { body, .. } => statements(body, requested),
        ExprKind::Conditional { condition, when_true, when_false, .. } => {
            visit(condition);
            visit(when_true);
            visit(when_false);
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expressions(condition, requested);
            statements(then_branch, requested);
            if let Some(body) = else_branch {
                statements(body, requested);
            }
        }
        ExprKind::For {
            condition_or_iterable,
            body,
            else_branch,
            ..
        } => {
            if let Some(condition) = condition_or_iterable {
                expressions(condition, requested);
            }
            statements(body, requested);
            if let Some(body) = else_branch {
                statements(body, requested);
            }
        }
        ExprKind::TemplateLiteral(parts) => {
            for part in parts { visit(part); }
        }
        ExprKind::IntLiteral(_)
        | ExprKind::LongLiteral(_)
        | ExprKind::DecimalLiteral(_)
        | ExprKind::StringLiteral(_)
        | ExprKind::TextBlock(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::NullLiteral
        | ExprKind::Identifier(_)
        | ExprKind::This => {}
    }
}
fn statements(body: &[Stmt], requested: &mut HashSet<String>) {
    for statement in body {
        match &statement.kind {
            StmtKind::LocalDecl { initializer, .. } | StmtKind::Return(initializer) => {
                if let Some(expr) = initializer {
                    expressions(expr, requested);
                }
            }
            StmtKind::Expr(expr) | StmtKind::Throw(expr) | StmtKind::Yield(expr) => {
                expressions(expr, requested)
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                expressions(condition, requested);
                statements(then_branch, requested);
                if let Some(body) = else_branch {
                    statements(body, requested);
                }
            }
            StmtKind::For {
                condition_or_iterable,
                body,
                else_branch,
                ..
            } => {
                if let Some(condition) = condition_or_iterable {
                    expressions(condition, requested);
                }
                statements(body, requested);
                if let Some(body) = else_branch {
                    statements(body, requested);
                }
            }
            StmtKind::Try {
                body,
                catches,
                finally_body,
            } => {
                statements(body, requested);
                for catch in catches {
                    if let Some(condition) = &catch.condition {
                        expressions(condition, requested);
                    }
                    statements(&catch.body, requested);
                }
                if let Some(body) = finally_body {
                    statements(body, requested);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
        }
    }
}

pub const PREFIX: &str = "UbeMockery_";
pub fn class_name(contract: &str) -> String {
    format!("{PREFIX}{contract}")
}
pub fn kind(spec: Option<&TypeSpec>) -> Option<&'static str> {
    Some(match spec {
        None => "void",
        Some(TypeSpec::Int) => "int",
        Some(TypeSpec::Long) => "long",
        Some(TypeSpec::Byte) => "byte",
        Some(TypeSpec::Bool) => "bool",
        Some(TypeSpec::Decimal) => "decimal",
        Some(TypeSpec::String) => "string",
        _ => return None,
    })
}

pub fn expand(program: &Program) -> Program {
    let mut result = program.clone();
    let mut requested = HashSet::new();
    for item in &program.items {
        match item {
            Item::Class(class) => {
                for parameter in &class.primary_params {
                    if let Some(value) = &parameter.default_value {
                        expressions(value, &mut requested);
                    }
                }
                for field in &class.static_fields {
                    if let Some(value) = &field.initializer {
                        expressions(value, &mut requested);
                    }
                }
                for method in &class.methods {
                    statements(&method.body, &mut requested);
                }
            }
            Item::Trait(contract) => {
                for method in &contract.methods {
                    statements(&method.body, &mut requested);
                }
            }
            Item::Import(_) => {}
        }
    }
    for item in &program.items {
        let Item::Trait(contract) = item else {
            continue;
        };
        if !requested.contains(&contract.name) {
            continue;
        }
        if contract.methods.iter().any(|m| {
            m.is_async
                || kind(m.return_type.as_ref()).is_none()
                || matches!(m.name.as_str(), "mockeryId" | "_mockeryId")
        }) {
            continue;
        }
        let name = class_name(&contract.name);
        if result
            .items
            .iter()
            .any(|i| matches!(i, Item::Class(c) if c.name == name))
        {
            continue;
        }
        let span = contract.span;
        let methods = contract
            .methods
            .iter()
            .map(|method| {
                let call = Expr {
                    span,
                    kind: ExprKind::Call {
                        callee: Box::new(Expr {
                            span,
                            kind: ExprKind::MemberAccess {
                                object: Box::new(Expr {
                                    span,
                                    kind: ExprKind::Identifier("Mockery".into()),
                                }),
                                member: format!(
                                    "_call_{}",
                                    kind(method.return_type.as_ref()).unwrap()
                                ),
                                is_optional: false,
                            },
                        }),
                        args: vec![
                            Expr {
                                span,
                                kind: ExprKind::MemberAccess {
                                    object: Box::new(Expr {
                                        span,
                                        kind: ExprKind::This,
                                    }),
                                    member: "_mockeryId".into(),
                                    is_optional: false,
                                },
                            },
                            Expr {
                                span,
                                kind: ExprKind::StringLiteral(method.name.clone()),
                            },
                        ],
                    },
                };
                let mut generated = method.clone();
                generated.body = vec![Stmt {
                    span,
                    kind: if method.return_type.is_some() {
                        StmtKind::Return(Some(call))
                    } else {
                        StmtKind::Expr(call)
                    },
                }];
                generated
            })
            .collect();
        result.items.push(Item::Class(ClassDecl {
            static_fields: vec![],
            name,
            span,
            cases: vec![],
            methods,
            primary_params: vec![ConstructorParam {
                name: "mockeryId".into(),
                type_spec: TypeSpec::Long,
                is_var: false,
                default_value: None,
                span,
            }],
        }));
    }
    result
}
