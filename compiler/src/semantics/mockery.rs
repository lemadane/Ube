impl TypeChecker {
    fn check_mockery_call(
        &mut self,
        cls: &ClassDecl,
        member: &str,
        args: &[Expr],
        types: &[Type],
        static_call: bool,
        span: crate::diagnostics::Span,
    ) -> Option<Type> {
        let invalid = |checker: &mut Self, message: &str| {
            checker
                .diagnostics
                .push(Diagnostic::error(message, Some(span)));
            None
        };
        if !static_call || types.len() != args.len() {
            return invalid(
                self,
                "Mockery methods require static calls with valid non-null arguments",
            );
        }
        if let Some(kind) = member.strip_prefix("_call_") {
            let result = match kind {
                "void" => Type::Void,
                "int" => Type::Int,
                "long" => Type::Long,
                "byte" => Type::Byte,
                "bool" => Type::Bool,
                "decimal" => Type::Decimal,
                "string" => Type::String,
                _ => return invalid(self, "Unknown internal Mockery method"),
            };
            if cls.name.starts_with(crate::mockery::PREFIX) && types == [Type::Long, Type::String] {
                return Some(result);
            }
            return invalid(
                self,
                "Internal Mockery invocation methods are reserved for generated mocks",
            );
        }
        let Some(Type::Class { name }) = types.first() else {
            return invalid(
                self,
                "Mockery requires a mock handle returned by Mockery.mock",
            );
        };
        let Some(contract) = name
            .strip_prefix(crate::mockery::PREFIX)
            .and_then(|name| self.traits.get(name))
        else {
            return invalid(
                self,
                "Mockery requires a mock handle returned by Mockery.mock",
            );
        };
        if matches!(member, "reset" | "clearCalls" | "dispose" | "verifyNoCalls") {
            if args.len() != 1 {
                return invalid(
                    self,
                    "Mockery reset/clearCalls/dispose/verifyNoCalls expect one mock handle",
                );
            }
            return Some(Type::Void);
        }
        let Some(Expr {
            kind: ExprKind::StringLiteral(method),
            ..
        }) = args.get(1)
        else {
            return invalid(
                self,
                "Mockery requires a literal method name declared on the mocked trait",
            );
        };
        let Some((_, result)) = contract.methods.get(method).cloned() else {
            return invalid(
                self,
                "Mockery method name is not declared on the mocked trait",
            );
        };
        let valid = match member {
            "returns" => types.len() == 3 && result != Type::Void && types[2] == result,
            "allow" => types.len() == 2 && result == Type::Void,
            "throws" => types.len() == 3 && types[2] == Type::String,
            "verify" => types.len() == 3 && types[2] == Type::Int,
            "calls" => types.len() == 2,
            _ => false,
        };
        if !valid {
            return invalid(self, "Invalid Mockery call: returns must match the method return type; allow requires implicit void; throws requires string; verify requires int count");
        }
        Some(if member == "calls" {
            Type::Long
        } else {
            Type::Void
        })
    }
}
