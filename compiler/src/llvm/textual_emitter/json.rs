use super::*;
impl TextualLlvmEmitter {
    pub(super) fn json_kind(&self, ty: &Type) -> (i64, String) {
        let nullable = matches!(ty, Type::Nullable(_));
        let inner = if let Type::Nullable(inner) = ty {
            inner.as_ref()
        } else {
            ty
        };
        let (kind, name) = match inner {
            Type::Int => (1, "int".into()),
            Type::Long => (2, "long".into()),
            Type::Byte => (3, "byte".into()),
            Type::Bool => (4, "bool".into()),
            Type::Decimal => (5, "decimal".into()),
            Type::String => (6, "string".into()),
            Type::Void => (9, "null".into()),
            Type::Sequence(_) => (10, "List".into()),
            Type::Class { name } if name == "Map" => (11, name.clone()),
            Type::Class { name } if name == "List" => (10, name.clone()),
            Type::Class { name } if self.class_map.contains_key(name) => (7, name.clone()),
            Type::Class { name } if name == "JsonObject" || name == "JsonArray" => {
                (8, name.clone())
            }
            _ => (0, ty.to_string()),
        };
        (kind | if nullable { 256 } else { 0 }, name)
    }
    pub(super) fn emit_json_encode(&mut self, dest: Register, value: &IrOperand) -> String {
        let ty = value.get_type();
        let (kind, name) = self.json_kind(&ty);
        let source = self.emit_operand(value);
        let (mut lines, payload, meta) =
            self.encode_value(&ty, &source, &format!("json_{}", dest.0));
        let name = self.emit_operand(&IrOperand::ConstantString(name));
        lines.push(format!(
            "%{} = call i8* @ube_to_json(i64 {}, i64 {}, i64 {}, i8* {})",
            dest.0, payload, meta, kind, name
        ));
        lines.join("\n  ")
    }
    pub(super) fn emit_xml_encode(&mut self,dest:Register,value:&IrOperand,options:&Option<IrOperand>)->String {
        let ty=value.get_type();let (kind,name)=self.json_kind(&ty);let source=self.emit_operand(value);
        let (mut lines,payload,meta)=self.encode_value(&ty,&source,&format!("xml_{}",dest.0));
        let name=self.emit_operand(&IrOperand::ConstantString(name));
        let options=options.as_ref().map(|value|self.emit_operand(value)).unwrap_or_else(||"null".into());
        lines.push(format!("%{} = call i8* @ube_to_xml(i64 {}, i64 {}, i64 {}, i8* {}, i8* {})",dest.0,payload,meta,kind,name,options));
        lines.join("\n  ")
    }
    pub(super) fn emit_json_schemas(&mut self, program: &IrProgram) {
        for class in &program.classes {
            let owner = self.emit_operand(&IrOperand::ConstantString(class.name.clone()));
            self.output
                .push_str(&format!("  call void @ube_json_class(i8* {})\n", owner));
            for (index, (name, ty)) in class.fields.iter().enumerate() {
                let field = self.emit_operand(&IrOperand::ConstantString(
                    name.strip_prefix('_').unwrap_or(name).into(),
                ));
                let (kind, child) = self.json_kind(ty);
                let child = self.emit_operand(&IrOperand::ConstantString(child));
                let llvm = self.type_to_llvm(ty);
                let offset = format!(
                    "ptrtoint ({}* getelementptr (%{}, %{}* null, i32 0, i32 {}) to i64)",
                    llvm, class.name, class.name, index
                );
                let (payload, presence): (String, String) =
                    if let Some(inner) = ty.nullable_scalar_inner() {
                        (
                            format!(
                                "ptrtoint ({}* getelementptr ({}, {}* null, i32 0, i32 1) to i64)",
                                self.type_to_llvm(inner),
                                llvm,
                                llvm
                            ),
                            "0".into(),
                        )
                    } else {
                        ("0".into(), "-1".into())
                    };
                let inner = ty.nullable_scalar_inner().unwrap_or(ty);
                let width = match inner {
                    Type::Byte | Type::Bool => 1,
                    Type::Int => 4,
                    _ => 8,
                };
                self.output.push_str(&format!("  call void @ube_json_field(i8* {}, i8* {}, i8* {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",owner,field,child,kind,offset,payload,presence,width));
            }
        }
    }
}
