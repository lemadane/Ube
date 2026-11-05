use super::nodes::*;
use crate::builtins::canonical_builtin_class_name;
use crate::diagnostics::Diagnostic;
use crate::parser::parser::Precedence;
use crate::parser::*;
use crate::types::Type;
use std::collections::HashMap;

#[derive(Clone)]
struct Cleanup {
    body: Vec<Stmt>,
    scopes: Vec<HashMap<String, (Register, Type)>>,
    handlers: Vec<(Register, BlockId)>,
    loops: Vec<(BlockId, BlockId)>,
    loop_depths: Vec<usize>,
}

#[derive(Clone)]
pub struct IrBuilder {
    next_reg: usize,
    next_block: usize,
    current_instructions: Vec<IrInstruction>,
    blocks: Vec<BasicBlock>,
    var_pointers: Vec<HashMap<String, (Register, Type)>>,
    method_return_types: HashMap<(String, String), Type>,
    class_fields: HashMap<(String, String), Type>,
    class_field_indices: HashMap<(String, String), (usize, Type)>,
    class_decls: HashMap<String, ClassDecl>,
    next_async_thunk: usize,
    loop_stack: Vec<(BlockId, BlockId)>,
    exception_handlers: Vec<(Register, BlockId)>,
    cleanups: Vec<Cleanup>,
    cleanup_pending: Vec<IrOperand>,
    trait_members: HashMap<String, Vec<String>>,
    trait_signatures: HashMap<(String, String), (Vec<Type>, Type)>,
    loop_cleanup_depths: Vec<usize>,
    in_cleanup: bool,
    yield_target_stack: Vec<(Option<Register>, Option<Register>)>,
    current_block_id: Option<BlockId>,
    current_method_async: bool,
    current_method_return_type: Type,
    generated_functions: Vec<IrFunction>,
    generated_classes: Vec<IrClass>,
    next_server_handler: usize,
    pub diagnostics: Vec<Diagnostic>,
}

impl IrBuilder {
    pub fn new() -> Self {
        Self {
            next_reg: 0,
            next_block: 0,
            current_instructions: Vec::new(),
            blocks: Vec::new(),
            var_pointers: Vec::new(),
            method_return_types: HashMap::new(),
            class_fields: HashMap::new(),
            class_field_indices: HashMap::new(),
            class_decls: HashMap::new(),
            next_async_thunk: 0,
            loop_stack: Vec::new(),
            exception_handlers: Vec::new(),
            cleanups: Vec::new(),
            cleanup_pending: Vec::new(),
            trait_members: HashMap::new(),
            trait_signatures: HashMap::new(),
            loop_cleanup_depths: Vec::new(),
            in_cleanup: false,
            yield_target_stack: Vec::new(),
            current_block_id: None,
            current_method_async: false,
            current_method_return_type: Type::Void,
            generated_functions: Vec::new(),
            generated_classes: Vec::new(),
            next_server_handler: 0,
            diagnostics: Vec::new(),
        }
    }

    fn build_server_handler(&mut self, lambda: &Expr) -> Option<(String, IrOperand)> {
        let ExprKind::Lambda { parameter, body } = &lambda.kind else {
            self.diagnostics.push(Diagnostic::error(
                "Server routes require a `{ request -> Response(...) }` handler",
                Some(lambda.span),
            ));
            return None;
        };
        let Some((context_slot, context_ty)) = self.lookup_var("this") else {
            self.diagnostics.push(Diagnostic::error(
                "Server route handlers must be registered from an instance method",
                Some(lambda.span),
            ));
            return None;
        };
        let context_reg = self.fresh_reg();
        self.emit(IrInstruction::Load {
            dest: context_reg,
            ptr: context_slot,
            ty: context_ty.clone(),
        });
        let mut captures = std::collections::BTreeMap::new();
        for scope in &self.var_pointers {
            for (name, (pointer, ty)) in scope {
                if name != "this" {
                    captures.insert(name.clone(), (*pointer, ty.clone()));
                }
            }
        }
        let mut closure_fields = vec![("_this".to_string(), context_ty.clone())];
        let mut closure_values = vec![IrOperand::Register(context_reg, context_ty.clone())];
        for (name, (pointer, ty)) in &captures {
            let value = self.fresh_reg();
            self.emit(IrInstruction::Load {
                dest: value,
                ptr: *pointer,
                ty: ty.clone(),
            });
            closure_fields.push((name.clone(), ty.clone()));
            closure_values.push(IrOperand::Register(value, ty.clone()));
        }

        let handler_index = self.next_server_handler;
        self.next_server_handler += 1;
        let class_name = format!("UbeRouteHandler{}", handler_index);
        let context_class_name = format!("UbeRouteContext{}", handler_index);
        let method_name = "handle".to_string();
        let context_class_ty = Type::Class {
            name: context_class_name.clone(),
        };
        let closure_context = self.fresh_reg();
        self.emit(IrInstruction::InstantiateClass {
            dest: closure_context,
            class_name: context_class_name.clone(),
            args: closure_values,
        });
        self.generated_classes.push(IrClass {
            name: context_class_name.clone(),
            fields: closure_fields,
        });

        let mut handler_builder = self.clone();
        handler_builder.next_reg = 0;
        handler_builder.next_block = 0;
        handler_builder.blocks.clear();
        handler_builder.current_instructions.clear();
        handler_builder.var_pointers.clear();
        handler_builder.loop_stack.clear();
        handler_builder.exception_handlers.clear();
        handler_builder.cleanups.clear();
        handler_builder.cleanup_pending.clear();
        handler_builder.loop_cleanup_depths.clear();
        handler_builder.yield_target_stack.clear();
        handler_builder.generated_functions.clear();
        handler_builder.current_method_async = false;
        let return_type = Type::Class {
            name: "Response".into(),
        };
        handler_builder.current_method_return_type = return_type.clone();

        let entry = handler_builder.fresh_block();
        handler_builder.current_block_id = Some(entry);
        handler_builder.push_var_scope();

        let receiver = handler_builder.fresh_reg();
        let request_ty = Type::Class {
            name: "Request".into(),
        };
        let request = handler_builder.fresh_reg();
        let receiver_slot = handler_builder.fresh_reg();
        handler_builder.emit(IrInstruction::Alloca {
            dest: receiver_slot,
            ty: context_ty.clone(),
        });
        let captured_receiver = handler_builder.fresh_reg();
        handler_builder.emit(IrInstruction::GetField {
            dest: captured_receiver,
            ptr: receiver,
            class_name: context_class_name.clone(),
            field_index: 0,
            field_ty: context_ty.clone(),
        });
        handler_builder.emit(IrInstruction::Store {
            val: IrOperand::Register(captured_receiver, context_ty.clone()),
            ptr: receiver_slot,
        });
        handler_builder.insert_var("this".into(), receiver_slot, context_ty.clone());

        for (index, (name, (_, ty))) in captures.iter().enumerate() {
            let value = handler_builder.fresh_reg();
            handler_builder.emit(IrInstruction::GetField {
                dest: value,
                ptr: receiver,
                class_name: context_class_name.clone(),
                field_index: index + 1,
                field_ty: ty.clone(),
            });
            let slot = handler_builder.fresh_reg();
            handler_builder.emit(IrInstruction::Alloca {
                dest: slot,
                ty: ty.clone(),
            });
            handler_builder.emit(IrInstruction::Store {
                val: IrOperand::Register(value, ty.clone()),
                ptr: slot,
            });
            handler_builder.insert_var(name.clone(), slot, ty.clone());
        }

        let request_slot = handler_builder.fresh_reg();
        handler_builder.emit(IrInstruction::Alloca {
            dest: request_slot,
            ty: request_ty.clone(),
        });
        handler_builder.emit(IrInstruction::Store {
            val: IrOperand::Register(request, request_ty.clone()),
            ptr: request_slot,
        });
        handler_builder.insert_var(parameter.clone(), request_slot, request_ty.clone());

        for (index, statement) in body.iter().enumerate() {
            if handler_builder.is_current_terminated() {
                break;
            }
            if index + 1 == body.len() {
                if let StmtKind::Expr(result_expr) = &statement.kind {
                    let result = handler_builder.build_expr(result_expr);
                    if result.get_type() != return_type {
                        self.diagnostics.push(Diagnostic::error(
                            format!(
                                "Server route handlers must return Response, found '{}'",
                                result.get_type()
                            ),
                            Some(result_expr.span),
                        ));
                        return None;
                    }
                    handler_builder.emit(IrInstruction::Return { val: Some(result) });
                    continue;
                }
            }
            handler_builder.build_stmt(statement);
        }
        if !handler_builder.is_current_terminated() {
            self.diagnostics.push(Diagnostic::error(
                "Server route handler must return a Response on every path",
                Some(lambda.span),
            ));
            handler_builder.emit(IrInstruction::Return {
                val: Some(IrOperand::NullOf(return_type.clone())),
            });
        }
        let last_block = handler_builder.current_block_id.unwrap_or(entry);
        handler_builder.finish_block(last_block);
        handler_builder.pop_var_scope();

        self.generated_functions
            .extend(std::mem::take(&mut handler_builder.generated_functions));
        self.generated_functions.push(IrFunction {
            class_name: class_name.clone(),
            name: method_name.clone(),
            receiver: (receiver, context_class_ty),
            params: vec![(request, request_ty)],
            return_type,
            is_async: false,
            blocks: std::mem::take(&mut handler_builder.blocks),
        });
        Some((
            format!("{}_{}", class_name, method_name),
            IrOperand::Register(
                closure_context,
                Type::Class {
                    name: context_class_name,
                },
            ),
        ))
    }

    fn type_spec_to_type(&self, ts: &TypeSpec) -> Type {
        match ts {
            TypeSpec::Int => Type::Int,
            TypeSpec::Decimal => Type::Decimal,
            TypeSpec::Byte => Type::Byte,
            TypeSpec::Long => Type::Long,
            TypeSpec::Bool => Type::Bool,
            TypeSpec::String => Type::String,
            TypeSpec::Class(n) => {
                if n == "void" {
                    Type::Void
                } else {
                    Type::Class {
                        name: canonical_builtin_class_name(n).unwrap_or(n).into(),
                    }
                }
            }
            TypeSpec::Generic(name, element) if name == "Channel" => {
                Type::Channel(Box::new(self.type_spec_to_type(element)))
            }
            TypeSpec::Generic(name, _) => Type::Class {
                name: canonical_builtin_class_name(name).unwrap_or(name).into(),
            },
            TypeSpec::Nullable(inner) => Type::Nullable(Box::new(self.type_spec_to_type(inner))),
        }
    }

    fn fresh_reg(&mut self) -> Register {
        let r = Register(self.next_reg);
        self.next_reg += 1;
        r
    }

    fn fresh_block(&mut self) -> BlockId {
        let b = BlockId(self.next_block);
        self.next_block += 1;
        b
    }

    fn emit(&mut self, instr: IrInstruction) {
        let check = matches!(instr, IrInstruction::CallMethod { .. } | IrInstruction::CallTrait { .. })
            || matches!(&instr, IrInstruction::CallRuntime { symbol, .. }
                if symbol.starts_with("ube_jwt_") || symbol.starts_with("ube_oauth2_") || symbol.starts_with("ube_session_") || matches!(symbol.as_str(), "ube_request_session" | "ube_request_query" | "ube_request_header" | "ube_response_redirect") || symbol.starts_with("ube_console_") || symbol.starts_with("ube_output_") || symbol.starts_with("ube_log_") || symbol.starts_with("ube_sqlite_") || symbol.starts_with("ube_file_") || symbol.starts_with("ube_directory_")
                    || symbol.starts_with("ube_stream_") || symbol.starts_with("ube_server_"));
        self.current_instructions.push(instr);
        if check {
            let error = self.runtime("ube_exception_take", vec![], Type::Bytes);
            self.check_exception(error);
        }
    }

    fn finish_block(&mut self, id: BlockId) {
        let instrs = std::mem::take(&mut self.current_instructions);
        self.blocks.push(BasicBlock {
            id,
            instructions: instrs,
        });
    }

    fn is_current_terminated(&self) -> bool {
        self.current_instructions.last().map_or(false, |i| {
            matches!(
                i,
                IrInstruction::Return { .. }
                    | IrInstruction::Jump { .. }
                    | IrInstruction::Branch { .. }
            )
        })
    }

    fn runtime(&mut self, symbol: &str, args: Vec<IrOperand>, ty: Type) -> IrOperand {
        let dest = (ty != Type::Void).then(|| self.fresh_reg());
        self.emit(IrInstruction::CallRuntime { dest, symbol: symbol.into(), args, ret_ty: ty.clone() });
        dest.map_or(IrOperand::Null, |d| IrOperand::Register(d, ty))
    }

    fn cast(&mut self, value: IrOperand, ty: Type) -> IrOperand {
        if value.get_type() == ty { return value; }
        if matches!(value, IrOperand::Null | IrOperand::NullOf(_)) { return IrOperand::NullOf(ty); }
        let dest = self.fresh_reg();
        self.emit(IrInstruction::Cast { dest, value, ty: ty.clone() });
        IrOperand::Register(dest, ty)
    }

    fn end_block(&mut self) {
        if let Some(block) = self.current_block_id { self.finish_block(block); }
    }

    fn check_exception(&mut self, record: IrOperand) {
        let condition = self.fresh_reg();
        self.emit(IrInstruction::Binary { dest: condition, op: BinaryOp::NotEqual,
            left: record.clone(), right: IrOperand::NullOf(Type::Bytes), operand_ty: Type::Bytes, result_ty: Type::Bool });
        let failed = self.fresh_block();
        let success = self.fresh_block();
        self.emit(IrInstruction::Branch { cond: IrOperand::Register(condition, Type::Bool), then_block: failed, else_block: success });
        self.end_block();
        self.current_block_id = Some(failed);
        self.route_exception(record);
        self.end_block();
        self.current_block_id = Some(success);
    }

    fn route_exception(&mut self, record: IrOperand) {
        for original in self.cleanup_pending.clone() {
            self.runtime("ube_exception_suppress", vec![record.clone(), original], Type::Void);
        }
        if let Some((slot, target)) = self.exception_handlers.last().copied() {
            self.emit(IrInstruction::Store { val: record, ptr: slot });
            self.emit(IrInstruction::Jump { target });
        } else {
            self.runtime("ube_exception_set", vec![record], Type::Void);
            self.emit(IrInstruction::Return { val: (self.current_method_return_type != Type::Void)
                .then(|| Self::default_operand_for_type(&self.current_method_return_type)) });
        }
    }

    fn object_matches(&mut self, object: IrOperand, type_name: &str) -> IrOperand {
        let raw = self.cast(object, Type::Bytes);
        if self.trait_members.contains_key(type_name) {
            let mut signatures: Vec<_> = self.trait_signatures.iter().filter(|((name,_),_)| name == type_name)
                .map(|((_,method),(params,result))| method_signature(method, params, result)).collect();
            signatures.sort();
            self.runtime("ube_object_satisfies", vec![raw, IrOperand::ConstantString(signatures.join("\n"))], Type::Bool)
        } else {
            self.runtime("ube_object_is", vec![raw, IrOperand::ConstantString(type_name.into())], Type::Bool)
        }
    }

    fn build_trait_call(&mut self, trait_name: &str, member: &str, object: IrOperand, args: Vec<IrOperand>) -> IrOperand {
        let (params, result_ty) = self.trait_signatures.get(&(trait_name.into(), member.into())).cloned().unwrap_or((vec![], Type::Void));
        let signature = method_signature(member, &params, &result_ty);
        let args = args.into_iter().enumerate().map(|(i,a)| self.cast(a.clone(), params.get(i).cloned().unwrap_or(a.get_type()))).collect();
        let receiver = self.cast(object, Type::Bytes);
        let dest = (result_ty != Type::Void).then(|| self.fresh_reg());
        self.emit(IrInstruction::CallTrait { dest, signature, receiver, args, ret_ty: result_ty.clone() });
        dest.map_or(IrOperand::Null, |d| IrOperand::Register(d, result_ty))
    }

    fn format_output_value(&mut self, mut value: IrOperand) -> IrOperand {
        if let Type::Class { name } = value.get_type() {
            if name == "Error" || self.trait_members.get("Error").is_some_and(|members| members.contains(&name)) {
                value = self.build_trait_call("Error", "message", value, vec![]);
            }
        }
        let ty = value.get_type();
        let inner = match &ty { Type::Nullable(inner) => inner.as_ref(), _ => &ty };
        if !matches!(inner, Type::String | Type::Int | Type::Long | Type::Byte | Type::Bool | Type::Decimal | Type::Void) {
            self.diagnostics.push(Diagnostic::error("String interpolation requires a scalar, string, or Error value", None));
            return IrOperand::ConstantString(String::new());
        }
        self.runtime("ube_format_value", vec![value], Type::String)
    }

    fn make_exception(&mut self, value: IrOperand, span: crate::diagnostics::Span) -> IrOperand {
        let value = if value.get_type() == Type::String {
            let dest = self.fresh_reg();
            self.emit(IrInstruction::InstantiateClass { dest, class_name: "GenericError".into(), args: vec![value] });
            IrOperand::Register(dest, Type::Class { name: "GenericError".into() })
        } else { value };
        let Type::Class { name } = value.get_type() else { return IrOperand::NullOf(Type::Bytes); };
        let message = if self.trait_members.contains_key(&name) {
            self.build_trait_call(&name, "message", value.clone(), vec![])
        } else {
            let dest = self.fresh_reg();
            self.emit(IrInstruction::CallMethod { dest: Some(dest), class_name: name.clone(), method_name: "message".into(), receiver: value.clone(), args: vec![], ret_ty: Type::String });
            IrOperand::Register(dest, Type::String)
        };
        let raw = self.cast(value, Type::Bytes);
        self.runtime("ube_exception_create", vec![raw, IrOperand::NullOf(Type::String), message,
            IrOperand::ConstantString(format!("{}:{}", span.start.line, span.start.column))], Type::Bytes)
    }

    fn run_cleanup(&mut self, index: usize, pending: Option<IrOperand>) {
        let frame = self.cleanups[index].clone();
        let saved_cleanups = self.cleanups.clone();
        let saved_scopes = self.var_pointers.clone();
        let saved_handlers = self.exception_handlers.clone();
        let saved_loops = self.loop_stack.clone();
        let saved_depths = self.loop_cleanup_depths.clone();
        let saved_in_cleanup = self.in_cleanup;
        self.cleanups.truncate(index);
        self.var_pointers = frame.scopes;
        self.exception_handlers = frame.handlers;
        self.loop_stack = frame.loops;
        self.loop_cleanup_depths = frame.loop_depths;
        self.in_cleanup = true;
        if let Some(pending) = &pending { self.cleanup_pending.push(pending.clone()); }
        self.push_var_scope();
        for stmt in &frame.body {
            self.build_stmt(stmt);
            if self.is_current_terminated() { break; }
        }
        self.pop_var_scope();
        if pending.is_some() { self.cleanup_pending.pop(); }
        self.in_cleanup = saved_in_cleanup;
        self.cleanups = saved_cleanups;
        self.var_pointers = saved_scopes;
        self.exception_handlers = saved_handlers;
        self.loop_stack = saved_loops;
        self.loop_cleanup_depths = saved_depths;
    }

    fn emit_cleanups(&mut self, depth: usize) {
        for index in (depth..self.cleanups.len()).rev() {
            self.run_cleanup(index, None);
            if self.is_current_terminated() { break; }
        }
    }

    fn build_try(&mut self, body: &[Stmt], catches: &[CatchClause], finally: &Option<Vec<Stmt>>) {
        let slot = self.fresh_reg();
        self.emit(IrInstruction::Alloca { dest: slot, ty: Type::Bytes });
        let dispatch = self.fresh_block();
        let propagate = self.fresh_block();
        let merge = self.fresh_block();
        let cleanup_index = self.cleanups.len();
        self.cleanups.push(Cleanup { body: finally.clone().unwrap_or_default(), scopes: self.var_pointers.clone(), handlers: self.exception_handlers.clone(), loops: self.loop_stack.clone(), loop_depths: self.loop_cleanup_depths.clone() });
        self.exception_handlers.push((slot, dispatch));
        self.push_var_scope();
        for stmt in body { self.build_stmt(stmt); if self.is_current_terminated() { break; } }
        self.pop_var_scope();
        self.exception_handlers.pop();
        if !self.is_current_terminated() {
            self.run_cleanup(cleanup_index, None);
            if !self.is_current_terminated() { self.emit(IrInstruction::Jump { target: merge }); }
        }
        self.end_block();
        self.current_block_id = Some(dispatch);
        let record_reg = self.fresh_reg();
        self.emit(IrInstruction::Load { dest: record_reg, ptr: slot, ty: Type::Bytes });
        let record = IrOperand::Register(record_reg, Type::Bytes);
        let object = self.runtime("ube_exception_object", vec![record], Type::Bytes);
        for handler in catches {
            let matched = self.object_matches(object.clone(), &handler.type_name);
            let handler_block = self.fresh_block();
            let next = self.fresh_block();
            self.emit(IrInstruction::Branch { cond: matched, then_block: handler_block, else_block: next });
            self.end_block();
            self.current_block_id = Some(handler_block);
            self.push_var_scope();
            let ty = Type::Class { name: handler.type_name.clone() };
            let variable_slot = self.fresh_reg();
            self.emit(IrInstruction::Alloca { dest: variable_slot, ty: ty.clone() });
            let variable = self.cast(object.clone(), ty.clone());
            self.emit(IrInstruction::Store { val: variable, ptr: variable_slot });
            self.insert_var(handler.variable.clone(), variable_slot, ty);
            self.exception_handlers.push((slot, propagate));
            if let Some(condition) = &handler.condition {
                let condition = self.build_expr(condition);
                let accepted = self.fresh_block();
                self.emit(IrInstruction::Branch { cond: condition, then_block: accepted, else_block: next });
                self.end_block();
                self.current_block_id = Some(accepted);
            }
            for stmt in &handler.body { self.build_stmt(stmt); if self.is_current_terminated() { break; } }
            self.exception_handlers.pop();
            self.pop_var_scope();
            if !self.is_current_terminated() {
                self.run_cleanup(cleanup_index, None);
                if !self.is_current_terminated() { self.emit(IrInstruction::Jump { target: merge }); }
            }
            self.end_block();
            self.current_block_id = Some(next);
        }
        self.emit(IrInstruction::Jump { target: propagate });
        self.end_block();
        self.current_block_id = Some(propagate);
        let pending_reg = self.fresh_reg();
        self.emit(IrInstruction::Load { dest: pending_reg, ptr: slot, ty: Type::Bytes });
        let pending = IrOperand::Register(pending_reg, Type::Bytes);
        self.run_cleanup(cleanup_index, Some(pending.clone()));
        self.cleanups.pop();
        if !self.is_current_terminated() { self.route_exception(pending); }
        self.end_block();
        self.current_block_id = Some(merge);
    }

    fn route_await_error(&mut self, error_dest: Register) {
        self.check_exception(IrOperand::Register(error_dest, Type::Bytes));
    }

    fn build_deferred_expression(&mut self, expression: &Expr) -> IrOperand {
        let mut captured = std::collections::BTreeMap::new();
        for scope in &self.var_pointers {
            for (name, (pointer, ty)) in scope {
                captured.insert(name.clone(), (*pointer, ty.clone()));
            }
        }

        let mut capture_values = Vec::with_capacity(captured.len());
        let mut capture_info = Vec::with_capacity(captured.len());
        for (name, (pointer, ty)) in captured {
            let value = self.fresh_reg();
            self.emit(IrInstruction::Load {
                dest: value,
                ptr: pointer,
                ty: ty.clone(),
            });
            capture_values.push(IrOperand::Register(value, ty.clone()));
            capture_info.push((name, ty));
        }

        let callback = format!("ube_async_expr_thunk_{}", self.next_async_thunk);
        let class_name = format!("UbeAsyncExpr{}", self.next_async_thunk);
        self.next_async_thunk += 1;
        let method_name = "run".to_owned();

        let mut deferred = self.clone();
        deferred.next_reg = 0;
        deferred.next_block = 0;
        deferred.blocks.clear();
        deferred.current_instructions.clear();
        deferred.var_pointers.clear();
        deferred.loop_stack.clear();
        deferred.exception_handlers.clear();
        deferred.cleanups.clear();
        deferred.cleanup_pending.clear();
        deferred.loop_cleanup_depths.clear();
        deferred.yield_target_stack.clear();
        deferred.generated_functions.clear();
        deferred.current_method_async = true;
        // The result type is only known after lowering; early failure returns
        // are emitted valueless and patched below.
        deferred.current_method_return_type = Type::Void;
        let entry = deferred.fresh_block();
        deferred.current_block_id = Some(entry);

        let receiver = deferred.fresh_reg();
        let mut params = Vec::new();
        deferred.push_var_scope();
        let mut parameters = Vec::new();
        for (name, ty) in &capture_info {
            let parameter = deferred.fresh_reg();
            params.push((parameter, ty.clone()));
            parameters.push((name, ty, parameter));
        }
        for (name, ty, parameter) in parameters {
            let slot = deferred.fresh_reg();
            deferred.emit(IrInstruction::Alloca {
                dest: slot,
                ty: ty.clone(),
            });
            deferred.emit(IrInstruction::Store {
                val: IrOperand::Register(parameter, ty.clone()),
                ptr: slot,
            });
            deferred.insert_var(name.clone(), slot, ty.clone());
        }

        let value = deferred.build_expr(expression);
        let result_ty = value.get_type();
        if result_ty != Type::Void {
            for block in &mut deferred.blocks {
                for instruction in &mut block.instructions {
                    if let IrInstruction::Return { val } = instruction {
                        if val.is_none() {
                            *val = Some(Self::default_operand_for_type(&result_ty));
                        }
                    }
                }
            }
        }
        deferred.emit(IrInstruction::Return { val: Some(value) });
        let last_block = deferred.current_block_id.unwrap_or(entry);
        deferred.finish_block(last_block);
        self.next_async_thunk = deferred.next_async_thunk;
        let known_diagnostics = self.diagnostics.len();
        self.diagnostics
            .extend(deferred.diagnostics.drain(known_diagnostics..));
        let nested_functions = std::mem::take(&mut deferred.generated_functions);
        self.generated_functions.extend(nested_functions);
        self.generated_functions.push(IrFunction {
            class_name: class_name.clone(),
            name: method_name.clone(),
            receiver: (receiver, Type::Int),
            params,
            return_type: result_ty.clone(),
            is_async: true,
            blocks: std::mem::take(&mut deferred.blocks),
        });

        let dest = self.fresh_reg();
        self.emit(IrInstruction::SpawnAsyncExpression {
            dest,
            callback,
            class_name,
            method_name,
            captures: capture_values,
            result_ty: result_ty.clone(),
        });
        IrOperand::Register(dest, Type::Task(Box::new(result_ty)))
    }

    fn push_var_scope(&mut self) {
        self.var_pointers.push(HashMap::new());
    }

    fn pop_var_scope(&mut self) {
        self.var_pointers.pop();
    }

    fn insert_var(&mut self, name: String, reg: Register, ty: Type) {
        if let Some(scope) = self.var_pointers.last_mut() {
            scope.insert(name, (reg, ty));
        }
    }

    fn lookup_var(&self, name: &str) -> Option<(Register, Type)> {
        for scope in self.var_pointers.iter().rev() {
            if let Some(entry) = scope.get(name) {
                return Some(entry.clone());
            }
        }
        None
    }

    fn condition_to_bool(&mut self, condition: IrOperand) -> IrOperand {
        let condition_ty = condition.get_type();
        if condition_ty == Type::Bool {
            return condition;
        }

        let dest = self.fresh_reg();
        let (op, zero) = match condition_ty {
            Type::Decimal => (BinaryOp::NotEqual, IrOperand::ConstantDecimal(0.0)),
            Type::Int | Type::Long | Type::Byte => (BinaryOp::NotEqual, IrOperand::ConstantInt(0)),
            _ => (BinaryOp::NotEqual, IrOperand::Null),
        };
        self.emit(IrInstruction::Binary {
            dest,
            op,
            left: condition,
            right: zero,
            operand_ty: condition_ty,
            result_ty: Type::Bool,
        });
        IrOperand::Register(dest, Type::Bool)
    }

    fn yield_type_in_stmts(&self, statements: &[Stmt]) -> Option<Type> {
        let mut probe = self.clone();
        probe.push_var_scope();
        for stmt in statements {
            let result = match &stmt.kind {
                StmtKind::Yield(expr) => Some(probe.build_expr(expr).get_type()),
                StmtKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => probe.yield_type_in_stmts(then_branch).or_else(|| {
                    else_branch
                        .as_ref()
                        .and_then(|s| probe.yield_type_in_stmts(s))
                }),
                StmtKind::For {
                    body, else_branch, ..
                } => probe.yield_type_in_stmts(body).or_else(|| {
                    else_branch
                        .as_ref()
                        .and_then(|s| probe.yield_type_in_stmts(s))
                }),
                StmtKind::Try { body, catches, finally_body } => probe.yield_type_in_stmts(body)
                    .or_else(|| catches.iter().find_map(|c| probe.yield_type_in_stmts(&c.body)))
                    .or_else(|| finally_body.as_ref().and_then(|b| probe.yield_type_in_stmts(b))),
                _ => None,
            };
            if let Some(ty) = result {
                if !matches!(ty, Type::Nullable(ref inner) if **inner == Type::Void) {
                    return Some(ty);
                }
            }
            probe.build_stmt(stmt);
        }
        None
    }

    fn default_operand_for_type(ty: &Type) -> IrOperand {
        match ty {
            Type::Int | Type::Byte => IrOperand::ConstantInt(0),
            Type::Long => IrOperand::ConstantLong(0),
            Type::Decimal => IrOperand::ConstantDecimal(0.0),
            Type::Bool => IrOperand::ConstantBool(false),
            Type::String
            | Type::Class { .. }
            | Type::Bytes
            | Type::InputStream
            | Type::OutputStream
            | Type::FileStream
            | Type::Task(_)
            | Type::Channel(_) => IrOperand::NullOf(ty.clone()),
            Type::Nullable(_) => IrOperand::NullOf(ty.clone()),
            _ => IrOperand::Null,
        }
    }

    fn scale_integer_operand(&mut self, operand: IrOperand, factor: i64) -> IrOperand {
        let dest = self.fresh_reg();
        self.emit(IrInstruction::Binary {
            dest,
            op: BinaryOp::Mul,
            left: operand,
            right: IrOperand::ConstantInt(factor),
            operand_ty: Type::Int,
            result_ty: Type::Int,
        });
        IrOperand::Register(dest, Type::Int)
    }

    pub fn build_program(&mut self, program: &Program) -> IrProgram {
        let mut normalized = program.clone();
        for name in ["SQLiteError", "GenericError", "OutOfBoundsError", "FileError", "FileNotFoundError", "PermissionError", "AlreadyExistsError", "InvalidPathError", "DiskFullError", "LocationPermissionError", "LocationUnavailableError", "JsonError", "JsonParseError", "MissingFieldError", "InvalidTypeError", "XmlError", "XmlParseError"] {
            if !normalized.items.iter().any(|i| matches!(i, Item::Class(c) if c.name == name)) {
                normalized.items.push(Item::Class(ClassDecl { name: name.into(), primary_params: vec![ConstructorParam {
                    name: "message".into(), type_spec: TypeSpec::String, is_var: false,
                    default_value: Some(Expr { kind: ExprKind::StringLiteral(String::new()), span: Default::default() }), span: Default::default()
                }], cases: vec![], methods: vec![], span: Default::default() }));
            }
        }
        let program = &normalized;
        let mut checker = crate::semantics::TypeChecker::new();
        checker.check_program(program);
        self.trait_members.clear();
        self.trait_signatures.clear();
        let mut traits = vec!["Error".to_string()];
        traits.extend(program.items.iter().filter_map(|i| if let Item::Trait(t) = i { Some(t.name.clone()) } else { None }));
        for name in traits {
            let mut members: Vec<String> = program.items.iter().filter_map(|i| if let Item::Class(c) = i {
                checker.satisfies_trait(&c.name, &name).then(|| c.name.clone())
            } else { None }).collect();
            members.sort();
            self.trait_members.insert(name, members);
        }
        self.trait_signatures.insert(("Error".into(), "message".into()), (vec![], Type::String));
        for item in &program.items {
            if let Item::Trait(t) = item {
                for m in &t.methods {
                    self.trait_signatures.insert((t.name.clone(), m.name.clone()), (m.params.iter().map(|p| self.type_spec_to_type(&p.type_spec)).collect(), m.return_type.as_ref().map_or(Type::Void, |t| self.type_spec_to_type(t))));
                }
            }
        }

        self.method_return_types.clear();
        self.class_fields.clear();
        self.class_field_indices.clear();
        self.class_decls.clear();
        self.next_async_thunk = 0;
        self.next_server_handler = 0;
        self.generated_functions.clear();
        self.generated_classes.clear();

        for item in &program.items {
            if let Item::Class(cls) = item {
                self.class_decls.insert(cls.name.clone(), cls.clone());
            }
        }

        let mut ir_classes = Vec::new();
        let mut ir_functions = Vec::new();

        // 1. Collect all concrete classes
        let mut concrete_classes = Vec::new();
        for item in &program.items {
            if let Item::Class(cls) = item {
                concrete_classes.push(cls.clone());
            }
        }

        // 2. Pre-register method return types and class field types & indices
        for item in &program.items {
            match item {
                Item::Class(cls) => {
                    for (i, param) in cls.primary_params.iter().enumerate() {
                        let field_ty = self.type_spec_to_type(&param.type_spec);
                        self.class_fields.insert(
                            (cls.name.clone(), format!("_{}", param.name)),
                            field_ty.clone(),
                        );
                        self.class_fields
                            .insert((cls.name.clone(), param.name.clone()), field_ty.clone());
                        self.class_field_indices.insert(
                            (cls.name.clone(), format!("_{}", param.name)),
                            (i, field_ty.clone()),
                        );
                        self.class_field_indices.insert(
                            (cls.name.clone(), param.name.clone()),
                            (i, field_ty.clone()),
                        );
                        self.method_return_types
                            .insert((cls.name.clone(), param.name.clone()), field_ty);
                    }
                    for case in &cls.cases {
                        self.method_return_types.insert(
                            (cls.name.clone(), case.name.clone()),
                            Type::Class {
                                name: cls.name.clone(),
                            },
                        );
                    }
                    for method in &cls.methods {
                        let ret_ty = method
                            .return_type
                            .as_ref()
                            .map_or(Type::Void, |t| self.type_spec_to_type(t));
                        self.method_return_types
                            .insert((cls.name.clone(), method.name.clone()), ret_ty.clone());

                        let has_trait_param = method
                            .params
                            .iter()
                            .any(|p| matches!(&p.type_spec, TypeSpec::Class(name) if canonical_builtin_class_name(name).is_none() || self.trait_members.contains_key(name)));
                        if has_trait_param {
                            for target_cls in &concrete_classes {
                                if target_cls.name != cls.name {
                                    let spec_name = format!("{}_{}", method.name, target_cls.name);
                                    self.method_return_types
                                        .insert((cls.name.clone(), spec_name), ret_ty.clone());
                                }
                            }
                        }
                    }
                }
                Item::Trait(tr) => {
                    for method in &tr.methods {
                        let ret_ty = method
                            .return_type
                            .as_ref()
                            .map_or(Type::Void, |t| self.type_spec_to_type(t));
                        self.method_return_types
                            .insert((tr.name.clone(), method.name.clone()), ret_ty.clone());
                        for cls in &concrete_classes {
                            self.method_return_types
                                .entry((cls.name.clone(), method.name.clone()))
                                .or_insert_with(|| ret_ty.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        // 3. Build classes and traits
        for item in &program.items {
            match item {
                Item::Class(cls) => {
                    let (ir_cls, fns) = self.build_class(cls, &concrete_classes);
                    ir_classes.push(ir_cls);
                    ir_functions.extend(fns);
                }
                Item::Trait(tr) => {
                    let dummy_cls = ClassDecl {
                        name: tr.name.clone(),
                        primary_params: Vec::new(),
                        cases: Vec::new(),
                        methods: tr.methods.clone(),
                        span: tr.span,
                    };
                    for method in &tr.methods {
                        let ir_fn = self.build_trait_method(&dummy_cls, method, &concrete_classes);
                        ir_functions.push(ir_fn);
                    }
                }
                _ => {}
            }
        }

        ir_functions.append(&mut self.generated_functions);
        ir_classes.append(&mut self.generated_classes);
        if let Some(entry) = ir_functions.iter().find(|function| function.name.eq_ignore_ascii_case("main")) {
            let ty = Type::Class { name: entry.class_name.clone() };
            let mut startup = Self::new();
            startup.class_decls = self.class_decls.clone();
            startup.current_method_return_type = ty.clone();
            let receiver = startup.fresh_reg();
            let block = startup.fresh_block();
            startup.current_block_id = Some(block);
            let value = startup.build_zero_value(&ty, &mut Vec::new());
            startup.emit(IrInstruction::Return { val: Some(value) });
            startup.end_block();
            self.diagnostics.extend(startup.diagnostics);
            ir_functions.push(IrFunction {
                // The dot prevents collisions with source-level class names.
                class_name: "__ube.internal".into(), name: "zero".into(),
                receiver: (receiver, Type::Bytes), params: Vec::new(),
                return_type: ty, is_async: false, blocks: startup.blocks,
            });
        }
        IrProgram {
            classes: ir_classes,
            functions: ir_functions,
        }
    }

    fn build_class(
        &mut self,
        cls: &ClassDecl,
        concrete_classes: &[ClassDecl],
    ) -> (IrClass, Vec<IrFunction>) {
        let mut fields = Vec::new();

        for param in &cls.primary_params {
            let field_name = format!("_{}", param.name);
            let field_ty = self.type_spec_to_type(&param.type_spec);
            fields.push((field_name, field_ty));
        }

        let ir_cls = IrClass {
            name: cls.name.clone(),
            fields,
        };

        let mut functions = Vec::new();

        for (i, param) in cls.primary_params.iter().enumerate() {
            let getter_fn = self.build_getter(cls, param, i);
            functions.push(getter_fn);
        }

        for method in &cls.methods {
            let has_trait_param = method
                .params
                .iter()
                .any(|p| matches!(&p.type_spec, TypeSpec::Class(name) if canonical_builtin_class_name(name).is_none() || self.trait_members.contains_key(name)));
            if has_trait_param {
                // Monomorphize method for each concrete class
                for target_cls in concrete_classes {
                    if target_cls.name != cls.name {
                        let mut specialized_method = method.clone();
                        specialized_method.name = format!("{}_{}", method.name, target_cls.name);
                        for p in &mut specialized_method.params {
                            if matches!(&p.type_spec, TypeSpec::Class(name) if canonical_builtin_class_name(name).is_none() || self.trait_members.contains_key(name)) {
                                p.type_spec = TypeSpec::Class(target_cls.name.clone());
                            }
                        }
                        let spec_fn = self.build_method(cls, &specialized_method);
                        functions.push(spec_fn);
                    }
                }
            }
            let ir_fn = self.build_method(cls, method);
            functions.push(ir_fn);
        }

        (ir_cls, functions)
    }

    fn build_getter(
        &mut self,
        cls: &ClassDecl,
        param: &ConstructorParam,
        field_index: usize,
    ) -> IrFunction {
        self.next_reg = 0;
        self.next_block = 0;
        self.blocks.clear();
        self.current_instructions.clear();
        self.var_pointers.clear();

        let entry_block = self.fresh_block();
        let receiver_reg = self.fresh_reg();
        let this_ty = Type::Class {
            name: cls.name.clone(),
        };

        let field_ty = self.type_spec_to_type(&param.type_spec);

        let load_reg = self.fresh_reg();
        self.emit(IrInstruction::GetField {
            dest: load_reg,
            ptr: receiver_reg,
            class_name: cls.name.clone(),
            field_index,
            field_ty: field_ty.clone(),
        });
        self.emit(IrInstruction::Return {
            val: Some(IrOperand::Register(load_reg, field_ty.clone())),
        });

        self.finish_block(entry_block);

        IrFunction {
            class_name: cls.name.clone(),
            name: param.name.clone(),
            receiver: (receiver_reg, this_ty),
            params: Vec::new(),
            return_type: field_ty,
            is_async: false,
            blocks: std::mem::take(&mut self.blocks),
        }
    }

    fn build_trait_method(
        &mut self,
        cls: &ClassDecl,
        method: &MethodDecl,
        _concrete_classes: &[ClassDecl],
    ) -> IrFunction {
        if !method.body.is_empty() {
            return self.build_method(cls, method);
        }

        self.next_reg = 0;
        self.next_block = 0;
        self.blocks.clear();
        self.current_instructions.clear();
        self.var_pointers.clear();

        let entry_block = self.fresh_block();
        let receiver_reg = self.fresh_reg();
        let this_ty = Type::Class {
            name: cls.name.clone(),
        };

        let mut param_info = Vec::new();
        let mut ir_params = Vec::new();

        for p in &method.params {
            let p_reg = self.fresh_reg();
            let p_ty = self.type_spec_to_type(&p.type_spec);
            ir_params.push((p_reg, p_ty.clone()));
            param_info.push((p.name.clone(), p_reg, p_ty));
        }

        let ret_ty = method
            .return_type
            .as_ref()
            .map_or(Type::Void, |t| self.type_spec_to_type(t));
        let ret_val = if ret_ty == Type::Void {
            None
        } else {
            Some(Self::default_operand_for_type(&ret_ty))
        };
        self.emit(IrInstruction::Return { val: ret_val });
        self.finish_block(entry_block);

        IrFunction {
            class_name: cls.name.clone(),
            name: method.name.clone(),
            receiver: (receiver_reg, this_ty),
            params: ir_params,
            return_type: ret_ty,
            is_async: method.is_async,
            blocks: std::mem::take(&mut self.blocks),
        }
    }

    fn build_method(&mut self, cls: &ClassDecl, method: &MethodDecl) -> IrFunction {
        self.current_method_async = method.is_async;
        let ret_ty = method.return_type.as_ref().map_or_else(
            || {
                if method.is_async {
                    Type::Int
                } else {
                    Type::Void
                }
            },
            |t| self.type_spec_to_type(t),
        );
        self.current_method_return_type = ret_ty.clone();
        self.next_reg = 0;
        self.next_block = 0;
        self.blocks.clear();
        self.current_instructions.clear();
        self.var_pointers.clear();
        self.loop_stack.clear();
        self.loop_cleanup_depths.clear();
        self.exception_handlers.clear();
        self.cleanups.clear();
        self.cleanup_pending.clear();
        self.in_cleanup = false;

        self.push_var_scope();
        let entry_block = self.fresh_block();
        self.current_block_id = Some(entry_block);

        let receiver_reg = self.fresh_reg();
        let this_ty = Type::Class {
            name: cls.name.clone(),
        };

        let mut param_info = Vec::new();
        let mut ir_params = Vec::new();

        for p in &method.params {
            let p_reg = self.fresh_reg();
            let p_ty = self.type_spec_to_type(&p.type_spec);
            ir_params.push((p_reg, p_ty.clone()));
            param_info.push((p.name.clone(), p_reg, p_ty));
        }

        let this_ptr = self.fresh_reg();
        self.emit(IrInstruction::Alloca {
            dest: this_ptr,
            ty: this_ty.clone(),
        });
        self.emit(IrInstruction::Store {
            val: IrOperand::Register(receiver_reg, this_ty.clone()),
            ptr: this_ptr,
        });
        self.insert_var("this".into(), this_ptr, this_ty.clone());

        for (p_name, p_reg, p_ty) in param_info {
            let ptr_reg = self.fresh_reg();
            self.emit(IrInstruction::Alloca {
                dest: ptr_reg,
                ty: p_ty.clone(),
            });
            self.emit(IrInstruction::Store {
                val: IrOperand::Register(p_reg, p_ty.clone()),
                ptr: ptr_reg,
            });
            self.insert_var(p_name, ptr_reg, p_ty);
        }

        for stmt in &method.body {
            self.build_stmt(stmt);
            if self.is_current_terminated() {
                break;
            }
        }

        if !self.is_current_terminated() {
            self.emit(IrInstruction::Return {
                val: if ret_ty != Type::Void {
                    Some(Self::default_operand_for_type(&ret_ty))
                } else {
                    None
                },
            });
        }

        if let Some(b_id) = self.current_block_id {
            self.finish_block(b_id);
        }

        self.pop_var_scope();

        IrFunction {
            class_name: cls.name.clone(),
            name: method.name.clone(),
            receiver: (receiver_reg, this_ty),
            params: ir_params,
            return_type: ret_ty,
            is_async: method.is_async,
            blocks: std::mem::take(&mut self.blocks),
        }
    }

    fn build_zero_value(&mut self, ty: &Type, visiting: &mut Vec<String>) -> IrOperand {
        match ty {
            Type::Int | Type::Byte => IrOperand::ConstantInt(0),
            Type::Long => IrOperand::ConstantLong(0),
            Type::Decimal => IrOperand::ConstantDecimal(0.0),
            Type::Bool => IrOperand::ConstantBool(false),
            Type::String => IrOperand::ConstantString(String::new()),
            Type::Nullable(_) => IrOperand::NullOf(ty.clone()),
            Type::Class { name } => {
                if visiting.contains(name) {
                    self.diagnostics.push(Diagnostic::error(
                        format!("cannot zero-initialize recursive non-nullable class '{}'; make the recursive field nullable", name), None));
                    return IrOperand::NullOf(ty.clone());
                }
                let Some(class) = self.class_decls.get(name).cloned() else {
                    self.diagnostics.push(Diagnostic::error(format!("type '{}' requires an explicit initializer", ty), None));
                    return IrOperand::NullOf(ty.clone());
                };
                visiting.push(name.clone());
                let args = class.primary_params.iter().map(|param| {
                    let field_ty = self.type_spec_to_type(&param.type_spec);
                    self.build_zero_value(&field_ty, visiting)
                }).collect();
                visiting.pop();
                let dest = self.fresh_reg();
                self.emit(IrInstruction::InstantiateClass { dest, class_name: name.clone(), args });
                IrOperand::Register(dest, ty.clone())
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(format!("type '{}' requires an explicit initializer", ty), None));
                IrOperand::NullOf(ty.clone())
            }
        }
    }

    pub fn build_class_instantiation(&mut self, class_name: &str, args: &[Expr]) -> IrOperand {
        if class_name == "Channel" {
            return self.build_channel_instantiation(args, Type::Int);
        }
        if class_name == "Server" {
            let dest = self.fresh_reg();
            let ty = Type::Class {
                name: "Server".into(),
            };
            self.emit(IrInstruction::CallRuntime {
                dest: Some(dest),
                symbol: "ube_server_create".into(),
                args: Vec::new(),
                ret_ty: ty.clone(),
            });
            return IrOperand::Register(dest, ty);
        }
        let mut arg_ops: Vec<IrOperand> = args.iter().map(|a| self.build_expr(a)).collect();
        if let Some(cls_decl) = self.class_decls.get(class_name).cloned() {
            if arg_ops.len() < cls_decl.primary_params.len() {
                for param in cls_decl.primary_params.iter().skip(arg_ops.len()) {
                    if let Some(def_expr) = &param.default_value {
                        arg_ops.push(self.build_expr(def_expr));
                    } else {
                        let field_ty = self.type_spec_to_type(&param.type_spec);
                        let default_op = self.build_zero_value(&field_ty, &mut vec![class_name.into()]);
                        arg_ops.push(default_op);
                    }
                }
            }
        }
        let dest = self.fresh_reg();
        self.emit(IrInstruction::InstantiateClass {
            dest,
            class_name: class_name.to_string(),
            args: arg_ops,
        });
        IrOperand::Register(
            dest,
            Type::Class {
                name: class_name.to_string(),
            },
        )
    }

    fn build_channel_instantiation(&mut self, args: &[Expr], element_ty: Type) -> IrOperand {
        let capacity = args
            .first()
            .map(|arg| self.build_expr(arg))
            .unwrap_or(IrOperand::ConstantInt(64));
        let dest = self.fresh_reg();
        let channel_ty = Type::Channel(Box::new(element_ty));
        self.emit(IrInstruction::CallRuntime {
            dest: Some(dest),
            symbol: "ube_channel_create_capacity".into(),
            args: vec![capacity],
            ret_ty: channel_ty.clone(),
        });
        IrOperand::Register(dest, channel_ty)
    }

    fn build_stmt(&mut self, stmt: &Stmt) {
        if !self.in_cleanup {
            let cancellation = self.runtime("ube_exception_poll_cancel", vec![], Type::Bytes);
            self.check_exception(cancellation);
        }

        match &stmt.kind {
            StmtKind::LocalDecl {
                name,
                initializer,
                type_annotation,
                ..
            } => {
                let init_op = if let Some(init) = initializer {
                    self.build_expr(init)
                } else if let Some(ts) = type_annotation {
                    let ty = self.type_spec_to_type(ts);
                    self.build_zero_value(&ty, &mut Vec::new())
                } else {
                    IrOperand::ConstantInt(0)
                };
                let ty = type_annotation.as_ref().map(|t| self.type_spec_to_type(t)).unwrap_or_else(|| init_op.get_type());
                let init_op = self.cast(init_op, ty.clone());
                let ptr_reg = self.fresh_reg();
                self.emit(IrInstruction::Alloca {
                    dest: ptr_reg,
                    ty: ty.clone(),
                });
                self.emit(IrInstruction::Store {
                    val: init_op,
                    ptr: ptr_reg,
                });
                self.insert_var(name.clone(), ptr_reg, ty);
            }
            StmtKind::Expr(expr) => {
                self.build_expr(expr);
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let condition = self.build_expr(condition);
                let cond_op = self.condition_to_bool(condition);
                let then_block = self.fresh_block();
                let else_block = self.fresh_block();
                let merge_block = self.fresh_block();

                if let Some(cb) = self.current_block_id {
                    self.emit(IrInstruction::Branch {
                        cond: cond_op,
                        then_block,
                        else_block: if else_branch.is_some() {
                            else_block
                        } else {
                            merge_block
                        },
                    });
                    self.finish_block(cb);
                }

                self.current_block_id = Some(then_block);
                self.push_var_scope();
                for s in then_branch {
                    self.build_stmt(s);
                    if self.is_current_terminated() {
                        break;
                    }
                }
                if !self.is_current_terminated() {
                    self.emit(IrInstruction::Jump {
                        target: merge_block,
                    });
                }
                if let Some(cb) = self.current_block_id {
                    self.finish_block(cb);
                }
                self.pop_var_scope();

                if let Some(eb) = else_branch {
                    self.current_block_id = Some(else_block);
                    self.push_var_scope();
                    for s in eb {
                        self.build_stmt(s);
                        if self.is_current_terminated() {
                            break;
                        }
                    }
                    if !self.is_current_terminated() {
                        self.emit(IrInstruction::Jump {
                            target: merge_block,
                        });
                    }
                    if let Some(cb) = self.current_block_id {
                        self.finish_block(cb);
                    }
                    self.pop_var_scope();
                }

                self.current_block_id = Some(merge_block);
            }
            StmtKind::For {
                loop_var,
                index_var,
                condition_or_iterable,
                body,
                else_branch,
            } => {
                let header_block = self.fresh_block();
                let body_block = self.fresh_block();
                let increment_block = if loop_var.is_some() {
                    Some(self.fresh_block())
                } else {
                    None
                };
                let exit_block = self.fresh_block();
                let iterable = if loop_var.is_some() {
                    condition_or_iterable
                        .as_ref()
                        .map(|expr| self.build_expr(expr))
                } else {
                    None
                };
                let iterable_index_ptr = if iterable.is_some() {
                    let ptr = self.fresh_reg();
                    self.emit(IrInstruction::Alloca {
                        dest: ptr,
                        ty: Type::Int,
                    });
                    self.emit(IrInstruction::Store {
                        val: IrOperand::ConstantInt(0),
                        ptr,
                    });
                    Some(ptr)
                } else {
                    None
                };

                let has_iter_ptr = if else_branch.is_some() {
                    let ptr = self.fresh_reg();
                    self.emit(IrInstruction::Alloca {
                        dest: ptr,
                        ty: Type::Bool,
                    });
                    self.emit(IrInstruction::Store {
                        val: IrOperand::ConstantBool(false),
                        ptr,
                    });
                    Some(ptr)
                } else {
                    None
                };

                let else_check_block = if else_branch.is_some() {
                    Some(self.fresh_block())
                } else {
                    None
                };

                let false_target = else_check_block.unwrap_or(exit_block);

                self.emit(IrInstruction::Jump {
                    target: header_block,
                });
                if let Some(cb) = self.current_block_id {
                    self.finish_block(cb);
                }

                self.current_block_id = Some(header_block);
                if !self.in_cleanup {
                    let error = self.runtime("ube_exception_poll_cancel", vec![], Type::Bytes);
                    self.check_exception(error);
                }
                let cond_op = if let (Some(list), Some(index_ptr)) = (&iterable, iterable_index_ptr)
                {
                    let index_reg = self.fresh_reg();
                    self.emit(IrInstruction::Load {
                        dest: index_reg,
                        ptr: index_ptr,
                        ty: Type::Int,
                    });
                    let length_reg = self.fresh_reg();
                    self.emit(IrInstruction::CallRuntime {
                        dest: Some(length_reg),
                        symbol: "ube_list_len".into(),
                        args: vec![list.clone()],
                        ret_ty: Type::Int,
                    });
                    let condition_reg = self.fresh_reg();
                    self.emit(IrInstruction::Binary {
                        dest: condition_reg,
                        op: BinaryOp::Less,
                        left: IrOperand::Register(index_reg, Type::Int),
                        right: IrOperand::Register(length_reg, Type::Int),
                        operand_ty: Type::Int,
                        result_ty: Type::Bool,
                    });
                    IrOperand::Register(condition_reg, Type::Bool)
                } else if let Some(cond) = condition_or_iterable {
                    let condition = self.build_expr(cond);
                    self.condition_to_bool(condition)
                } else {
                    IrOperand::ConstantBool(true)
                };

                self.emit(IrInstruction::Branch {
                    cond: cond_op,
                    then_block: body_block,
                    else_block: false_target,
                });
                self.end_block();

                self.current_block_id = Some(body_block);
                if let Some(ptr) = has_iter_ptr {
                    self.emit(IrInstruction::Store {
                        val: IrOperand::ConstantBool(true),
                        ptr,
                    });
                }

                self.push_var_scope();
                if let (Some(list), Some(index_ptr)) = (&iterable, iterable_index_ptr) {
                    let index_reg = self.fresh_reg();
                    self.emit(IrInstruction::Load {
                        dest: index_reg,
                        ptr: index_ptr,
                        ty: Type::Int,
                    });
                    if let Some(index_name) = index_var {
                        let index_slot = self.fresh_reg();
                        self.emit(IrInstruction::Alloca {
                            dest: index_slot,
                            ty: Type::Int,
                        });
                        self.emit(IrInstruction::Store {
                            val: IrOperand::Register(index_reg, Type::Int),
                            ptr: index_slot,
                        });
                        self.insert_var(index_name.clone(), index_slot, Type::Int);
                    }
                    if let Some(loop_name) = loop_var {
                        let value_reg = self.fresh_reg();
                        self.emit(IrInstruction::CallRuntime {
                            dest: Some(value_reg),
                            symbol: "ube_list_get".into(),
                            args: vec![list.clone(), IrOperand::Register(index_reg, Type::Int)],
                            ret_ty: Type::Int,
                        });
                        let value_slot = self.fresh_reg();
                        self.emit(IrInstruction::Alloca {
                            dest: value_slot,
                            ty: Type::Int,
                        });
                        self.emit(IrInstruction::Store {
                            val: IrOperand::Register(value_reg, Type::Int),
                            ptr: value_slot,
                        });
                        self.insert_var(loop_name.clone(), value_slot, Type::Int);
                    }
                }

                let continue_target = increment_block.unwrap_or(header_block);
                self.loop_stack.push((continue_target, exit_block));
                self.loop_cleanup_depths.push(self.cleanups.len());
                for s in body {
                    self.build_stmt(s);
                    if self.is_current_terminated() {
                        break;
                    }
                }
                if !self.is_current_terminated() {
                    self.emit(IrInstruction::Jump {
                        target: continue_target,
                    });
                }
                if let Some(cb) = self.current_block_id {
                    self.finish_block(cb);
                }
                if let (Some(increment), Some(index_ptr)) = (increment_block, iterable_index_ptr) {
                    self.current_block_id = Some(increment);
                    let index_reg = self.fresh_reg();
                    self.emit(IrInstruction::Load {
                        dest: index_reg,
                        ptr: index_ptr,
                        ty: Type::Int,
                    });
                    let next_index = self.fresh_reg();
                    self.emit(IrInstruction::Binary {
                        dest: next_index,
                        op: BinaryOp::Add,
                        left: IrOperand::Register(index_reg, Type::Int),
                        right: IrOperand::ConstantInt(1),
                        operand_ty: Type::Int,
                        result_ty: Type::Int,
                    });
                    self.emit(IrInstruction::Store {
                        val: IrOperand::Register(next_index, Type::Int),
                        ptr: index_ptr,
                    });
                    self.emit(IrInstruction::Jump {
                        target: header_block,
                    });
                    self.finish_block(increment);
                }
                self.pop_var_scope();
                self.loop_stack.pop();
                self.loop_cleanup_depths.pop();

                if let (Some(check_block), Some(e_stmts), Some(ptr)) =
                    (else_check_block, else_branch, has_iter_ptr)
                {
                    self.current_block_id = Some(check_block);
                    let has_iter_reg = self.fresh_reg();
                    self.emit(IrInstruction::Load {
                        dest: has_iter_reg,
                        ptr,
                        ty: Type::Bool,
                    });

                    let else_block = self.fresh_block();
                    self.emit(IrInstruction::Branch {
                        cond: IrOperand::Register(has_iter_reg, Type::Bool),
                        then_block: exit_block,
                        else_block,
                    });
                    self.finish_block(check_block);

                    self.current_block_id = Some(else_block);
                    self.push_var_scope();
                    for s in e_stmts {
                        self.build_stmt(s);
                        if self.is_current_terminated() {
                            break;
                        }
                    }
                    if !self.is_current_terminated() {
                        self.emit(IrInstruction::Jump { target: exit_block });
                    }
                    if let Some(cb) = self.current_block_id {
                        self.finish_block(cb);
                    }
                    self.pop_var_scope();
                }

                self.current_block_id = Some(exit_block);
            }
            StmtKind::Break => {
                if let Some((_, target)) = self.loop_stack.last().copied() {
                    let depth = *self.loop_cleanup_depths.last().unwrap_or(&0);
                    self.emit_cleanups(depth);
                    if !self.is_current_terminated() { self.emit(IrInstruction::Jump { target }); }
                }
            }
            StmtKind::Continue => {
                if let Some((target, _)) = self.loop_stack.last().copied() {
                    let depth = *self.loop_cleanup_depths.last().unwrap_or(&0);
                    self.emit_cleanups(depth);
                    if !self.is_current_terminated() { self.emit(IrInstruction::Jump { target }); }
                }
            }
            StmtKind::Return(expr) => {
                let val = expr.as_ref().map(|e| self.build_expr(e));
                let val = val.map(|v| self.cast(v, self.current_method_return_type.clone()));
                self.emit_cleanups(0);
                if !self.is_current_terminated() { self.emit(IrInstruction::Return { val }); }
            }
            StmtKind::Yield(expr) => {
                let val = self.build_expr(expr);
                if let Some((res_ptr_opt, list_ptr_opt)) = self.yield_target_stack.last().cloned() {
                    if let Some(res_ptr) = res_ptr_opt {
                        self.emit(IrInstruction::Store { val, ptr: res_ptr });
                    } else if let Some(list_ptr) = list_ptr_opt {
                        let obj_reg = self.fresh_reg();
                        let list_ty = Type::Class {
                            name: "List".into(),
                        };
                        self.emit(IrInstruction::Load {
                            dest: obj_reg,
                            ptr: list_ptr,
                            ty: list_ty.clone(),
                        });
                        self.emit(IrInstruction::CallRuntime {
                            dest: None,
                            symbol: "ube_list_add".into(),
                            args: vec![IrOperand::Register(obj_reg, list_ty), val],
                            ret_ty: Type::Void,
                        });
                    }
                } else {
                    self.emit(IrInstruction::Return { val: Some(val) });
                }
            }
            StmtKind::Try { body, catches, finally_body } => self.build_try(body, catches, finally_body),
            StmtKind::Throw(expr) => {
                let value = self.build_expr(expr);
                let record = self.make_exception(value, expr.span);
                self.route_exception(record);
            }
        }
    }

    fn build_expr(&mut self, expr: &Expr) -> IrOperand {
        match &expr.kind {
            ExprKind::IntLiteral(v) => IrOperand::ConstantInt(*v),
            ExprKind::LongLiteral(v) => IrOperand::ConstantLong(*v),
            ExprKind::DecimalLiteral(v) => IrOperand::ConstantDecimal(*v),
            ExprKind::TextBlock(value) => IrOperand::ConstantString(value.clone()),
            ExprKind::StringLiteral(v) => {
                if v.contains("${") {
                    let mut parts = Vec::new();
                    let mut curr = v.as_str();
                    while let Some(start) = curr.find("${") {
                        let (prefix, rest) = curr.split_at(start);
                        if !prefix.is_empty() {
                            parts.push(IrOperand::ConstantString(prefix.to_string()));
                        }
                        // Find matching closing brace balancing nested braces
                        let mut depth = 0;
                        let mut match_idx = None;
                        for (idx, ch) in rest[1..].char_indices() {
                            if ch == '{' {
                                depth += 1;
                            } else if ch == '}' {
                                depth -= 1;
                                if depth == 0 {
                                    match_idx = Some(idx + 1);
                                    break;
                                }
                            }
                        }

                        if let Some(end) = match_idx {
                            let expr_str = &rest[2..end];
                            let lexer = crate::lexer::Lexer::new(expr_str);
                            let mut parser = crate::parser::Parser::new(lexer);
                            if let Ok(parsed_expr) = parser.parse_expr(Precedence::None) {
                                let expr_op = self.build_expr(&parsed_expr);
                                parts.push(self.format_output_value(expr_op));
                            } else if let Some((ptr, ty)) = self.lookup_var(expr_str) {
                                let dest = self.fresh_reg();
                                self.emit(IrInstruction::Load {
                                    dest,
                                    ptr,
                                    ty: ty.clone(),
                                });
                                parts.push(self.format_output_value(IrOperand::Register(dest, ty)));
                            }
                            curr = &rest[end + 1..];
                        } else {
                            break;
                        }
                    }
                    if !curr.is_empty() {
                        parts.push(IrOperand::ConstantString(curr.to_string()));
                    }
                    if parts.is_empty() {
                        IrOperand::ConstantString("".into())
                    } else if parts.len() == 1 {
                        parts[0].clone()
                    } else {
                        let mut acc = parts[0].clone();
                        for next in &parts[1..] {
                            let dest = self.fresh_reg();
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: "ube_string_concat".into(),
                                args: vec![acc, next.clone()],
                                ret_ty: Type::String,
                            });
                            acc = IrOperand::Register(dest, Type::String);
                        }
                        acc
                    }
                } else {
                    IrOperand::ConstantString(v.clone())
                }
            }
            ExprKind::BoolLiteral(v) => IrOperand::ConstantBool(*v),
            ExprKind::NullLiteral => IrOperand::Null,
            ExprKind::This => {
                let (ptr, ty) = self.lookup_var("this").expect("'this' pointer missing");
                let dest = self.fresh_reg();
                self.emit(IrInstruction::Load {
                    dest,
                    ptr,
                    ty: ty.clone(),
                });
                IrOperand::Register(dest, ty)
            }
            ExprKind::Identifier(name) => {
                let (ptr, ty) = self.lookup_var(name).expect("Variable missing in IR");
                let dest = self.fresh_reg();
                self.emit(IrInstruction::Load {
                    dest,
                    ptr,
                    ty: ty.clone(),
                });
                IrOperand::Register(dest, ty)
            }
            ExprKind::ClassInstantiation {
                class_name,
                generic_type,
                args,
            } => {
                if class_name == "Channel" {
                    let ty = generic_type
                        .as_ref()
                        .map(|t| self.type_spec_to_type(t))
                        .unwrap_or(Type::Int);
                    self.build_channel_instantiation(args, ty)
                } else {
                    self.build_class_instantiation(class_name, args)
                }
            }
            ExprKind::Call { callee, args } => {
                let mut prepared_receiver = None;
                if let ExprKind::MemberAccess { object, member, .. } = &callee.kind {
                    if let Some(method) = match member.as_str() {
                        "get" => Some("GET"),
                        "post" => Some("POST"),
                        "put" => Some("PUT"),
                        "patch" => Some("PATCH"),
                        "delete" => Some("DELETE"),
                        "options" => Some("OPTIONS"),
                        "head" => Some("HEAD"),
                        _ => None,
                    } {
                        let is_static = matches!(&object.kind, ExprKind::Identifier(name) if self.lookup_var(name).is_none());
                        if !is_static {
                            let receiver = self.build_expr(object);
                            let is_server = receiver.get_type() == (Type::Class { name: "Server".into() });
                            prepared_receiver = Some(receiver);
                            if is_server {
                                if args.len() != 2 {
                                    self.diagnostics.push(Diagnostic::error(
                                        format!("Server.{} requires a path and a handler", member),
                                        Some(expr.span),
                                    ));
                                    return IrOperand::Null;
                                }
                                let server = prepared_receiver.take().unwrap();
                                let path = self.build_expr(&args[0]);
                                let Some((handler_symbol, context)) = self.build_server_handler(&args[1])
                                else {
                                    return IrOperand::Null;
                                };
                                self.emit(IrInstruction::ServerRoute {
                                    server,
                                    path,
                                    method: method.into(),
                                    context,
                                    handler_symbol,
                                });
                                return IrOperand::Null;
                            }
                        }
                    }
                }
                let arg_ops: Vec<IrOperand> = args.iter().map(|a| self.build_expr(a)).collect();
                match &callee.kind {
                    ExprKind::MemberAccess { object, member, .. } => {
                        let static_class_name = match &object.kind {
                            ExprKind::Identifier(name) if self.lookup_var(name).is_none() => Some(
                                canonical_builtin_class_name(name)
                                    .unwrap_or(name)
                                    .to_string(),
                            ),
                            _ => None,
                        };
                        let obj_op = if static_class_name.is_some() {
                            IrOperand::Null
                        } else {
                            prepared_receiver.take().unwrap_or_else(|| self.build_expr(object))
                        };
                        let dest = self.fresh_reg();
                        let mut class_name =
                            static_class_name.unwrap_or_else(|| match obj_op.get_type() {
                                Type::Class { name } => name,
                                Type::Channel(_) => "Channel".into(),
                                Type::Nullable(inner) => match *inner {
                                    Type::Class { name } => name,
                                    _ => "Application".into(),
                                },
                                _ => "Application".into(),
                            });
                        if crate::builtins::AUTH_CLASSES.contains(&class_name.as_str()) {
                            if let Some(method) = crate::builtins::auth_methods(&class_name).into_iter().find(|m| m.name == member) {
                                let mut args = Vec::new();
                                if !method.is_static { args.push(obj_op); }
                                args.extend(arg_ops.into_iter().zip(&method.params).map(|(arg, ty)| self.cast(arg, ty.clone())));
                                return self.runtime(method.symbol, args, method.result);
                            }
                        }
                        if class_name == "Session" {
                            let mut args = vec![obj_op];
                            args.extend(arg_ops);
                            let result = if member == "get" { Type::Nullable(Box::new(Type::String)) } else { Type::Void };
                            return self.runtime(&format!("ube_session_{}", member), args, result);
                        }
                        if class_name == "Log" || class_name == "Console" {
                            if class_name == "Console" && ["read", "readLine"].contains(&member.as_str()) {
                                let symbol = if member == "read" { "ube_console_read" } else { "ube_console_read_line" };
                                return self.runtime(symbol, arg_ops, Type::Nullable(Box::new(Type::String)));
                            }
                            if class_name == "Log" && ["put", "get", "remove", "clear"].contains(&member.as_str()) {
                                let result = if member == "get" { Type::Nullable(Box::new(Type::String)) } else { Type::Void };
                                return self.runtime(&format!("ube_log_{}", member), arg_ops, result);
                            }
                            if member == "setLevel" {
                                return self.runtime("ube_log_set_level", arg_ops, Type::Void);
                            }
                            let mode = match member.as_str() {
                                "debug" => 0, "info" => 1, "warn" => 2, "error" => 3,
                                "print" => 4, "printLine" => 5, _ => 1,
                            };
                            let mut value = arg_ops.into_iter().next().unwrap_or(IrOperand::Null);
                            if matches!(value.get_type(), Type::Class { .. }) {
                                value = self.build_trait_call("Error", "message", value, vec![]);
                            }
                            let ty = value.get_type();
                            let inner = ty.nullable_scalar_inner().unwrap_or(&ty);
                            let suffix = match inner {
                                Type::Int | Type::Long | Type::Byte => "i64",
                                Type::Decimal => "f64", Type::Bool => "bool", _ => "str",
                            };
                            self.runtime(&format!("ube_output_{}", suffix), vec![IrOperand::ConstantInt(mode), value], Type::Void);
                            return IrOperand::Null;
                        }
                        if self.trait_members.contains_key(&class_name) {
                            return self.build_trait_call(&class_name, member, obj_op, arg_ops);
                        }

                        let async_method = self
                            .class_decls
                            .get(&class_name)
                            .and_then(|decl| {
                                decl.methods.iter().find(|method| method.name == *member)
                            })
                            .filter(|method| method.is_async)
                            .cloned();
                        if let Some(method) = async_method {
                            let arg_ops = arg_ops.into_iter().zip(&method.params).map(|(value, param)| {
                                self.cast(value, self.type_spec_to_type(&param.type_spec))
                            }).collect();
                            let dest = self.fresh_reg();
                            let result_ty = method
                                .return_type
                                .as_ref()
                                .map_or(Type::Int, |ty| self.type_spec_to_type(ty));
                            let callback = format!("ube_async_thunk_{}", self.next_async_thunk);
                            self.next_async_thunk += 1;
                            self.emit(IrInstruction::SpawnAsyncCall {
                                dest,
                                callback,
                                class_name,
                                method_name: member.clone(),
                                receiver: obj_op,
                                args: arg_ops,
                                result_ty: result_ty.clone(),
                            });
                            return IrOperand::Register(dest, Type::Task(Box::new(result_ty)));
                        }

                        let setter_field = self.class_decls.get(&class_name)
                            .and_then(|c| c.primary_params.iter().enumerate()
                                .find(|(_, p)| p.name == *member && p.is_var && arg_ops.len() == 1))
                            .map(|(index, p)| (index, self.type_spec_to_type(&p.type_spec)));
                        if let Some((field_index, field_ty)) = setter_field {
                            let value = self.cast(arg_ops[0].clone(), field_ty);
                            if let IrOperand::Register(ptr, _) = obj_op {
                                self.emit(IrInstruction::StoreField { ptr, class_name, field_index, val: value });
                            }
                            return IrOperand::Null;
                        }

                        if self.class_decls.get(&class_name).is_some_and(|c| c.methods.iter().any(|m| m.name == *member) || c.primary_params.iter().any(|p| p.name == *member)) {
                            let mut method_name = member.clone();
                            if member == "render" {
                                if let Some(IrOperand::Register(_, Type::Class { name })) = arg_ops.first() { method_name = format!("{}_{}", member, name); }
                            }
                            let ret_ty = self.method_return_types.get(&(class_name.clone(), method_name.clone())).cloned().unwrap_or(Type::Void);
                            let dest = (ret_ty != Type::Void).then(|| self.fresh_reg());
                            self.emit(IrInstruction::CallMethod { dest, class_name, method_name, receiver: obj_op, args: arg_ops, ret_ty: ret_ty.clone() });
                            return dest.map_or(IrOperand::Null, |d| IrOperand::Register(d, ret_ty));
                        }

                        if class_name == "List" {
                            let symbol = match member.as_str() {
                                "add" => "ube_list_add",
                                "len" => "ube_list_len",
                                _ => "ube_list_len",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = vec![obj_op];
                            call_args.extend(arg_ops);
                            let ret_ty = if member == "len" {
                                Type::Int
                            } else {
                                Type::Void
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: if ret_ty == Type::Void {
                                    None
                                } else {
                                    Some(dest)
                                },
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return if ret_ty == Type::Void {
                                IrOperand::Null
                            } else {
                                IrOperand::Register(dest, ret_ty)
                            };
                        }
                        if class_name == "Map" {
                            let symbol = match member.as_str() {
                                "len" => "ube_map_len",
                                _ => "ube_map_len",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = vec![obj_op];
                            call_args.extend(arg_ops);
                            let ret_ty = if member == "len" {
                                Type::Int
                            } else {
                                Type::Void
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: if ret_ty == Type::Void {
                                    None
                                } else {
                                    Some(dest)
                                },
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return if ret_ty == Type::Void {
                                IrOperand::Null
                            } else {
                                IrOperand::Register(dest, ret_ty)
                            };
                        }
                        if class_name == "Channel" {
                            let arg_ops = if member == "send" {
                                if let Type::Channel(element) = obj_op.get_type() {
                                    arg_ops.into_iter().map(|value| self.cast(value, (*element).clone())).collect()
                                } else { arg_ops }
                            } else { arg_ops };
                            let symbol = match member.as_str() {
                                "send" => "ube_channel_send_async",
                                "close" => "ube_channel_close",
                                _ => "ube_channel_recv_async",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = vec![obj_op];
                            call_args.extend(arg_ops);
                            let ret_ty = match member.as_str() {
                                "send" => Type::Task(Box::new(Type::Bool)),
                                "receive" => match call_args[0].get_type() {
                                    Type::Channel(element) => Type::Task(element),
                                    _ => Type::Task(Box::new(Type::Int)),
                                },
                                _ => Type::Void,
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: if ret_ty == Type::Void {
                                    None
                                } else {
                                    Some(dest)
                                },
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return if ret_ty == Type::Void {
                                IrOperand::Null
                            } else {
                                IrOperand::Register(dest, ret_ty)
                            };
                        }
                        if class_name == "Task" || matches!(obj_op.get_type(), Type::Task(_)) {
                            let symbol = match member.as_str() {
                                "timeout" => "ube_task_timeout",
                                "all" => "ube_task_all_async",
                                "any" => "ube_task_any_async",
                                "race" => "ube_task_race_async",
                                "allSettled" => "ube_task_all_settled_async",
                                "cancel" => "ube_task_cancel",
                                _ => "ube_task_await",
                            };
                            let dest = self.fresh_reg();
                            let ret_ty = match member.as_str() {
                                "timeout" => match obj_op.get_type() {
                                    Type::Task(result) => Type::Task(result),
                                    _ => Type::Task(Box::new(Type::Int)),
                                },
                                "all" | "allSettled" => Type::Task(Box::new(Type::Class {
                                    name: "List".into(),
                                })),
                                "cancel" => Type::Bool,
                                _ => Type::Task(Box::new(Type::Int)),
                            };
                            let mut call_args = Vec::new();
                            if member == "cancel" || member == "timeout" {
                                call_args.push(obj_op);
                                if member == "timeout" {
                                    call_args.extend(arg_ops);
                                }
                            } else {
                                call_args.extend(arg_ops);
                            }
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "DateTime"
                            || member == "now"
                            || member == "date"
                            || member == "time"
                            || member == "of"
                            || member == "utc"
                            || member == "parse"
                            || member == "fromUnix"
                            || member == "fromUnixMillis"
                        {
                            let symbol = match member.as_str() {
                                "now" => "ube_datetime_now",
                                "utc" => "ube_datetime_utc",
                                "date" => "ube_datetime_date",
                                "time" => "ube_datetime_time",
                                "of" => "ube_datetime_of",
                                "parse" => "ube_datetime_parse",
                                "fromUnix" => "ube_datetime_from_unix",
                                "fromUnixMillis" => "ube_datetime_from_unix_millis",
                                "year" => "ube_datetime_year",
                                "month" => "ube_datetime_month",
                                "day" => "ube_datetime_day",
                                "hour" => "ube_datetime_hour",
                                "minute" => "ube_datetime_minute",
                                "second" => "ube_datetime_second",
                                "timestamp" => "ube_datetime_timestamp",
                                "unix" => "ube_datetime_unix",
                                "unixMillis" => "ube_datetime_unix_millis",
                                "unixNanos" => "ube_datetime_unix_nanos",
                                "zone" => "ube_datetime_zone",
                                "in" => "ube_datetime_in_zone",
                                "addDays" | "addHours" | "addMinutes" | "addDuration" => {
                                    "ube_datetime_add_seconds"
                                }
                                "diffSeconds" | "diffDays" => "ube_datetime_diff_seconds",
                                "format" => "ube_datetime_format",
                                _ => "ube_datetime_now",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = Vec::new();
                            if ![
                                "now",
                                "utc",
                                "date",
                                "time",
                                "of",
                                "parse",
                                "fromUnix",
                                "fromUnixMillis",
                            ]
                            .contains(&member.as_str())
                            {
                                call_args.push(obj_op);
                            }
                            for arg in arg_ops {
                                let arg = match member.as_str() {
                                    "addDays" => self.scale_integer_operand(arg, 86_400),
                                    "addHours" => self.scale_integer_operand(arg, 3_600),
                                    "addMinutes" => self.scale_integer_operand(arg, 60),
                                    "addDuration" => {
                                        let seconds = self.fresh_reg();
                                        self.emit(IrInstruction::CallRuntime {
                                            dest: Some(seconds),
                                            symbol: "ube_duration_total_seconds".into(),
                                            args: vec![arg],
                                            ret_ty: Type::Int,
                                        });
                                        IrOperand::Register(seconds, Type::Int)
                                    }
                                    _ => arg,
                                };
                                call_args.push(arg);
                            }
                            let ret_ty = match member.as_str() {
                                "zone" | "format" => Type::String,
                                "year" | "month" | "day" | "hour" | "minute" | "second"
                                | "timestamp" | "unix" | "unixMillis" | "unixNanos"
                                | "diffSeconds" | "diffDays" => Type::Int,
                                _ => Type::Class {
                                    name: "DateTime".into(),
                                },
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            if member == "diffDays" {
                                let days = self.fresh_reg();
                                self.emit(IrInstruction::Binary {
                                    dest: days,
                                    op: BinaryOp::Div,
                                    left: IrOperand::Register(dest, Type::Int),
                                    right: IrOperand::ConstantInt(86_400),
                                    operand_ty: Type::Int,
                                    result_ty: Type::Int,
                                });
                                return IrOperand::Register(days, Type::Int);
                            }
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "Duration"
                            || member == "seconds"
                            || member == "minutes"
                            || member == "hours"
                            || member == "days"
                        {
                            let symbol = match member.as_str() {
                                "seconds" | "minutes" | "hours" | "days" => "ube_duration_create",
                                "totalSeconds" => "ube_duration_total_seconds",
                                _ => "ube_duration_create",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = Vec::new();
                            if member == "totalSeconds" {
                                call_args.push(obj_op);
                            }
                            for arg in arg_ops {
                                let factor = match member.as_str() {
                                    "minutes" => 60,
                                    "hours" => 3_600,
                                    "days" => 86_400,
                                    _ => 1,
                                };
                                call_args.push(if factor == 1 {
                                    arg
                                } else {
                                    self.scale_integer_operand(arg, factor)
                                });
                            }
                            let ret_ty = if member == "totalSeconds" {
                                Type::Int
                            } else {
                                Type::Class {
                                    name: "Duration".into(),
                                }
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "Currency"
                            || member == "code"
                            || ["PHP", "USD", "EUR", "JPY", "GBP"].contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "code" => "ube_currency_code",
                                _ => "ube_currency_create",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = Vec::new();
                            if member == "code" {
                                call_args.push(obj_op);
                            }
                            call_args.extend(arg_ops);
                            let ret_ty = if member == "code" {
                                Type::String
                            } else {
                                Type::Class {
                                    name: "Currency".into(),
                                }
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "Money"
                            || [
                                "php", "usd", "eur", "jpy", "gbp", "amount", "currency", "add",
                                "subtract", "multiply",
                            ]
                            .contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "php" => "ube_money_php",
                                "usd" => "ube_money_usd",
                                "eur" => "ube_money_eur",
                                "jpy" => "ube_money_jpy",
                                "gbp" => "ube_money_gbp",
                                "amount" => "ube_money_amount",
                                "currency" => "ube_money_currency",
                                "add" => "ube_money_add",
                                "subtract" => "ube_money_subtract",
                                "multiply" => "ube_money_multiply_percent",
                                _ => "ube_money_create",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = Vec::new();
                            if ["amount", "currency", "add", "subtract", "multiply"]
                                .contains(&member.as_str())
                            {
                                call_args.push(obj_op);
                            }
                            call_args.extend(arg_ops);
                            let ret_ty = match member.as_str() {
                                "amount" => Type::Decimal,
                                "currency" => Type::Class {
                                    name: "Currency".into(),
                                },
                                _ => Type::Class {
                                    name: "Money".into(),
                                },
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "Exchange" || member == "convert" {
                            let dest = self.fresh_reg();
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: "ube_exchange_convert".into(),
                                args: arg_ops,
                                ret_ty: Type::Class {
                                    name: "Money".into(),
                                },
                            });
                            return IrOperand::Register(
                                dest,
                                Type::Class {
                                    name: "Money".into(),
                                },
                            );
                        }

                        if class_name == "Percent" || member == "of" || member == "value" {
                            let symbol = if member == "value" {
                                "ube_percent_value"
                            } else {
                                "ube_percent_create"
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = Vec::new();
                            if member == "value" {
                                call_args.push(obj_op);
                            }
                            call_args.extend(arg_ops);
                            let ret_ty = if member == "value" {
                                Type::Decimal
                            } else {
                                Type::Class {
                                    name: "Percent".into(),
                                }
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "Rate" || member == "percent" || member == "value" {
                            let symbol = if member == "value" {
                                "ube_rate_value"
                            } else {
                                "ube_rate_create"
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = Vec::new();
                            if member == "value" {
                                call_args.push(obj_op);
                            }
                            call_args.extend(arg_ops);
                            let ret_ty = if member == "value" {
                                Type::Decimal
                            } else {
                                Type::Class {
                                    name: "Rate".into(),
                                }
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "GeoPoint"
                            || ["latitude", "longitude", "distanceTo", "bearingTo"]
                                .contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "latitude" => "ube_geopoint_latitude",
                                "longitude" => "ube_geopoint_longitude",
                                "distanceTo" => "ube_geopoint_distance_to",
                                "bearingTo" => "ube_geopoint_bearing_to",
                                _ => "ube_geopoint_create",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = vec![obj_op];
                            call_args.extend(arg_ops);
                            let ret_ty = match member.as_str() {
                                "distanceTo" => Type::Class {
                                    name: "Distance".into(),
                                },
                                _ => Type::Decimal,
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "Distance"
                            || ["meters", "kilometers", "miles"].contains(&member.as_str())
                        {
                            let is_inst = obj_op.get_type()
                                == (Type::Class {
                                    name: "Distance".into(),
                                });
                            let symbol = if is_inst {
                                match member.as_str() {
                                    "meters" => "ube_distance_meters",
                                    "kilometers" => "ube_distance_km",
                                    "miles" => "ube_distance_miles",
                                    _ => "ube_distance_meters",
                                }
                            } else {
                                match member.as_str() {
                                    "meters" => "ube_distance_create_meters",
                                    "kilometers" => "ube_distance_create_km",
                                    "miles" => "ube_distance_create_miles",
                                    _ => "ube_distance_create_meters",
                                }
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = Vec::new();
                            if is_inst {
                                call_args.push(obj_op);
                            }
                            call_args.extend(arg_ops);
                            let ret_ty = if is_inst {
                                Type::Decimal
                            } else {
                                Type::Class {
                                    name: "Distance".into(),
                                }
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "GeoBounds"
                            || ["southwest", "northeast", "contains"].contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "contains" => "ube_geobounds_contains",
                                _ => "ube_geobounds_create",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = vec![obj_op];
                            call_args.extend(arg_ops);
                            let ret_ty = if member == "contains" {
                                Type::Bool
                            } else {
                                Type::Class {
                                    name: "GeoPoint".into(),
                                }
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "Geolocation" || member == "current" {
                            let dest = self.fresh_reg();
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: "ube_geolocation_current".into(),
                                args: arg_ops,
                                ret_ty: Type::Class {
                                    name: "GeoPoint".into(),
                                },
                            });
                            return IrOperand::Register(
                                dest,
                                Type::Class {
                                    name: "GeoPoint".into(),
                                },
                            );
                        }

                        if class_name == "Math"
                            || [
                                "PI",
                                "E",
                                "TAU",
                                "abs",
                                "min",
                                "max",
                                "sqrt",
                                "cbrt",
                                "pow",
                                "floor",
                                "ceil",
                                "round",
                                "truncate",
                                "clamp",
                                "sign",
                                "sin",
                                "cos",
                                "tan",
                                "asin",
                                "acos",
                                "atan",
                                "atan2",
                                "radians",
                                "degrees",
                                "log",
                                "log10",
                                "log2",
                                "exp",
                                "isNaN",
                                "isInfinite",
                                "isFinite",
                                "isEven",
                                "isOdd",
                                "gcd",
                                "lcm",
                            ]
                            .contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "PI" => "ube_math_pi",
                                "E" => "ube_math_e",
                                "TAU" => "ube_math_tau",
                                "abs" => "ube_math_abs",
                                "min" => "ube_math_min",
                                "max" => "ube_math_max",
                                "sqrt" => "ube_math_sqrt",
                                "cbrt" => "ube_math_cbrt",
                                "pow" => "ube_math_pow",
                                "floor" => "ube_math_floor",
                                "ceil" => "ube_math_ceil",
                                "round" => {
                                    if arg_ops.len() > 1 {
                                        "ube_math_round_decimals"
                                    } else {
                                        "ube_math_round"
                                    }
                                }
                                "truncate" => "ube_math_truncate",
                                "clamp" => "ube_math_clamp",
                                "sign" => "ube_math_sign",
                                "sin" => "ube_math_sin",
                                "cos" => "ube_math_cos",
                                "tan" => "ube_math_tan",
                                "asin" => "ube_math_asin",
                                "acos" => "ube_math_acos",
                                "atan" => "ube_math_atan",
                                "atan2" => "ube_math_atan2",
                                "radians" => "ube_math_radians",
                                "degrees" => "ube_math_degrees",
                                "log" => "ube_math_log",
                                "log10" => "ube_math_log10",
                                "log2" => "ube_math_log2",
                                "exp" => "ube_math_exp",
                                "isNaN" => "ube_math_is_nan",
                                "isInfinite" => "ube_math_is_infinite",
                                "isFinite" => "ube_math_is_finite",
                                "isEven" => "ube_math_is_even",
                                "isOdd" => "ube_math_is_odd",
                                "gcd" => "ube_math_gcd",
                                "lcm" => "ube_math_lcm",
                                _ => "ube_math_pi",
                            };
                            let dest = self.fresh_reg();
                            let ret_ty = match member.as_str() {
                                "sign" | "gcd" | "lcm" => Type::Int,
                                "isNaN" | "isInfinite" | "isFinite" | "isEven" | "isOdd" => {
                                    Type::Bool
                                }
                                _ => Type::Decimal,
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: arg_ops,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "Random"
                            || ["int", "decimal", "bool", "uuid", "bytes"]
                                .contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "int" => "ube_random_int",
                                "decimal" => "ube_random_decimal",
                                "bool" => "ube_random_bool",
                                "uuid" => "ube_random_uuid",
                                "bytes" => "ube_random_bytes",
                                _ => "ube_random_int",
                            };
                            let dest = self.fresh_reg();
                            let ret_ty = match member.as_str() {
                                "int" => Type::Int,
                                "decimal" => Type::Decimal,
                                "bool" => Type::Bool,
                                "uuid" => Type::String,
                                _ => Type::Class {
                                    name: "List".into(),
                                },
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: arg_ops,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "SecureRandom" || member == "bytes" {
                            let dest = self.fresh_reg();
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: "ube_securerandom_bytes".into(),
                                args: arg_ops,
                                ret_ty: Type::Class {
                                    name: "List".into(),
                                },
                            });
                            return IrOperand::Register(
                                dest,
                                Type::Class {
                                    name: "List".into(),
                                },
                            );
                        }

                        if class_name == "Statistics"
                            || [
                                "mean",
                                "median",
                                "variance",
                                "standardDeviation",
                                "percentile",
                                "covariance",
                                "correlation",
                            ]
                            .contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "mean" => "ube_statistics_mean",
                                "median" => "ube_statistics_median",
                                "variance" => "ube_statistics_variance",
                                "standardDeviation" => "ube_statistics_std_dev",
                                "percentile" => "ube_statistics_percentile",
                                "covariance" => "ube_statistics_covariance",
                                "correlation" => "ube_statistics_correlation",
                                _ => "ube_statistics_mean",
                            };
                            let dest = self.fresh_reg();
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: arg_ops,
                                ret_ty: Type::Decimal,
                            });
                            return IrOperand::Register(dest, Type::Decimal);
                        }

                        if class_name == "Probability"
                            || [
                                "factorial",
                                "permutations",
                                "combinations",
                                "normal",
                                "binomial",
                                "poisson",
                                "uniform",
                            ]
                            .contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "factorial" => "ube_probability_factorial",
                                "permutations" => "ube_probability_permutations",
                                "combinations" => "ube_probability_combinations",
                                "normal" => "ube_probability_normal",
                                "binomial" => "ube_probability_binomial",
                                "poisson" => "ube_probability_poisson",
                                "uniform" => "ube_probability_uniform",
                                _ => "ube_probability_factorial",
                            };
                            let dest = self.fresh_reg();
                            let ret_ty = match member.as_str() {
                                "factorial" | "permutations" | "combinations" => Type::Int,
                                _ => Type::Decimal,
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: arg_ops,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if crate::builtins::SQLITE_CLASSES.contains(&class_name.as_str()) {
                            if let Some(method) = crate::builtins::sqlite_methods(&class_name).into_iter().find(|m| m.name == member) {
                                let mut args = Vec::new();
                                if !method.is_static { args.push(obj_op); }
                                args.extend(arg_ops.into_iter().zip(&method.params).map(|(arg, ty)| self.cast(arg, ty.clone())));
                                let dest = self.fresh_reg();
                                self.emit(IrInstruction::CallRuntime {
                                    dest: if method.result == Type::Void { None } else { Some(dest) },
                                    symbol: method.symbol.into(), args, ret_ty: method.result.clone(),
                                });
                                return if method.result == Type::Void { IrOperand::Null } else { IrOperand::Register(dest, method.result) };
                            }
                        }

                        if class_name == "File" {
                            let is_inst = obj_op.get_type()
                                == (Type::Class {
                                    name: "File".into(),
                                });
                            let symbol = match member.as_str() {
                                "open" => "ube_file_open",
                                "read" => "ube_file_read",
                                "write" => "ube_file_write",
                                "append" => "ube_file_append",
                                "delete" => "ube_file_delete",
                                "exists" => "ube_file_exists",
                                "info" => "ube_file_info",
                                "close" => "ube_file_close",
                                _ => "ube_file_read",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = Vec::new();
                            if is_inst {
                                call_args.push(obj_op);
                            }
                            call_args.extend(arg_ops);
                            let ret_ty = match member.as_str() {
                                "open" => Type::Class {
                                    name: "File".into(),
                                },
                                "read" => Type::String,
                                "exists" => Type::Bool,
                                "info" => Type::Class {
                                    name: "FileInfo".into(),
                                },
                                _ => Type::Void,
                            };
                            let dest_opt = if ret_ty == Type::Void {
                                None
                            } else {
                                Some(dest)
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: dest_opt,
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return if ret_ty == Type::Void {
                                IrOperand::Null
                            } else {
                                IrOperand::Register(dest, ret_ty)
                            };
                        }

                        if class_name == "FileInfo"
                            || ["size", "createdAt", "modifiedAt", "isDirectory"]
                                .contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "size" => "ube_fileinfo_size",
                                "createdAt" => "ube_fileinfo_created_at",
                                "modifiedAt" => "ube_fileinfo_modified_at",
                                "isDirectory" => "ube_fileinfo_is_directory",
                                _ => "ube_fileinfo_size",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = vec![obj_op];
                            call_args.extend(arg_ops);
                            let ret_ty = match member.as_str() {
                                "size" => Type::Long,
                                "createdAt" | "modifiedAt" => Type::Class {
                                    name: "DateTime".into(),
                                },
                                "isDirectory" => Type::Bool,
                                _ => Type::Long,
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "Directory"
                            || ["create", "delete", "list"].contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "create" => "ube_directory_create",
                                "delete" => "ube_directory_delete",
                                "list" => "ube_directory_list",
                                _ => "ube_directory_create",
                            };
                            let dest = self.fresh_reg();
                            let ret_ty = if member == "list" {
                                Type::Class {
                                    name: "List".into(),
                                }
                            } else {
                                Type::Void
                            };
                            let dest_opt = if ret_ty == Type::Void {
                                None
                            } else {
                                Some(dest)
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: dest_opt,
                                symbol: symbol.into(),
                                args: arg_ops,
                                ret_ty: ret_ty.clone(),
                            });
                            return if ret_ty == Type::Void {
                                IrOperand::Null
                            } else {
                                IrOperand::Register(dest, ret_ty)
                            };
                        }

                        if class_name == "Path"
                            || ["join", "filename", "extension"].contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "join" => "ube_path_join",
                                "filename" => "ube_path_filename",
                                "extension" => "ube_path_extension",
                                _ => "ube_path_join",
                            };
                            let dest = self.fresh_reg();
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: arg_ops,
                                ret_ty: Type::String,
                            });
                            return IrOperand::Register(dest, Type::String);
                        }

                        if class_name == "Server"
                            || [
                                "get",
                                "post",
                                "put",
                                "patch",
                                "delete",
                                "options",
                                "head",
                                "use",
                                "static",
                                "websocket",
                                "start",
                                "stop",
                                "port",
                                "tls",
                                "enableHttp2",
                                "enableHttp3",
                                "http3Port",
                            ]
                            .contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "start" => "ube_server_start",
                                "stop" => "ube_server_stop",
                                "port" => "ube_server_port",
                                "tls" => "ube_server_tls",
                                "use" => "ube_server_use",
                                "enableHttp2" => "ube_server_enable_http2",
                                "enableHttp3" => "ube_server_enable_http3",
                                "http3Port" => "ube_server_http3_port",
                                _ => "ube_server_route",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = vec![obj_op];
                            call_args.extend(arg_ops);
                            let ret_ty = if member == "port" || member == "http3Port" {
                                Type::Int
                            } else {
                                Type::Void
                            };
                            let dest_opt = if ret_ty == Type::Void {
                                None
                            } else {
                                Some(dest)
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: dest_opt,
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return if ret_ty == Type::Void {
                                IrOperand::Null
                            } else {
                                IrOperand::Register(dest, ret_ty)
                            };
                        }

                        if class_name == "Request" && member == "session" {
                            return self.runtime("ube_request_session", vec![obj_op], Type::Class { name: "Session".into() });
                        }
                        if class_name == "Request"
                            || [
                                "path",
                                "pathLong",
                                "pathInt",
                                "query",
                                "queryDecimal",
                                "header",
                                "cookie",
                                "hasBearerToken",
                                "body",
                                "method",
                                "url",
                                "ip",
                                "protocol",
                                "isHttp3",
                            ]
                            .contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "pathLong" => "ube_request_path_long",
                                "pathInt" => "ube_request_path_int",
                                "query" => "ube_request_query",
                                "header" => "ube_request_header",
                                "hasBearerToken" => "ube_request_has_bearer_token",
                                "protocol" => "ube_request_protocol",
                                "isHttp3" => "ube_request_is_http3",
                                _ => "ube_request_path",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = vec![obj_op];
                            call_args.extend(arg_ops);
                            let ret_ty = match member.as_str() {
                                "pathLong" => Type::Long,
                                "pathInt" => Type::Int,
                                "hasBearerToken" | "isHttp3" => Type::Bool,
                                _ => Type::String,
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "Response"
                            || [
                                "ok",
                                "json",
                                "html",
                                "text",
                                "created",
                                "notFound",
                                "internalServerError",
                                "unauthorized",
                                "redirect",
                                "status",
                            ]
                            .contains(&member.as_str())
                        {
                            if member == "redirect" {
                                return self.runtime("ube_response_redirect", arg_ops, Type::Class { name: "Response".into() });
                            }
                            let status_code = match member.as_str() {
                                "created" => 201,
                                "notFound" => 404,
                                "unauthorized" => 401,
                                "internalServerError" => 500,
                                _ => 200,
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = vec![IrOperand::ConstantInt(status_code)];
                            call_args.extend(arg_ops);
                            let ret_ty = Type::Class {
                                name: "Response".into(),
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: "ube_response_create".into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "WebSocket"
                            || ["onMessage", "send", "close"].contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "onMessage" => "ube_websocket_on_message",
                                "send" => "ube_websocket_send",
                                _ => "ube_websocket_send",
                            };
                            let mut call_args = vec![obj_op];
                            call_args.extend(arg_ops);
                            self.emit(IrInstruction::CallRuntime {
                                dest: None,
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: Type::Void,
                            });
                            return IrOperand::Null;
                        }

                        if class_name == "Json"
                            || ["parse", "partialParse", "stringify", "from", "to"]
                                .contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "parse" | "partialParse" => "ube_json_parse",
                                "stringify" | "from" => "ube_json_stringify",
                                _ => "ube_json_parse",
                            };
                            let dest = self.fresh_reg();
                            let ret_ty = if member == "stringify" || member == "from" {
                                Type::String
                            } else {
                                Type::Class {
                                    name: "JsonObject".into(),
                                }
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: arg_ops,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "JsonObject"
                            || [
                                "getString",
                                "getInt",
                                "getLong",
                                "getDecimal",
                                "getBool",
                                "getObject",
                                "getArray",
                            ]
                            .contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "getString" => "ube_jsonobject_get_string",
                                "getInt" => "ube_jsonobject_get_int",
                                "getLong" => "ube_jsonobject_get_long",
                                "getDecimal" => "ube_jsonobject_get_decimal",
                                "getBool" => "ube_jsonobject_get_bool",
                                _ => "ube_jsonobject_get_string",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = vec![obj_op];
                            call_args.extend(arg_ops);
                            let ret_ty = match member.as_str() {
                                "getInt" => Type::Int,
                                "getLong" => Type::Long,
                                "getDecimal" => Type::Decimal,
                                "getBool" => Type::Bool,
                                "getObject" => Type::Class {
                                    name: "JsonObject".into(),
                                },
                                "getArray" => Type::Class {
                                    name: "JsonArray".into(),
                                },
                                _ => Type::String,
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "Xml"
                            || ["parse", "partialParse", "create", "stringify", "from", "to"]
                                .contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "parse" | "partialParse" => "ube_xml_parse",
                                "create" => "ube_xml_create",
                                _ => "ube_xml_parse",
                            };
                            let dest = self.fresh_reg();
                            let ret_ty = Type::Class {
                                name: "XmlDocument".into(),
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: arg_ops,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "XmlDocument"
                            || ["root", "toString"].contains(&member.as_str())
                        {
                            let symbol = match member.as_str() {
                                "root" => "ube_xmldocument_root",
                                "toString" => "ube_xmldocument_to_string",
                                _ => "ube_xmldocument_root",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = vec![obj_op];
                            call_args.extend(arg_ops);
                            let ret_ty = if member == "toString" {
                                Type::String
                            } else {
                                Type::Class {
                                    name: "XmlElement".into(),
                                }
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        if class_name == "XmlElement"
                            || ["child", "element", "text", "getText"].contains(&member.as_str())
                        {
                            let is_text_getter = member == "text" && arg_ops.is_empty();
                            let symbol = match member.as_str() {
                                "child" => "ube_xmlement_child",
                                "element" => "ube_xmlement_element",
                                "text" => {
                                    if is_text_getter {
                                        "ube_xmlement_get_text"
                                    } else {
                                        "ube_xmlement_text"
                                    }
                                }
                                "getText" => "ube_xmlement_get_text",
                                _ => "ube_xmlement_child",
                            };
                            let dest = self.fresh_reg();
                            let mut call_args = vec![obj_op];
                            call_args.extend(arg_ops);
                            let ret_ty = if is_text_getter || member == "getText" {
                                Type::String
                            } else {
                                Type::Class {
                                    name: "XmlElement".into(),
                                }
                            };
                            self.emit(IrInstruction::CallRuntime {
                                dest: Some(dest),
                                symbol: symbol.into(),
                                args: call_args,
                                ret_ty: ret_ty.clone(),
                            });
                            return IrOperand::Register(dest, ret_ty);
                        }

                        let mut target_method = member.clone();

                        if member == "getHeader" {
                            class_name = "Printable".into();
                            target_method = "getHeader".into();
                        } else if member == "render" && !arg_ops.is_empty() {
                            if let Type::Class { name: arg_cls } = arg_ops[0].get_type() {
                                target_method = format!("{}_{}", member, arg_cls);
                            }
                        }

                        let ret_ty = self
                            .method_return_types
                            .get(&(class_name.clone(), target_method.clone()))
                            .cloned()
                            .or_else(|| {
                                self.method_return_types
                                    .iter()
                                    .find(|((_, m), _)| m == &target_method || m == member)
                                    .map(|(_, ty)| ty.clone())
                            })
                            .unwrap_or(Type::Void);

                        let dest_opt = if ret_ty == Type::Void {
                            None
                        } else {
                            Some(dest)
                        };
                        self.emit(IrInstruction::CallMethod {
                            dest: dest_opt,
                            class_name,
                            method_name: target_method,
                            receiver: obj_op,
                            args: arg_ops,
                            ret_ty: ret_ty.clone(),
                        });
                        if ret_ty == Type::Void {
                            IrOperand::Null
                        } else {
                            IrOperand::Register(dest, ret_ty)
                        }
                    }
                    ExprKind::Identifier(class_name) => {
                        let symbol = match class_name.as_str() {
                            "Money" => "ube_money_create",
                            "Percent" => "ube_percent_create",
                            "Rate" => "ube_rate_create",
                            "GeoPoint" => "ube_geopoint_create",
                            "GeoBounds" => "ube_geobounds_create",
                            "Distance" => "ube_distance_create_meters",
                            "Server" => "ube_server_create",
                            "Cookie" => "ube_cookie_create",
                            "LocationPermissionError"
                            | "LocationUnavailableError"
                            | "FileError"
                            | "FileNotFoundError"
                            | "PermissionError"
                            | "AlreadyExistsError"
                            | "InvalidPathError"
                            | "DiskFullError"
                            | "JsonError"
                            | "JsonParseError"
                            | "MissingFieldError"
                            | "InvalidTypeError"
                            | "XmlError"
                            | "XmlParseError" => "ube_generic_error_create",
                            _ => "ube_money_create",
                        };
                        let dest = self.fresh_reg();
                        let ret_ty = Type::Class {
                            name: class_name.clone(),
                        };
                        self.emit(IrInstruction::CallRuntime {
                            dest: Some(dest),
                            symbol: symbol.into(),
                            args: arg_ops,
                            ret_ty: ret_ty.clone(),
                        });
                        IrOperand::Register(dest, ret_ty)
                    }
                    _ => {
                        self.diagnostics.push(Diagnostic::error(
                            "direct function calls are not supported",
                            Some(expr.span),
                        ));
                        IrOperand::Null
                    }
                }
            }
            ExprKind::MemberAccess {
                object,
                member,
                is_optional,
            } => {
                if *is_optional {
                    self.diagnostics.push(Diagnostic::error(
                        "optional member access '?.' is not yet supported by native code generation",
                        Some(expr.span),
                    ));
                }
                let static_class_name = match &object.kind {
                    ExprKind::Identifier(name) if self.lookup_var(name).is_none() => Some(
                        canonical_builtin_class_name(name)
                            .unwrap_or(name)
                            .to_string(),
                    ),
                    _ => None,
                };
                if static_class_name.as_deref() == Some("Math")
                    && matches!(member.as_str(), "PI" | "E" | "TAU")
                {
                    let symbol = match member.as_str() {
                        "PI" => "ube_math_pi",
                        "E" => "ube_math_e",
                        _ => "ube_math_tau",
                    };
                    let dest = self.fresh_reg();
                    self.emit(IrInstruction::CallRuntime {
                        dest: Some(dest),
                        symbol: symbol.into(),
                        args: vec![],
                        ret_ty: Type::Decimal,
                    });
                    return IrOperand::Register(dest, Type::Decimal);
                }
                let obj_op = if static_class_name.is_some() {
                    IrOperand::Null
                } else {
                    self.build_expr(object)
                };
                let dest = self.fresh_reg();
                let class_name = static_class_name.unwrap_or_else(|| match obj_op.get_type() {
                    Type::Class { name } => name,
                    _ => "".into(),
                });
                if class_name == "Math" && (member == "PI" || member == "E" || member == "TAU") {
                    let symbol = match member.as_str() {
                        "PI" => "ube_math_pi",
                        "E" => "ube_math_e",
                        "TAU" => "ube_math_tau",
                        _ => "ube_math_pi",
                    };
                    self.emit(IrInstruction::CallRuntime {
                        dest: Some(dest),
                        symbol: symbol.into(),
                        args: vec![],
                        ret_ty: Type::Decimal,
                    });
                    return IrOperand::Register(dest, Type::Decimal);
                }
                if let Some((idx, field_ty)) = self
                    .class_field_indices
                    .get(&(class_name.clone(), member.clone()))
                    .cloned()
                {
                    if let IrOperand::Register(ptr, _) = obj_op {
                        self.emit(IrInstruction::GetField {
                            dest,
                            ptr,
                            class_name,
                            field_index: idx,
                            field_ty: field_ty.clone(),
                        });
                        IrOperand::Register(dest, field_ty)
                    } else {
                        self.diagnostics.push(Diagnostic::error(
                            format!("cannot lower member access '{}'", member),
                            Some(expr.span),
                        ));
                        IrOperand::Null
                    }
                } else {
                    self.diagnostics.push(Diagnostic::error(
                        format!("cannot lower member access '{}'", member),
                        Some(expr.span),
                    ));
                    IrOperand::Null
                }
            }
            ExprKind::Assign { target, value } => {
                let val_op = self.build_expr(value);
                match &target.kind {
                    ExprKind::MemberAccess { object, member, .. } => {
                        let obj_op = self.build_expr(object);
                        let class_name = match obj_op.get_type() {
                            Type::Class { name } => name,
                            _ => "".into(),
                        };
                        let field_idx_opt = self
                            .class_field_indices
                            .get(&(class_name.clone(), member.clone()))
                            .map(|(i, _)| *i);
                        if let Some(idx) = field_idx_opt {
                            if let IrOperand::Register(ptr, _) = obj_op {
                                self.emit(IrInstruction::StoreField {
                                    ptr,
                                    class_name,
                                    field_index: idx,
                                    val: val_op.clone(),
                                });
                            }
                        }
                    }
                    ExprKind::Identifier(name) => {
                        if let Some((ptr, _)) = self.lookup_var(name) {
                            self.emit(IrInstruction::Store {
                                val: val_op.clone(),
                                ptr,
                            });
                        }
                    }
                    _ => {
                        self.diagnostics.push(Diagnostic::error(
                            "left side of assignment is not assignable",
                            Some(expr.span),
                        ));
                    }
                }
                val_op
            }
            ExprKind::Binary { op, left, right } => {
                if op == &BinaryOp::Range {
                    self.diagnostics.push(Diagnostic::error(
                        "range expressions are not yet supported by native code generation",
                        Some(expr.span),
                    ));
                    return IrOperand::ConstantInt(0);
                }
                let l_op = self.build_expr(left);
                if op == &BinaryOp::NullCoalescing {
                    let Type::Nullable(inner) = l_op.get_type() else { return l_op; };
                    let ty = *inner;
                    let slot = self.fresh_reg();
                    self.emit(IrInstruction::Alloca { dest: slot, ty: ty.clone() });
                    let condition = self.fresh_reg();
                    self.emit(IrInstruction::Binary { dest: condition, op: BinaryOp::NotEqual, left: l_op.clone(), right: IrOperand::NullOf(l_op.get_type()), operand_ty: l_op.get_type(), result_ty: Type::Bool });
                    let present = self.fresh_block(); let absent = self.fresh_block(); let merge = self.fresh_block();
                    self.emit(IrInstruction::Branch { cond: IrOperand::Register(condition, Type::Bool), then_block: present, else_block: absent }); self.end_block();
                    self.current_block_id = Some(present);
                    let value = self.cast(l_op, ty.clone()); self.emit(IrInstruction::Store { val: value, ptr: slot });
                    self.emit(IrInstruction::Jump { target: merge }); self.end_block();
                    self.current_block_id = Some(absent);
                    let value = self.build_expr(right); let value = self.cast(value, ty.clone());
                    self.emit(IrInstruction::Store { val: value, ptr: slot });
                    self.emit(IrInstruction::Jump { target: merge }); self.end_block();
                    self.current_block_id = Some(merge);
                    let result = self.fresh_reg(); self.emit(IrInstruction::Load { dest: result, ptr: slot, ty: ty.clone() });
                    return IrOperand::Register(result, ty);
                }
                let r_op = self.build_expr(right);
                let dest = self.fresh_reg();
                let operand_ty = l_op.get_type();

                let res_ty = match op {
                    BinaryOp::Equal
                    | BinaryOp::NotEqual
                    | BinaryOp::Less
                    | BinaryOp::LessEqual
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEqual
                    | BinaryOp::And
                    | BinaryOp::Or => Type::Bool,
                    _ => operand_ty.clone(),
                };

                self.emit(IrInstruction::Binary {
                    dest,
                    op: op.clone(),
                    left: l_op,
                    right: r_op,
                    operand_ty,
                    result_ty: res_ty.clone(),
                });
                IrOperand::Register(dest, res_ty)
            }
            ExprKind::Unary { op, operand } => {
                let op_val = self.build_expr(operand);
                let op_val = if matches!(op, UnaryOp::Not) { self.condition_to_bool(op_val) } else { op_val };
                let dest = self.fresh_reg();
                let ty = op_val.get_type();
                match op {
                    UnaryOp::Not => {
                        self.emit(IrInstruction::Binary {
                            dest,
                            op: BinaryOp::Equal,
                            left: op_val,
                            right: IrOperand::ConstantBool(false),
                            operand_ty: Type::Bool,
                            result_ty: Type::Bool,
                        });
                        IrOperand::Register(dest, Type::Bool)
                    }
                    _ => {
                        self.emit(IrInstruction::Binary {
                            dest,
                            op: BinaryOp::Sub,
                            left: IrOperand::ConstantInt(0),
                            right: op_val,
                            operand_ty: ty.clone(),
                            result_ty: ty.clone(),
                        });
                        IrOperand::Register(dest, ty)
                    }
                }
            }
            ExprKind::ListLiteral(elements) => {
                let list_reg = self.fresh_reg();
                self.emit(IrInstruction::CallRuntime {
                    dest: Some(list_reg),
                    symbol: "ube_list_create".into(),
                    args: vec![],
                    ret_ty: Type::Class {
                        name: "List".into(),
                    },
                });
                let list_op = IrOperand::Register(
                    list_reg,
                    Type::Class {
                        name: "List".into(),
                    },
                );
                for elem in elements {
                    let elem_op = self.build_expr(elem);
                    self.emit(IrInstruction::CallRuntime {
                        dest: None,
                        symbol: "ube_list_add".into(),
                        args: vec![list_op.clone(), elem_op],
                        ret_ty: Type::Void,
                    });
                }
                list_op
            }
            ExprKind::MapLiteral(pairs) => {
                let map_reg = self.fresh_reg();
                self.emit(IrInstruction::CallRuntime {
                    dest: Some(map_reg),
                    symbol: "ube_map_create".into(),
                    args: vec![],
                    ret_ty: Type::Class { name: "Map".into() },
                });
                let map_op = IrOperand::Register(map_reg, Type::Class { name: "Map".into() });
                for (k, v) in pairs {
                    let k_op = self.build_expr(k);
                    let v_op = self.build_expr(v);
                    self.emit(IrInstruction::CallRuntime {
                        dest: None,
                        symbol: "ube_map_put".into(),
                        args: vec![map_op.clone(), k_op, v_op],
                        ret_ty: Type::Void,
                    });
                }
                map_op
            }
            ExprKind::IndexAccess { target, index } => {
                let target_op = self.build_expr(target);
                let index_op = self.build_expr(index);
                if let Type::Class { ref name } = target_op.get_type() {
                    if name == "Map" {
                        let dest = self.fresh_reg();
                        self.emit(IrInstruction::CallRuntime {
                            dest: Some(dest),
                            symbol: "ube_map_get".into(),
                            args: vec![target_op, index_op],
                            ret_ty: Type::Int,
                        });
                        return IrOperand::Register(dest, Type::Int);
                    }
                }
                let len_reg = self.fresh_reg();
                self.emit(IrInstruction::CallRuntime {
                    dest: Some(len_reg),
                    symbol: "ube_list_len".into(),
                    args: vec![target_op.clone()],
                    ret_ty: Type::Int,
                });
                let len_op = IrOperand::Register(len_reg, Type::Int);

                let is_past_end = self.fresh_reg();
                self.emit(IrInstruction::Binary {
                    dest: is_past_end,
                    op: BinaryOp::GreaterEqual,
                    left: index_op.clone(),
                    right: len_op,
                    operand_ty: Type::Int,
                    result_ty: Type::Bool,
                });
                let is_negative = self.fresh_reg();
                self.emit(IrInstruction::Binary {
                    dest: is_negative,
                    op: BinaryOp::Less,
                    left: index_op.clone(),
                    right: IrOperand::ConstantInt(0),
                    operand_ty: Type::Int,
                    result_ty: Type::Bool,
                });
                let is_out_of_bounds = self.fresh_reg();
                self.emit(IrInstruction::Binary {
                    dest: is_out_of_bounds,
                    op: BinaryOp::Or,
                    left: IrOperand::Register(is_past_end, Type::Bool),
                    right: IrOperand::Register(is_negative, Type::Bool),
                    operand_ty: Type::Bool,
                    result_ty: Type::Bool,
                });

                let check_target_opt = self.exception_handlers.last().cloned();
                if let Some((err_ptr, check_target)) = check_target_opt {
                    let oob_block = self.fresh_block();
                    let ok_block = self.fresh_block();

                    self.emit(IrInstruction::Branch {
                        cond: IrOperand::Register(is_out_of_bounds, Type::Bool),
                        then_block: oob_block,
                        else_block: ok_block,
                    });
                    if let Some(cb) = self.current_block_id {
                        self.finish_block(cb);
                    }

                    self.current_block_id = Some(oob_block);
                    let err_inst = self.build_class_instantiation(
                        "OutOfBoundsError",
                        &[Expr {
                            kind: ExprKind::StringLiteral(
                                "OutOfBoundsError: List index out of bounds".into(),
                            ),
                            span: expr.span,
                        }],
                    );
                    let record = self.make_exception(err_inst, expr.span);
                    self.emit(IrInstruction::Store { val: record, ptr: err_ptr });
                    self.emit(IrInstruction::Jump {
                        target: check_target,
                    });
                    // Creating the exception calls message(), whose exception
                    // check can move emission to a new block.
                    self.end_block();

                    self.current_block_id = Some(ok_block);
                }

                // Allocate after bounds-check instructions so LLVM's numbered
                // register definitions remain in ascending order.
                let dest = self.fresh_reg();
                self.emit(IrInstruction::CallRuntime {
                    dest: Some(dest),
                    symbol: "ube_list_get".into(),
                    args: vec![target_op, index_op],
                    ret_ty: Type::Int,
                });
                IrOperand::Register(dest, Type::Int)
            }
            ExprKind::AsyncSpawn(inner) => self.build_deferred_expression(inner),
            ExprKind::Await(inner) => {
                let task_op = self.build_expr(inner);
                let dest = self.fresh_reg();
                let error_dest = self.fresh_reg();
                let result_ty = match task_op.get_type() {
                    Type::Task(result) => *result,
                    _ => Type::Int,
                };
                self.emit(IrInstruction::AwaitTask {
                    dest,
                    error_dest,
                    task: task_op,
                    result_ty: result_ty.clone(),
                });
                self.route_await_error(error_dest);
                IrOperand::Register(dest, result_ty)
            }
            ExprKind::Lambda { .. } => {
                self.diagnostics.push(Diagnostic::error(
                    "Lambda expressions are currently supported only as server route handlers",
                    Some(expr.span),
                ));
                IrOperand::Null
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let res_ty = self
                    .yield_type_in_stmts(then_branch)
                    .or_else(|| {
                        else_branch
                            .as_ref()
                            .and_then(|stmts| self.yield_type_in_stmts(stmts))
                    })
                    .unwrap_or(Type::Int);
                let res_ptr = self.fresh_reg();
                self.emit(IrInstruction::Alloca {
                    dest: res_ptr,
                    ty: res_ty.clone(),
                });
                self.emit(IrInstruction::Store {
                    val: Self::default_operand_for_type(&res_ty),
                    ptr: res_ptr,
                });

                let then_block = self.fresh_block();
                let else_block = self.fresh_block();
                let merge_block = self.fresh_block();

                let condition = self.build_expr(condition);
                let cond_op = self.condition_to_bool(condition);
                self.emit(IrInstruction::Branch {
                    cond: cond_op,
                    then_block,
                    else_block,
                });
                if let Some(cb) = self.current_block_id {
                    self.finish_block(cb);
                }

                // Then block
                self.current_block_id = Some(then_block);
                self.yield_target_stack.push((Some(res_ptr), None));
                self.push_var_scope();
                for s in then_branch {
                    self.build_stmt(s);
                    if self.is_current_terminated() {
                        break;
                    }
                }
                if !self.is_current_terminated() {
                    self.emit(IrInstruction::Jump {
                        target: merge_block,
                    });
                }
                if let Some(cb) = self.current_block_id {
                    self.finish_block(cb);
                }
                self.pop_var_scope();
                self.yield_target_stack.pop();

                // Else block
                self.current_block_id = Some(else_block);
                self.yield_target_stack.push((Some(res_ptr), None));
                self.push_var_scope();
                if let Some(eb) = else_branch {
                    for s in eb {
                        self.build_stmt(s);
                        if self.is_current_terminated() {
                            break;
                        }
                    }
                }
                if !self.is_current_terminated() {
                    self.emit(IrInstruction::Jump {
                        target: merge_block,
                    });
                }
                if let Some(cb) = self.current_block_id {
                    self.finish_block(cb);
                }
                self.pop_var_scope();
                self.yield_target_stack.pop();

                self.current_block_id = Some(merge_block);
                let res_reg = self.fresh_reg();
                self.emit(IrInstruction::Load {
                    dest: res_reg,
                    ptr: res_ptr,
                    ty: res_ty.clone(),
                });
                IrOperand::Register(res_reg, res_ty)
            }
            ExprKind::For {
                loop_var,
                index_var,
                condition_or_iterable,
                body,
                else_branch,
            } => {
                let list_ptr = self.fresh_reg();
                let list_ty = Type::Class {
                    name: "List".into(),
                };
                self.emit(IrInstruction::Alloca {
                    dest: list_ptr,
                    ty: list_ty.clone(),
                });

                let list_reg = self.fresh_reg();
                self.emit(IrInstruction::CallRuntime {
                    dest: Some(list_reg),
                    symbol: "ube_list_create".into(),
                    args: vec![],
                    ret_ty: list_ty.clone(),
                });
                let empty_list = IrOperand::Register(list_reg, list_ty.clone());
                self.emit(IrInstruction::Store {
                    val: empty_list,
                    ptr: list_ptr,
                });

                self.yield_target_stack.push((None, Some(list_ptr)));
                let stmt_for = Stmt {
                    kind: StmtKind::For {
                        loop_var: loop_var.clone(),
                        index_var: index_var.clone(),
                        condition_or_iterable: condition_or_iterable.as_ref().map(|b| *b.clone()),
                        body: body.clone(),
                        else_branch: else_branch.clone(),
                    },
                    span: expr.span,
                };
                self.build_stmt(&stmt_for);
                self.yield_target_stack.pop();

                let res_reg = self.fresh_reg();
                self.emit(IrInstruction::Load {
                    dest: res_reg,
                    ptr: list_ptr,
                    ty: list_ty.clone(),
                });
                IrOperand::Register(res_reg, list_ty)
            }
            unsupported => {
                self.diagnostics.push(Diagnostic::error(
                    format!(
                        "Native code generation for '{:?}' is not implemented yet",
                        unsupported
                    ),
                    Some(expr.span),
                ));
                IrOperand::Null
            }
        }
    }
}
