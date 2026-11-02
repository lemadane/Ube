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
    Task(Box<Type>),
    Channel(Box<Type>),
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
            Type::Task(result) => write!(f, "Task<{}>", result),
            Type::Channel(element) => write!(f, "Channel<{}>", element),
            Type::Class { name } => write!(f, "{}", name),

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
                write!(f, "fn({}) -> {}", params_str, return_type)
            }
        }
    }
}
