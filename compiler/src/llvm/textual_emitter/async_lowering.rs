//! Native poll functions: registers and local slots live in a rooted heap frame.
use super::*;
use std::collections::BTreeMap;

fn result(instruction: &IrInstruction) -> Vec<(Register, Type)> {
    use IrInstruction::*;
    match instruction {
        Cast { dest, ty, .. } | Load { dest, ty, .. } => vec![(*dest, ty.clone())],
        GetField { dest, field_ty, .. } => vec![(*dest, field_ty.clone())],
        Binary {
            dest, result_ty, ..
        } => vec![(*dest, result_ty.clone())],
        CallRuntime {
            dest: Some(dest),
            ret_ty,
            ..
        } => vec![(*dest, ret_ty.clone())],
        AwaitTask {
            dest,
            error_dest,
            result_ty,
            ..
        } => {
            if *result_ty == Type::Void {
                vec![(*error_dest, Type::Bytes)]
            } else {
                vec![(*dest, result_ty.clone()), (*error_dest, Type::Bytes)]
            }
        }
        _ => vec![],
    }
}
fn inputs(instruction: &IrInstruction) -> Vec<IrOperand> {
    use IrInstruction::*;
    match instruction {
        Cast { value, .. } | Store { val: value, .. } => vec![value.clone()],
        GetField {
            ptr, class_name, ..
        } => vec![IrOperand::Register(
            *ptr,
            Type::Class {
                name: class_name.clone(),
            },
        )],
        StoreField {
            ptr,
            class_name,
            val,
            ..
        } => vec![
            IrOperand::Register(
                *ptr,
                Type::Class {
                    name: class_name.clone(),
                },
            ),
            val.clone(),
        ],
        Binary { left, right, .. } => vec![left.clone(), right.clone()],
        CallRuntime { args, .. } => args.clone(),
        AwaitTask { task, .. } => vec![task.clone()],
        Branch { cond, .. } => vec![cond.clone()],
        Return { val } => val.iter().cloned().collect(),
        _ => vec![],
    }
}
fn nonblocking(symbol: &str) -> bool {
    symbol.starts_with("ube_exception_") && symbol != "ube_exception_fail_unhandled"
        || symbol.starts_with("ube_gc_")
        || symbol.starts_with("ube_list_")
        || symbol.starts_with("ube_map_")
        || symbol.starts_with("ube_task_outcome")
        || matches!(
            symbol,
            "ube_channel_create"
                | "ube_channel_create_capacity"
                | "ube_channel_send_async"
                | "ube_channel_recv_async"
                | "ube_channel_close"
                | "ube_task_timeout"
                | "ube_task_cancel"
                | "ube_task_all_async"
                | "ube_task_any_async"
                | "ube_task_race_async"
                | "ube_task_all_settled_async"
                | "ube_object_is"
                | "ube_object_satisfies"
                | "ube_log_put"
                | "ube_log_get"
                | "ube_log_remove"
                | "ube_log_clear"
        )
}

pub(super) fn layout(function: &IrFunction) -> Option<usize> {
    if !function.is_async
        || !function
            .blocks
            .iter()
            .flat_map(|b| &b.instructions)
            .any(|i| matches!(i, IrInstruction::AwaitTask { .. }))
    {
        return None;
    }
    if !std::iter::once(&function.receiver)
        .chain(function.params.iter())
        .enumerate()
        .all(|(index, (reg, _))| reg.0 == index)
    {
        return None;
    }
    let mut max = function.params.len();
    for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
        match instruction {
            IrInstruction::CallRuntime { symbol, .. } if !nonblocking(symbol) => return None,
            IrInstruction::JsonEncode { .. }
            | IrInstruction::CallMethod { .. }
            | IrInstruction::CallTrait { .. }
            | IrInstruction::InstantiateClass { .. }
            | IrInstruction::ServerRoute { .. }
            | IrInstruction::SpawnAsyncCall { .. }
            | IrInstruction::SpawnAsyncExpression { .. } => return None,
            IrInstruction::Alloca { dest, .. } => max = max.max(dest.0),
            _ => (),
        }
        for (reg, _) in result(instruction) {
            max = max.max(reg.0);
        }
        for input in inputs(instruction) {
            if let IrOperand::Register(reg, _) = input {
                max = max.max(reg.0);
            }
        }
    }
    Some((max + 1) * 2)
}

// Rebind just LLVM's numeric registers; never replace quoted string contents or
// named temporaries such as %slot_12. Every instruction has its own SSA names.
fn rebind(text: &str, names: &HashMap<usize, String>) -> String {
    let bytes = text.as_bytes();
    let mut output = String::new();
    let mut index = 0;
    let mut quoted = false;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            quoted = !quoted;
        }
        if !quoted && bytes[index] == b'%' && bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
            let start = index + 1;
            let mut end = start;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            let number: usize = text[start..end].parse().unwrap();
            output.push_str(
                names
                    .get(&number)
                    .unwrap_or_else(|| panic!("missing async register {number}")),
            );
            index = end;
        } else {
            let character = text[index..].chars().next().unwrap();
            output.push(character);
            index += character.len_utf8();
        }
    }
    output
}

impl TextualLlvmEmitter {
    pub(super) fn emit_poll_function(&mut self, function: &IrFunction) -> String {
        self.current_return_type = function.return_type.clone();
        self.slots = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .filter_map(|i| match i {
                IrInstruction::Alloca { dest, ty } => Some((*dest, ty.clone())),
                _ => None,
            })
            .collect();
        let instructions: Vec<_> = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .collect();
        let mut starts = HashMap::new();
        let mut offset = 0;
        for block in &function.blocks {
            starts.insert(block.id, offset);
            offset += block.instructions.len();
        }
        let symbol = format!("__ube_poll_{}_{}", function.class_name, function.name);
        let mut output = format!("define internal i1 @{symbol}(i8* %frame) {{\nentry:\n  call void @ube_gc_frame_enter()\n  %pc = call i64 @ube_poll_pc(i8* %frame)\n  switch i64 %pc, label %invalid [\n");
        for index in 0..instructions.len() {
            output.push_str(&format!("    i64 {index}, label %state_{index}\n"));
        }
        output.push_str("  ]\ninvalid:\n  unreachable\npending:\n  call void @ube_gc_frame_leave(i8* null)\n  ret i1 false\n");
        for (index, instruction) in instructions.iter().enumerate() {
            output.push_str(&format!("state_{index}:\n  %step_{index} = call i1 @ube_poll_step(i8* %frame, i64 {index})\n  br i1 %step_{index}, label %body_{index}, label %pending\nbody_{index}:\n"));
            let mut names = HashMap::new();
            let registers: BTreeMap<_, _> = inputs(instruction)
                .into_iter()
                .filter_map(|input| match input {
                    IrOperand::Register(reg, ty) => Some((reg.0, ty)),
                    _ => None,
                })
                .collect();
            for (reg, ty) in registers {
                output.push_str(&format!("  %raw_{index}_{reg} = call {{ i64, i64 }} @ube_poll_get(i8* %frame, i64 {})\n", reg * 2));
                let name = format!("input_{index}_{reg}");
                let decoded = self.decode_value(&ty, &format!("%raw_{index}_{reg}"), &name);
                for line in decoded.0 {
                    output.push_str(&format!("  {line}\n"));
                }
                names.insert(reg, decoded.1);
            }
            let definitions = result(instruction);
            for (reg, _) in &definitions {
                names.insert(reg.0, format!("%result_{index}_{}", reg.0));
            }
            // Match legacy escape barriers for runtime-retained references.
            if let IrInstruction::CallRuntime { symbol, args, .. } = instruction {
                if !symbol.starts_with("ube_gc_") {
                    for arg in args {
                        let escape = self.gc_escape(arg);
                        if !escape.is_empty() {
                            output.push_str(&format!("  {}\n", rebind(&escape, &names)));
                        }
                    }
                }
            }
            let mut lines = Vec::new();
            match instruction {
                IrInstruction::Alloca { .. } => (),
                IrInstruction::Store { val, ptr } => {
                    let ty = self.slots[ptr].clone();
                    let (casts, value) = self.coerce(val, &ty);
                    lines.extend(casts);
                    let encoded = self.encode_value(&ty, &value, &format!("store_{index}"));
                    lines.extend(encoded.0);
                    lines.push(format!(
                        "call void @ube_poll_set(i8* %frame, i64 {}, i64 {}, i64 {}, i1 {})",
                        ptr.0 * 2 + 1,
                        encoded.1,
                        encoded.2,
                        self.type_to_llvm(&ty).ends_with('*')
                    ));
                }
                IrInstruction::Load { dest, ptr, ty } => {
                    lines.push(format!(
                        "%loaded_{index} = call {{ i64, i64 }} @ube_poll_get(i8* %frame, i64 {})",
                        ptr.0 * 2 + 1
                    ));
                    lines.extend(
                        self.decode_value(ty, &format!("%loaded_{index}"), &dest.0.to_string())
                            .0,
                    );
                }
                IrInstruction::AwaitTask {
                    dest,
                    error_dest,
                    task,
                    result_ty,
                } => {
                    lines.push(format!(
                        "%ready_{index} = call i1 @ube_poll_await(i8* %frame, i8* {})",
                        self.emit_operand(task)
                    ));
                    lines.push(format!("br i1 %ready_{index}, label %await_ready_{index}, label %pending\nawait_ready_{index}:"));
                    lines.push(format!(
                        "%awaited_{index} = call {{ i64, i64 }} @ube_poll_value(i8* %frame)"
                    ));
                    lines.extend(
                        self.decode_value(
                            result_ty,
                            &format!("%awaited_{index}"),
                            &dest.0.to_string(),
                        )
                        .0,
                    );
                    lines.push(format!(
                        "%{} = call i8* @ube_exception_take()",
                        error_dest.0
                    ));
                }
                IrInstruction::Branch {
                    cond,
                    then_block,
                    else_block,
                } => {
                    lines.push(format!(
                        "br i1 {}, label %state_{}, label %state_{}",
                        self.emit_operand(cond),
                        starts[then_block],
                        starts[else_block]
                    ));
                }
                IrInstruction::Jump { target } => {
                    lines.push(format!("br label %state_{}", starts[target]))
                }
                IrInstruction::Return { val } => {
                    if let Some(value) = val.as_ref().filter(|_| function.return_type != Type::Void)
                    {
                        let operand = self.emit_operand(value);
                        let encoded = self.encode_value(
                            &function.return_type,
                            &operand,
                            &format!("return_{index}"),
                        );
                        lines.extend(encoded.0);
                        lines.push(format!(
                            "call void @ube_poll_finish(i8* %frame, i64 {}, i64 {})",
                            encoded.1, encoded.2
                        ));
                        let pointer = if self.type_to_llvm(&value.get_type()).ends_with('*') {
                            self.gc_pointer(&operand, &value.get_type())
                        } else {
                            (vec![], "null".into())
                        };
                        lines.extend(pointer.0);
                        lines.push(format!("call void @ube_gc_frame_leave(i8* {})", pointer.1));
                    } else {
                        lines.push("call void @ube_poll_finish(i8* %frame, i64 0, i64 0)".into());
                        lines.push("call void @ube_gc_frame_leave(i8* null)".into());
                    }
                    lines.push("ret i1 true".into());
                }
                _ => lines.push(self.emit_instruction(instruction)),
            }
            for line in lines {
                output.push_str(&format!("  {}\n", rebind(&line, &names)));
            }
            if !matches!(
                instruction,
                IrInstruction::Return { .. }
                    | IrInstruction::Branch { .. }
                    | IrInstruction::Jump { .. }
            ) {
                for (reg, ty) in definitions {
                    let encoded =
                        self.encode_value(&ty, &names[&reg.0], &format!("save_{index}_{}", reg.0));
                    for line in encoded.0 {
                        output.push_str(&format!("  {line}\n"));
                    }
                    output.push_str(&format!(
                        "  call void @ube_poll_set(i8* %frame, i64 {}, i64 {}, i64 {}, i1 {})\n",
                        reg.0 * 2,
                        encoded.1,
                        encoded.2,
                        self.type_to_llvm(&ty).ends_with('*')
                    ));
                }
                output.push_str(&format!("  br label %state_{}\n", index + 1));
            }
        }
        output.push_str("}\n");
        output
    }
}
