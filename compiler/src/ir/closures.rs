// First-class functions use traced objects and the existing structural call ABI.
impl IrBuilder {
    fn build_closure(&mut self, params: &[Param], expression: &Expr) -> IrOperand {
        self.build_function_closure(params, Some(expression), &[], None)
    }

    fn build_function_closure(&mut self, params: &[Param], expression: Option<&Expr>, statements: &[Stmt], checked_return_type: Option<Type>) -> IrOperand {
        let class_name = format!("__ube.closure.{}", self.next_closure);
        self.next_closure += 1;
        let mut references = std::collections::HashSet::new();
        if let Some(expression) = expression {
            closure_references(expression, &mut references);
        }
        closure_statement_references(statements, &mut references);
        let mut captures = std::collections::BTreeMap::new();
        for scope in &self.var_pointers {
            for (name, value) in scope {
                if references.contains(name) && !params.iter().any(|param| param.name == *name) {
                    captures.insert(name.clone(), value.clone());
                }
            }
        }
        let mut fields = Vec::new();
        let mut values = Vec::new();
        for (name, (pointer, ty)) in &captures {
            let value = self.fresh_reg();
            self.emit(IrInstruction::Load {
                dest: value,
                ptr: *pointer,
                ty: ty.clone(),
            });
            fields.push((name.clone(), ty.clone()));
            values.push(IrOperand::Register(value, ty.clone()));
        }

        let mut body = self.clone();
        body.next_reg = 0;
        body.next_block = 0;
        body.blocks.clear();
        body.current_instructions.clear();
        body.var_pointers.clear();
        body.loop_stack.clear();
        body.exception_handlers.clear();
        body.cleanups.clear();
        body.cleanup_pending.clear();
        body.loop_cleanup_depths.clear();
        body.yield_target_stack.clear();
        body.generated_functions.clear();
        body.generated_classes.clear();
        body.in_cleanup = false;
        body.current_method_async = false;
        body.current_method_return_type = checked_return_type.clone().unwrap_or(Type::Void);
        body.diagnostics.clear();
        let entry = body.fresh_block();
        body.current_block_id = Some(entry);
        body.push_var_scope();
        let receiver = body.fresh_reg();
        // Allocate all ABI parameters before any instruction registers.
        let arguments: Vec<_> = params
            .iter()
            .map(|p| (body.fresh_reg(), body.type_spec_to_type(&p.type_spec)))
            .collect();
        for (index, (name, (_, ty))) in captures.iter().enumerate() {
            let value = body.fresh_reg();
            body.emit(IrInstruction::GetField {
                dest: value,
                ptr: receiver,
                class_name: class_name.clone(),
                field_index: index,
                field_ty: ty.clone(),
            });
            let slot = body.fresh_reg();
            body.emit(IrInstruction::Alloca {
                dest: slot,
                ty: ty.clone(),
            });
            body.emit(IrInstruction::Store {
                val: IrOperand::Register(value, ty.clone()),
                ptr: slot,
            });
            body.insert_var(name.clone(), slot, ty.clone());
        }
        for (param, (value, ty)) in params.iter().zip(&arguments) {
            let slot = body.fresh_reg();
            body.emit(IrInstruction::Alloca {
                dest: slot,
                ty: ty.clone(),
            });
            body.emit(IrInstruction::Store {
                val: IrOperand::Register(*value, ty.clone()),
                ptr: slot,
            });
            body.insert_var(param.name.clone(), slot, ty.clone());
        }
        let result = if let Some(expression) = expression {
            body.build_expr(expression)
        } else {
            for statement in statements {
                if body.is_current_terminated() { break; }
                body.build_stmt(statement);
            }
            IrOperand::Null
        };
        let return_type = checked_return_type.unwrap_or_else(|| {
            if matches!(result, IrOperand::Null)
                && !expression.is_some_and(|expr| matches!(expr.kind, ExprKind::NullLiteral)) {
                Type::Void
            } else {
                result.get_type()
            }
        });
        // Exception exits were built before the inferred return type was known.
        if return_type != Type::Void {
            for block in &mut body.blocks {
                for instruction in &mut block.instructions {
                    if let IrInstruction::Return { val } = instruction {
                        if val.is_none() {
                            *val = Some(Self::default_operand_for_type(&return_type));
                        }
                    }
                }
            }
        }
        if !body.is_current_terminated() {
            body.emit(IrInstruction::Return {
                // Control-flow lowering can leave an unreachable merge block.
                val: (return_type != Type::Void).then(|| {
                    if expression.is_some() { result } else { Self::default_operand_for_type(&return_type) }
                }),
            });
        }
        body.end_block();
        self.next_closure = body.next_closure;
        self.next_async_thunk = body.next_async_thunk;
        self.next_server_handler = body.next_server_handler;
        self.diagnostics.extend(body.diagnostics);
        self.generated_classes.extend(body.generated_classes);
        self.generated_functions.extend(body.generated_functions);
        self.generated_classes.push(IrClass {
            name: class_name.clone(),
            fields,
        });
        let function_type = Type::Function {
            params: arguments.iter().map(|(_, ty)| ty.clone()).collect(),
            return_type: Box::new(return_type.clone()),
        };
        self.generated_functions.push(IrFunction {
            class_name: class_name.clone(),
            name: "invoke".into(),
            receiver: (receiver, function_type.clone()),
            params: arguments,
            return_type,
            is_async: false,
            is_static: false,
            blocks: body.blocks,
        });
        let dest = self.fresh_reg();
        self.emit(IrInstruction::InstantiateClass {
            dest,
            class_name,
            args: values,
        });
        IrOperand::Register(dest, function_type)
    }

    fn build_function_call(&mut self, callee: &Expr, args: &[Expr]) -> IrOperand {
        if let ExprKind::Identifier(name) = &callee.kind {
            if self.lookup_var(name).is_none() {
                self.diagnostics.push(Diagnostic::error(
                    "Unqualified method calls require 'this.' or 'static.'",
                    Some(callee.span),
                ));
                return IrOperand::Null;
            }
        }
        let receiver = self.build_expr(callee);
        let Type::Function {
            params,
            return_type,
        } = receiver.get_type()
        else {
            self.diagnostics.push(Diagnostic::error(
                "Call requires a Func value",
                Some(callee.span),
            ));
            return IrOperand::Null;
        };
        let arguments = args
            .iter()
            .zip(&params)
            .map(|(arg, ty)| {
                let value = self.build_expr(arg);
                self.cast(value, ty.clone())
            })
            .collect();
        let result_ty = *return_type;
        let dest = (result_ty != Type::Void).then(|| self.fresh_reg());
        self.emit(IrInstruction::CallTrait {
            dest,
            signature: method_signature("invoke", &params, &result_ty),
            receiver,
            args: arguments,
            ret_ty: result_ty.clone(),
        });
        dest.map_or(IrOperand::Null, |reg| IrOperand::Register(reg, result_ty))
    }

    fn build_function_handler(&mut self, expression: &Expr) -> Option<(String, IrOperand)> {
        let context = self.build_expr(expression);
        let request = Type::Class {
            name: "Request".into(),
        };
        let response = Type::Class {
            name: "Response".into(),
        };
        let expected = Type::Function {
            params: vec![request.clone()],
            return_type: Box::new(response.clone()),
        };
        if context.get_type() != expected {
            self.diagnostics.push(Diagnostic::error(
                "Server routes require Func(Request) -> Response",
                Some(expression.span),
            ));
            return None;
        }
        let class_name = format!("__ube.funcroute.{}", self.next_server_handler);
        self.next_server_handler += 1;
        let mut adapter = Self::new();
        adapter.current_method_return_type = response.clone();
        let entry = adapter.fresh_block();
        adapter.current_block_id = Some(entry);
        let receiver = adapter.fresh_reg();
        let argument = adapter.fresh_reg();
        let result = adapter.fresh_reg();
        adapter.emit(IrInstruction::CallTrait {
            dest: Some(result),
            signature: method_signature("invoke", &[request.clone()], &response),
            receiver: IrOperand::Register(receiver, expected.clone()),
            args: vec![IrOperand::Register(argument, request.clone())],
            ret_ty: response.clone(),
        });
        adapter.emit(IrInstruction::Return {
            val: Some(IrOperand::Register(result, response.clone())),
        });
        adapter.end_block();
        self.generated_functions.push(IrFunction {
            class_name: class_name.clone(),
            name: "handle".into(),
            receiver: (receiver, expected),
            params: vec![(argument, request)],
            return_type: response,
            is_async: false,
            is_static: false,
            blocks: adapter.blocks,
        });
        Some((format!("{}_handle", class_name), context))
    }
}

fn closure_references(expr: &Expr, names: &mut std::collections::HashSet<String>) {
    let mut visit = |expr: &Expr| closure_references(expr, names);
    match &expr.kind {
        ExprKind::BlockClosure { params, body, .. } => {
            let mut nested = std::collections::HashSet::new();
            closure_statement_references(body, &mut nested);
            for param in params { nested.remove(&param.name); }
            names.extend(nested);
        }
        ExprKind::Closure { params, expression } => {
            let mut nested = std::collections::HashSet::new();
            closure_references(expression, &mut nested);
            for param in params {
                nested.remove(&param.name);
            }
            names.extend(nested);
        }
        ExprKind::Call { callee, args } => {
            closure_references(callee, names);
            for arg in args {
                closure_references(arg, names);
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
            closure_references(target, names);
            for case in cases {
                closure_references(&case.pattern, names);
                if let Some(guard) = &case.guard {
                    closure_references(guard, names);
                }
                closure_statement_references(&case.body, names);
                if let Some(result) = &case.result_expr {
                    closure_references(result, names);
                }
            }
            if let Some(default) = default_case {
                closure_references(default, names);
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
        ExprKind::Lambda { body, .. } => closure_statement_references(body, names),
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            closure_references(condition, names);
            closure_statement_references(then_branch, names);
            if let Some(body) = else_branch {
                closure_statement_references(body, names);
            }
        }
        ExprKind::For {
            condition_or_iterable,
            body,
            else_branch,
            ..
        } => {
            if let Some(condition) = condition_or_iterable {
                closure_references(condition, names);
            }
            closure_statement_references(body, names);
            if let Some(body) = else_branch {
                closure_statement_references(body, names);
            }
        }
        ExprKind::Identifier(name) => {
            names.insert(name.clone());
        }
        ExprKind::This => {
            names.insert("this".into());
        }
        ExprKind::StringLiteral(text) => {
            let mut rest = text.as_str();
            while let Some(start) = rest.find("${") {
                rest = &rest[start + 2..];
                let mut depth = 1;
                let mut end = None;
                for (index, ch) in rest.char_indices() {
                    if ch == '{' {
                        depth += 1;
                    }
                    if ch == '}' {
                        depth -= 1;
                    }
                    if depth == 0 {
                        end = Some(index);
                        break;
                    }
                }
                let Some(end) = end else {
                    break;
                };
                let mut parser = Parser::new(crate::lexer::Lexer::new(&rest[..end]));
                if let Ok(expression) = parser.parse_expr(Precedence::None) {
                    closure_references(&expression, names);
                }
                rest = &rest[end + 1..];
            }
        }
        ExprKind::IntLiteral(_)
        | ExprKind::LongLiteral(_)
        | ExprKind::DecimalLiteral(_)
        | ExprKind::TextBlock(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::NullLiteral => {}
    }
}
fn closure_statement_references(body: &[Stmt], names: &mut std::collections::HashSet<String>) {
    for statement in body {
        match &statement.kind {
            StmtKind::LocalDecl { initializer, .. } | StmtKind::Return(initializer) => {
                if let Some(expr) = initializer {
                    closure_references(expr, names);
                }
            }
            StmtKind::Expr(expr) | StmtKind::Throw(expr) | StmtKind::Yield(expr) => {
                closure_references(expr, names)
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                closure_references(condition, names);
                closure_statement_references(then_branch, names);
                if let Some(body) = else_branch {
                    closure_statement_references(body, names);
                }
            }
            StmtKind::For {
                condition_or_iterable,
                body,
                else_branch,
                ..
            } => {
                if let Some(condition) = condition_or_iterable {
                    closure_references(condition, names);
                }
                closure_statement_references(body, names);
                if let Some(body) = else_branch {
                    closure_statement_references(body, names);
                }
            }
            StmtKind::Try {
                body,
                catches,
                finally_body,
            } => {
                closure_statement_references(body, names);
                for catch in catches {
                    if let Some(condition) = &catch.condition {
                        closure_references(condition, names);
                    }
                    closure_statement_references(&catch.body, names);
                }
                if let Some(body) = finally_body {
                    closure_statement_references(body, names);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
        }
    }
}
