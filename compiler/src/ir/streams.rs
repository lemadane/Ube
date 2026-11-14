impl IrBuilder {
    fn build_stream_call(
        &mut self,
        receiver_type: &Type,
        receiver: IrOperand,
        method: &str,
        args: Vec<IrOperand>,
        result: Type,
    ) -> IrOperand {
        if matches!(receiver_type, Type::Class { name } if name == "Stream") {
            if method == "range" {
                return self.runtime("ube_sequence_range", args, result);
            }
            let list = self.runtime(
                "ube_list_create",
                vec![],
                Type::Class {
                    name: "List".into(),
                },
            );
            for value in args {
                self.runtime(
                    "ube_sequence_values_add",
                    vec![list.clone(), value],
                    Type::Void,
                );
            }
            return self.runtime("ube_sequence_owned_source", vec![list], result);
        }
        if method == "stream" {
            return self.runtime("ube_sequence_source", vec![receiver], result);
        }
        if matches!(receiver_type, Type::Sequence(_)) {
            let mut all = vec![receiver];
            all.extend(args);
            return self.runtime(
                match method {
                    "length" => "ube_list_len",
                    "add" => "ube_sequence_list_add",
                    _ => "ube_sequence_get",
                },
                all,
                result,
            );
        }
        let mut all = vec![receiver];
        all.extend(args);
        self.runtime(&format!("ube_sequence_{method}"), all, result)
    }
}
