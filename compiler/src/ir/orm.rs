impl IrBuilder {
    // Determine the static receiver type without evaluating fluent calls twice.
    fn orm_expr_type(&self, expr: &Expr) -> Option<Type> {
        use crate::orm::{handle, Kind};
        match &expr.kind {
            ExprKind::Identifier(name) => self.lookup_var(name).map(|(_, ty)| ty),
            ExprKind::This => self.lookup_var("this").map(|(_, ty)| ty),
            ExprKind::MemberAccess { object, member, .. } => {
                if let Type::Class { name } = self.orm_expr_type(object)? {
                    self.class_fields.get(&(name, member.clone())).cloned()
                } else {
                    None
                }
            }
            ExprKind::Call { callee, args } => {
                let ExprKind::MemberAccess { object, member, .. } = &callee.kind else {
                    return None;
                };
                if matches!(&object.kind, ExprKind::Identifier(n) if n == "Orm" && self.lookup_var(n).is_none())
                    && member == "model"
                {
                    if let Some(Expr {
                        kind: ExprKind::Identifier(model),
                        ..
                    }) = args.get(1)
                    {
                        return Some(handle(model, Kind::Model));
                    }
                }
                match self.orm_expr_type(object)? {
                    Type::Orm {
                        model,
                        kind: Kind::Model,
                        ..
                    } if member == "query" => Some(handle(&model, Kind::Query)),
                    Type::Orm {
                        model,
                        kind: Kind::Model,
                        ..
                    } if member == "using" => Some(handle(&model, Kind::Model)),
                    ty @ Type::Orm {
                        kind: Kind::Query | Kind::Projection,
                        ..
                    } if member == "join" || member == "leftJoin" => {
                        let Type::Orm {
                            model: target,
                            kind: Kind::Model,
                            ..
                        } = self.orm_expr_type(&args[0])?
                        else {
                            return None;
                        };
                        let (ExprKind::StringLiteral(alias) | ExprKind::TextBlock(alias)) =
                            &args[1].kind
                        else {
                            return None;
                        };
                        Some(crate::orm::joined(&ty, &target, alias))
                    }
                    Type::Orm {
                        kind: Kind::Projection,
                        ..
                    } if member == "all" || member == "first" => Some(Type::Class {
                        name: "SQLResult".into(),
                    }),
                    ty @ Type::Orm {
                        kind: Kind::Query | Kind::Projection,
                        ..
                    } if member == "select" => Some(crate::orm::projected(&ty)),
                    Type::Orm {
                        model,
                        kind: Kind::Query,
                        ..
                    } if member == "all" => Some(handle(&model, Kind::Rows)),
                    ty @ Type::Orm {
                        kind: Kind::Query | Kind::Projection,
                        ..
                    } if [
                        "where",
                        "orWhere",
                        "whereGroup",
                        "orWhereGroup",
                        "orWhereNull",
                        "orWhereNotNull",
                        "whereNull",
                        "whereNotNull",
                        "orderBy",
                        "limit",
                        "offset",
                    ]
                    .contains(&member.as_str()) =>
                    {
                        Some(ty)
                    }
                    Type::Class { name } => self
                        .method_return_types
                        .get(&(name, member.clone()))
                        .cloned(),
                    _ => None,
                }
            }
            _ => None,
        }
    }
    fn orm_branch_value(
        &mut self,
        condition: IrOperand,
        ty: Type,
        present: impl FnOnce(&mut Self) -> IrOperand,
    ) -> IrOperand {
        let slot = self.fresh_reg();
        self.emit(IrInstruction::Alloca {
            dest: slot,
            ty: ty.clone(),
        });
        let yes = self.fresh_block();
        let no = self.fresh_block();
        let merge = self.fresh_block();
        self.emit(IrInstruction::Branch {
            cond: condition,
            then_block: yes,
            else_block: no,
        });
        self.end_block();
        self.current_block_id = Some(yes);
        let value = present(self);
        let value = self.cast(value, ty.clone());
        self.emit(IrInstruction::Store {
            val: value,
            ptr: slot,
        });
        self.emit(IrInstruction::Jump { target: merge });
        self.end_block();
        self.current_block_id = Some(no);
        self.emit(IrInstruction::Store {
            val: IrOperand::NullOf(ty.clone()),
            ptr: slot,
        });
        self.emit(IrInstruction::Jump { target: merge });
        self.end_block();
        self.current_block_id = Some(merge);
        let dest = self.fresh_reg();
        self.emit(IrInstruction::Load {
            dest,
            ptr: slot,
            ty: ty.clone(),
        });
        IrOperand::Register(dest, ty)
    }
    fn orm_read(&mut self, rows: IrOperand, model: &str) -> IrOperand {
        let fields = crate::orm::fields(&self.class_decls[model]).unwrap();
        let mut args = vec![];
        for (index, (_, ty)) in fields.iter().enumerate() {
            let inner = if let Type::Nullable(inner) = ty {
                inner.as_ref()
            } else {
                ty
            };
            let symbol = match inner {
                Type::Int => "ube_sql_result_get_int",
                Type::Long => "ube_sql_result_get_long",
                Type::Decimal => "ube_sql_result_get_decimal",
                Type::Bool => "ube_sql_result_get_bool",
                _ => "ube_sql_result_get_string",
            };
            let getter_args = vec![rows.clone(), IrOperand::ConstantInt(index as i64)];
            let value = if matches!(ty, Type::Nullable(_)) {
                let null = self.runtime("ube_sql_result_is_null", getter_args.clone(), Type::Bool);
                let condition = self.fresh_reg();
                self.emit(IrInstruction::Binary {
                    dest: condition,
                    op: BinaryOp::Equal,
                    left: null,
                    right: IrOperand::ConstantBool(false),
                    operand_ty: Type::Bool,
                    result_ty: Type::Bool,
                });
                self.orm_branch_value(
                    IrOperand::Register(condition, Type::Bool),
                    ty.clone(),
                    |this| this.runtime(symbol, getter_args, inner.clone()),
                )
            } else {
                self.runtime(symbol, getter_args, ty.clone())
            };
            args.push(value);
        }
        let dest = self.fresh_reg();
        self.emit(IrInstruction::InstantiateClass {
            dest,
            class_name: model.into(),
            args,
        });
        IrOperand::Register(dest, Type::Class { name: model.into() })
    }
    fn orm_find_result(&mut self, rows: IrOperand, model: &str) -> IrOperand {
        let present = self.fresh_reg();
        self.emit(IrInstruction::Binary {
            dest: present,
            op: BinaryOp::NotEqual,
            left: rows.clone(),
            right: IrOperand::NullOf(rows.get_type()),
            operand_ty: rows.get_type(),
            result_ty: Type::Bool,
        });
        self.orm_branch_value(
            IrOperand::Register(present, Type::Bool),
            Type::Nullable(Box::new(Type::Class { name: model.into() })),
            |this| {
                let value = this.orm_read(rows.clone(), model);
                this.runtime("ube_sql_result_close", vec![rows], Type::Void);
                value
            },
        )
    }
    fn orm_write_params(&mut self, object: IrOperand, model: &str) -> IrOperand {
        let params = self.runtime(
            "ube_sql_params_create",
            vec![],
            Type::Class {
                name: "SQLParams".into(),
            },
        );
        let fields = crate::orm::fields(&self.class_decls[model]).unwrap();
        let IrOperand::Register(ptr, _) = object else {
            unreachable!("model object must have a register");
        };
        for (index, (_, ty)) in fields.into_iter().enumerate() {
            let dest = self.fresh_reg();
            self.emit(IrInstruction::GetField {
                dest,
                ptr,
                class_name: model.into(),
                field_index: index,
                field_ty: ty.clone(),
            });
            let value = IrOperand::Register(dest, ty.clone());
            let inner = if let Type::Nullable(inner) = &ty {
                inner.as_ref()
            } else {
                &ty
            };
            let suffix = match inner {
                Type::Int => "int",
                Type::Long => "long",
                Type::Bool => "bool",
                Type::Decimal => "decimal",
                _ => "string",
            };
            if matches!(ty, Type::Nullable(_)) {
                let condition = self.fresh_reg();
                self.emit(IrInstruction::Binary {
                    dest: condition,
                    op: BinaryOp::NotEqual,
                    left: value.clone(),
                    right: IrOperand::NullOf(ty.clone()),
                    operand_ty: ty.clone(),
                    result_ty: Type::Bool,
                });
                let yes = self.fresh_block();
                let no = self.fresh_block();
                let merge = self.fresh_block();
                self.emit(IrInstruction::Branch {
                    cond: IrOperand::Register(condition, Type::Bool),
                    then_block: yes,
                    else_block: no,
                });
                self.end_block();
                self.current_block_id = Some(yes);
                let value = self.cast(value, inner.clone());
                self.runtime(
                    &format!("ube_sql_params_add_{suffix}"),
                    vec![params.clone(), value],
                    Type::Void,
                );
                self.emit(IrInstruction::Jump { target: merge });
                self.end_block();
                self.current_block_id = Some(no);
                self.runtime(
                    "ube_sql_params_add_null",
                    vec![params.clone(), IrOperand::ConstantString(suffix.into())],
                    Type::Void,
                );
                self.emit(IrInstruction::Jump { target: merge });
                self.end_block();
                self.current_block_id = Some(merge);
            } else {
                self.runtime(
                    &format!("ube_sql_params_add_{suffix}"),
                    vec![params.clone(), value],
                    Type::Void,
                );
            }
        }
        params
    }
    fn build_orm_call(&mut self, callee: &Expr, args: &[Expr]) -> Option<IrOperand> {
        use crate::orm::{handle, Kind};
        let ExprKind::MemberAccess { object, member, .. } = &callee.kind else {
            return None;
        };
        if matches!(&object.kind, ExprKind::Identifier(n) if n == "Orm" && self.lookup_var(n).is_none())
            && member == "model"
        {
            let ExprKind::Identifier(model) = &args[1].kind else {
                return Some(IrOperand::Null);
            };
            let fields = crate::orm::fields(&self.class_decls[model]).unwrap();
            let db = self.build_expr(&args[0]);
            let table = self.build_expr(&args[2]);
            return Some(self.runtime(
                "ube_orm_model",
                vec![
                    db,
                    table,
                    IrOperand::ConstantString(crate::orm::schema(&fields)),
                ],
                handle(model, Kind::Model),
            ));
        }
        let receiver_ty = self.orm_expr_type(object)?;
        let Type::Orm { model, kind, .. } = receiver_ty.clone() else {
            return None;
        };
        let receiver = self.build_expr(object);
        let mut operands = vec![receiver.clone()];
        operands.extend(args.iter().map(|a| self.build_expr(a)));
        Some(match (kind, member.as_str()) {
            (Kind::Model, "create" | "save") => {
                let params = self.orm_write_params(operands[1].clone(), &model);
                self.runtime(
                    &format!("ube_orm_{member}"),
                    vec![receiver, params],
                    Type::Long,
                )
            }
            (Kind::Model, "find" | "get" | "delete") => {
                operands[1] = self.cast(operands[1].clone(), Type::Long);
                if member == "delete" {
                    self.runtime("ube_orm_delete", operands, Type::Long)
                } else if member == "get" {
                    let rows = self.runtime(
                        "ube_orm_get",
                        operands,
                        Type::Class {
                            name: "SQLResult".into(),
                        },
                    );
                    let value = self.orm_read(rows.clone(), &model);
                    self.runtime("ube_sql_result_close", vec![rows], Type::Void);
                    value
                } else {
                    let rows = self.runtime(
                        "ube_orm_find",
                        operands,
                        Type::Class {
                            name: "SQLResult".into(),
                        },
                    );
                    self.orm_find_result(rows, &model)
                }
            }
            (Kind::Model, "migrate" | "close") => self.runtime(
                if member == "close" {
                    "ube_orm_model_close"
                } else {
                    "ube_orm_migrate"
                },
                operands,
                Type::Void,
            ),
            (Kind::Model, "using") => {
                self.runtime("ube_orm_using", operands, handle(&model, Kind::Model))
            }
            (Kind::Model, "query") => {
                self.runtime("ube_orm_query", operands, handle(&model, Kind::Query))
            }
            (Kind::Query | Kind::Projection, "join" | "leftJoin") => {
                let Type::Orm { model: target, .. } = operands[1].get_type() else {
                    unreachable!()
                };
                let (ExprKind::StringLiteral(alias) | ExprKind::TextBlock(alias)) = &args[1].kind
                else {
                    unreachable!()
                };
                let result = crate::orm::joined(&receiver_ty, &target, alias);
                operands.push(IrOperand::ConstantBool(member == "leftJoin"));
                self.runtime("ube_orm_join", operands, result)
            }
            (Kind::Query | Kind::Projection, "where" | "orWhere") => {
                let suffix = match operands[3].get_type() {
                    Type::Int => "int",
                    Type::Long => "long",
                    Type::Decimal => "decimal",
                    Type::Bool => "bool",
                    _ => "string",
                };
                self.runtime(
                    &format!(
                        "ube_orm_{}{suffix}",
                        if member == "orWhere" {
                            "or_where_"
                        } else {
                            "where_"
                        }
                    ),
                    operands,
                    receiver_ty.clone(),
                )
            }
            (
                Kind::Query | Kind::Projection,
                "whereNull" | "whereNotNull" | "orWhereNull" | "orWhereNotNull",
            ) => {
                operands.push(IrOperand::ConstantBool(member.ends_with("NotNull")));
                self.runtime(
                    if member.starts_with("or") {
                        "ube_orm_or_where_null"
                    } else {
                        "ube_orm_where_null"
                    },
                    operands,
                    receiver_ty.clone(),
                )
            }
            (Kind::Query | Kind::Projection, "orderBy" | "limit" | "offset") => self.runtime(
                &format!(
                    "ube_orm_{}",
                    if member == "orderBy" {
                        "order_by"
                    } else {
                        member
                    }
                ),
                operands,
                receiver_ty.clone(),
            ),
            (Kind::Query | Kind::Projection, "select") => {
                operands.push(IrOperand::ConstantBool(kind == Kind::Query));
                self.runtime(
                    "ube_orm_select",
                    operands,
                    crate::orm::projected(&receiver_ty),
                )
            }
            (Kind::Query | Kind::Projection, "whereGroup" | "orWhereGroup") => {
                operands.push(IrOperand::ConstantBool(member == "orWhereGroup"));
                self.runtime("ube_orm_where_group", operands, receiver_ty.clone())
            }
            (Kind::Projection, "all" | "first") => self.runtime(
                if member == "all" {
                    "ube_orm_all"
                } else {
                    "ube_orm_first"
                },
                operands,
                Type::Class {
                    name: "SQLResult".into(),
                },
            ),
            (Kind::Query, "all") => {
                self.runtime("ube_orm_all", operands, handle(&model, Kind::Rows))
            }
            (Kind::Query, "first") => {
                let rows = self.runtime(
                    "ube_orm_first",
                    operands,
                    Type::Class {
                        name: "SQLResult".into(),
                    },
                );
                self.orm_find_result(rows, &model)
            }
            (Kind::Query | Kind::Projection, "count") => {
                self.runtime("ube_orm_count", operands, Type::Long)
            }
            (Kind::Query | Kind::Projection, "sum" | "avg" | "min" | "max" | "countColumn") => {
                let (ExprKind::StringLiteral(column) | ExprKind::TextBlock(column)) = &args[0].kind
                else {
                    unreachable!()
                };
                let Type::Orm { joins, .. } = &receiver_ty else {
                    unreachable!()
                };
                let (column_model, column_name) =
                    if let Some((alias, name)) = column.split_once('.') {
                        (
                            if alias == "base" {
                                model.as_str()
                            } else {
                                joins
                                    .iter()
                                    .find(|(a, _)| a.eq_ignore_ascii_case(alias))
                                    .unwrap()
                                    .1
                                    .as_str()
                            },
                            name,
                        )
                    } else {
                        (model.as_str(), column.as_str())
                    };
                let fields = crate::orm::fields(&self.class_decls[column_model]).unwrap();
                let ty = fields
                    .iter()
                    .find(|(name, _)| name == column_name)
                    .unwrap()
                    .1
                    .clone();
                let inner = if let Type::Nullable(t) = ty { *t } else { ty };
                let result = if member == "countColumn" {
                    Type::Long
                } else if matches!(member.as_str(), "sum" | "avg") {
                    Type::Decimal
                } else {
                    inner
                };
                operands.push(IrOperand::ConstantString(member.clone()));
                let rows = self.runtime(
                    "ube_orm_aggregate",
                    operands,
                    Type::Class {
                        name: "SQLResult".into(),
                    },
                );
                let getter = match result {
                    Type::Int => "int",
                    Type::Long => "long",
                    Type::Decimal => "decimal",
                    _ => "string",
                };
                let value = if member == "countColumn" {
                    self.runtime(
                        "ube_sql_result_get_long",
                        vec![rows.clone(), IrOperand::ConstantInt(0)],
                        Type::Long,
                    )
                } else {
                    let is_null = self.runtime(
                        "ube_sql_result_is_null",
                        vec![rows.clone(), IrOperand::ConstantInt(0)],
                        Type::Bool,
                    );
                    let present = self.fresh_reg();
                    self.emit(IrInstruction::Binary {
                        dest: present,
                        op: BinaryOp::Equal,
                        left: is_null,
                        right: IrOperand::ConstantBool(false),
                        operand_ty: Type::Bool,
                        result_ty: Type::Bool,
                    });
                    self.orm_branch_value(
                        IrOperand::Register(present, Type::Bool),
                        Type::Nullable(Box::new(result.clone())),
                        |this| {
                            this.runtime(
                                &format!("ube_sql_result_get_{getter}"),
                                vec![rows.clone(), IrOperand::ConstantInt(0)],
                                result,
                            )
                        },
                    )
                };
                self.runtime("ube_sql_result_close", vec![rows], Type::Void);
                value
            }

            (Kind::Query | Kind::Projection, "close") => {
                self.runtime("ube_orm_query_close", operands, Type::Void)
            }
            (Kind::Rows, "get") => {
                let receiver = self.cast(
                    receiver,
                    Type::Class {
                        name: "SQLResult".into(),
                    },
                );
                self.orm_read(receiver, &model)
            }
            (Kind::Rows, method) => {
                let suffix = if method == "rowCount" {
                    "row_count"
                } else {
                    method
                };
                let result = match method {
                    "next" => Type::Bool,
                    "rowCount" => Type::Long,
                    _ => Type::Void,
                };
                operands[0] = self.cast(
                    receiver,
                    Type::Class {
                        name: "SQLResult".into(),
                    },
                );
                self.runtime(&format!("ube_sql_result_{suffix}"), operands, result)
            }
            _ => IrOperand::Null,
        })
    }
}
