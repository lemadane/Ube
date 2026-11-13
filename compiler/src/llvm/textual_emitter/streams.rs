use super::*;

impl TextualLlvmEmitter {
    fn stream_kind(&self, ty: &Type) -> (i64, String) {
        let (kind, name) = self.json_kind(ty);
        if kind == 0 && matches!(ty, Type::Class { .. }) {
            (7, name)
        } else {
            (kind, name)
        }
    }

    /// Adapt a typed Func invocation to the stream engine's two-payload ABI.
    fn stream_callback(&mut self, callback: &IrOperand) -> (String, String) {
        let Type::Function {
            params,
            return_type,
        } = callback.get_type()
        else {
            unreachable!()
        };
        let name = format!("ube_sequence_callback_{}", self.string_counter);
        self.string_counter += 1;
        let signature = self.emit_operand(&IrOperand::ConstantString(method_signature(
            "invoke",
            &params,
            &return_type,
        )));
        let mut body = vec![
            format!("define i64 @{name}(i8* %context, i64 %a, i64 %b) {{"),
            "entry:".into(),
        ];
        let mut arguments = vec!["i8* %context".into()];
        for (index, ty) in params.iter().enumerate() {
            let (lines, value) = self.decode_i64(
                ty,
                if index == 0 { "%a" } else { "%b" },
                &format!("arg{index}"),
            );
            body.extend(lines);
            arguments.push(format!("{} {}", self.type_to_llvm(ty), value));
        }
        let result = self.type_to_llvm(&return_type);
        let types = std::iter::once("i8*".to_string())
            .chain(params.iter().map(|t| self.type_to_llvm(t)))
            .collect::<Vec<_>>()
            .join(", ");
        body.push(format!(
            "%raw = call i8* @ube_object_method(i8* %context, i8* {signature})"
        ));
        body.push("%valid = icmp ne i8* %raw, null".into());
        body.push("br i1 %valid, label %invoke, label %invalid".into());
        body.push("invalid:".into());
        body.push("%failure = call i64 @ube_sequence_invalid_callback()".into());
        body.push("ret i64 %failure".into());
        body.push("invoke:".into());
        body.push(format!(
            "%function = bitcast i8* %raw to {result} ({types})*"
        ));
        if *return_type == Type::Void {
            body.push(format!("call void %function({})", arguments.join(", ")));
            body.push("ret i64 0".into());
        } else {
            body.push(format!(
                "%result = call {result} %function({})",
                arguments.join(", ")
            ));
            let (lines, payload) = self.encode_i64(&return_type, "%result", "encoded");
            body.extend(lines);
            body.push(format!("ret i64 {payload}"));
        }
        body.push("}".into());
        self.task_thunks.push(body.join("\n  "));
        (self.emit_operand(callback), format!("@{name}"))
    }

    pub(super) fn emit_stream_call(
        &mut self,
        dest: Option<Register>,
        symbol: &str,
        args: &[IrOperand],
        result: &Type,
    ) -> String {
        let method = symbol.strip_prefix("ube_sequence_").unwrap();
        let receiver = args
            .first()
            .map(|a| self.emit_operand(a))
            .unwrap_or_else(|| "null".into());
        if method == "values_add" || method == "list_add" {
            let value = self.emit_operand(&args[1]);
            let (mut lines, payload) = self.encode_i64(
                &args[1].get_type(),
                &value,
                &format!("sequence_value_{}", self.string_counter),
            );
            self.string_counter += 1;
            let (kind, name) = self.stream_kind(&args[1].get_type());
            let name = self.emit_operand(&IrOperand::ConstantString(name));
            lines.push(format!("call void @ube_sequence_{method}(i8* {receiver}, i64 {payload}, i64 {kind}, i8* {name})"));
            return lines.join("\n  ");
        }
        if method == "range" {
            let end = self.emit_operand(&args[1]);
            return format!(
                "%{} = call i8* @ube_sequence_range(i32 {receiver}, i32 {end})",
                dest.unwrap().0
            );
        }
        if method == "close" {
            return format!("call void @ube_sequence_close(i8* {receiver})");
        }
        if method == "get" {
            let index = self.emit_operand(&args[1]);
            let id = dest.unwrap().0;
            let mut lines = vec![format!(
                "%sequence_get_{id} = call i64 @ube_sequence_get(i8* {receiver}, i32 {index})"
            )];
            lines.extend(
                self.decode_i64(result, &format!("%sequence_get_{id}"), &id.to_string())
                    .0,
            );
            return lines.join("\n  ");
        }
        let element = match result {
            Type::Stream(t) => t.as_ref(),
            _ => match args[0].get_type() {
                Type::Stream(_) => result,
                _ => unreachable!(),
            },
        };
        if method == "source" || method == "owned_source" {
            let (kind, name) = self.stream_kind(element);
            let name = self.emit_operand(&IrOperand::ConstantString(name));
            return format!(
                "%{} = call i8* @ube_sequence_{method}(i8* {receiver}, i64 {kind}, i8* {name})",
                dest.unwrap().0
            );
        }
        let stage = match method {
            "filter" => Some(0),
            "map" => Some(1),
            "peek" => Some(2),
            "limit" => Some(3),
            "skip" => Some(4),
            "unique" | "distinct" => Some(5),
            "sort" | "sorted" => Some(6),
            _ => None,
        };
        let callback = match method {
            "filter" | "map" | "peek" | "forEach" | "anyMatch" | "allMatch" | "noneMatch"
            | "some" | "every" | "none" | "findFirst" => args.get(1),
            "reduce" => args.get(2),
            _ => None,
        };
        let (context, function) = callback
            .map(|c| self.stream_callback(c))
            .unwrap_or_else(|| ("null".into(), "null".into()));
        let mut lines = Vec::new();
        if let Some(op) = stage {
            let amount = if matches!(method, "limit" | "skip") {
                let arg = self.emit_operand(&args[1]);
                let (encoded, value) = self.encode_i64(
                    &args[1].get_type(),
                    &arg,
                    &format!("sequence_amount_{}", self.string_counter),
                );
                self.string_counter += 1;
                lines.extend(encoded);
                value
            } else {
                "0".into()
            };
            let (kind, name) = self.stream_kind(element);
            let name = self.emit_operand(&IrOperand::ConstantString(name));
            lines.push(format!("%{} = call i8* @ube_sequence_stage(i8* {receiver}, i32 {op}, i8* {context}, i64 (i8*, i64, i64)* {function}, i64 {amount}, i64 {kind}, i8* {name})", dest.unwrap().0));
        } else {
            let op = match method {
                "count" => 0,
                "collect" | "toList" => 1,
                "forEach" => 2,
                "some" | "anyMatch" => 3,
                "every" | "allMatch" => 4,
                "none" | "noneMatch" => 5,
                "findFirst" if args.len() == 2 => 9,
                "findFirst" => 6,
                "sum" => 7,
                "reduce" => 8,
                _ => unreachable!(),
            };
            let identity = if method == "reduce" {
                let arg = self.emit_operand(&args[1]);
                let (encoded, value) = self.encode_i64(
                    &args[1].get_type(),
                    &arg,
                    &format!("sequence_identity_{}", self.string_counter),
                );
                self.string_counter += 1;
                lines.extend(encoded);
                value
            } else {
                "0".into()
            };
            let raw = format!("sequence_result_{}", self.string_counter);
            self.string_counter += 1;
            lines.push(format!("%{raw} = call {{ i64, i64 }} @ube_sequence_terminal(i8* {receiver}, i32 {op}, i8* {context}, i64 (i8*, i64, i64)* {function}, i64 {identity})"));
            if let Some(dest) = dest {
                lines.extend(
                    self.decode_value(result, &format!("%{raw}"), &dest.0.to_string())
                        .0,
                );
            }
        }
        lines.join("\n  ")
    }
}
