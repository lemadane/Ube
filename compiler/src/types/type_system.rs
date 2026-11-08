use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Int,
    Decimal,
    Byte,
    Long,
    Bool,
    String,
    Void,
    Bytes,
    InputStream,
    OutputStream,
    FileStream,
    Async(Box<Type>),
    Channel(Box<Type>),
    Orm { model: String, kind: crate::orm::Kind, joins: Vec<(String, String)> },
    Class {
        name: std::string::String,
    },

    Nullable(Box<Type>),
    AnonymousObject {
        field_types: Vec<(std::string::String, Type)>,
    },
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },
}

impl Type {
    pub fn nullable_scalar_inner(&self) -> Option<&Type> {
        match self {
            Type::Nullable(inner) if matches!(inner.as_ref(), Type::Int | Type::Long | Type::Byte | Type::Bool | Type::Decimal) => Some(inner),
            _ => None,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => write!(f, "int"),
            Type::Decimal => write!(f, "decimal"),
            Type::Byte => write!(f, "byte"),
            Type::Long => write!(f, "long"),
            Type::Bool => write!(f, "bool"),
            Type::String => write!(f, "string"),
            Type::Void => write!(f, "void"),
            Type::Bytes => write!(f, "Bytes"),
            Type::InputStream => write!(f, "InputStream"),
            Type::OutputStream => write!(f, "OutputStream"),
            Type::FileStream => write!(f, "FileStream"),
            Type::Async(result) => write!(f, "Async<{}>", result),
            Type::Channel(element) => write!(f, "Channel<{}>", element),
            Type::Class { name } => write!(f, "{}", name),
            Type::Orm { model, kind, .. } => write!(f, "{}<{}>", kind.class(), model),

            Type::Nullable(inner) => write!(f, "{}?", inner),
            Type::AnonymousObject { field_types } => {
                let fields_str = field_types
                    .iter()
                    .map(|(n, t)| format!("{}: {}", n, t))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{{ {} }}", fields_str)
            }
            Type::Function {
                params,
                return_type,
            } => {
                let params_str = params
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "Func({}) -> {}", params_str, return_type)
            }
        }
    }
}
