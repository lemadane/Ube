const BUILTIN_CLASS_NAMES: &[&str] = &[
    "AlreadyExistsError",
    "Channel",
    "Console",
    "Cookie",
    "Currency",
    "DateTime",
    "Directory",
    "DiskFullError",
    "Distance",
    "Duration",
    "Error",
    "Exchange",
    "File",
    "FileError",
    "FileInfo",
    "FileNotFoundError",
    "GenericError",
    "GeoBounds",
    "GeoPoint",
    "Geolocation",
    "InvalidPathError",
    "InvalidTypeError",
    "Json",
    "JsonArray",
    "JsonError",
    "JsonObject",
    "JsonParseError",
    "List",
    "Log",
    "LocationPermissionError",
    "LocationUnavailableError",
    "Map",
    "Math",
    "MissingFieldError",
    "Money",
    "Path",
    "Percent",
    "PermissionError",
    "Probability",
    "Random",
    "Rate",
    "Request",
    "Response",
    "SecureRandom",
    "Server",
    "Statistics",
    "SQLite",
    "SQLiteResult",
    "SQLiteStatement",
    "SQLiteError",
    "Task",
    "WebSocket",
    "Xml",
    "XmlDocument",
    "XmlElement",
    "XmlError",
    "XmlParseError",
];

pub fn canonical_builtin_class_name(name: &str) -> Option<&'static str> {
    BUILTIN_CLASS_NAMES
        .iter()
        .copied()
        .find(|canonical| name == *canonical)
}

/// Suggest the canonical spelling for a miscapitalized built-in class.
pub fn builtin_class_case_suggestion(name: &str) -> Option<&'static str> {
    BUILTIN_CLASS_NAMES.iter().copied()
        .find(|canonical| name != *canonical && name.eq_ignore_ascii_case(canonical))
}

/// Shared SQLite ABI: keep semantic checks and emitted calls in agreement.
pub struct SQLiteMethod {
    pub name: &'static str,
    pub symbol: &'static str,
    pub params: Vec<crate::types::Type>,
    pub result: crate::types::Type,
    pub is_static: bool,
}
pub const SQLITE_CLASSES: &[&str] = &["SQLite", "SQLiteStatement", "SQLiteResult"];
pub fn sqlite_methods(class: &str) -> Vec<SQLiteMethod> {
    use crate::types::Type;
    match class {
        "SQLite" => vec![
            SQLiteMethod {
                name: "backup",
                symbol: "ube_sqlite_backup",
                params: vec![Type::String],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "restore",
                symbol: "ube_sqlite_restore",
                params: vec![Type::String],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "readOnly",
                symbol: "ube_sqlite_read_only",
                params: vec![],
                result: Type::Bool,
                is_static: false,
            },
            SQLiteMethod {
                name: "open",
                symbol: "ube_sqlite_open",
                params: vec![Type::String],
                result: Type::Class {
                    name: "SQLite".into(),
                },
                is_static: true,
            },
            SQLiteMethod {
                name: "openReadOnly",
                symbol: "ube_sqlite_open_read_only",
                params: vec![Type::String],
                result: Type::Class {
                    name: "SQLite".into(),
                },
                is_static: true,
            },
            SQLiteMethod {
                name: "version",
                symbol: "ube_sqlite_version",
                params: vec![],
                result: Type::String,
                is_static: true,
            },
            SQLiteMethod {
                name: "execute",
                symbol: "ube_sqlite_execute",
                params: vec![Type::String],
                result: Type::Long,
                is_static: false,
            },
            SQLiteMethod {
                name: "executeBatch",
                symbol: "ube_sqlite_execute_batch",
                params: vec![Type::String],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "query",
                symbol: "ube_sqlite_query",
                params: vec![Type::String],
                result: Type::Class {
                    name: "SQLiteResult".into(),
                },
                is_static: false,
            },
            SQLiteMethod {
                name: "prepare",
                symbol: "ube_sqlite_prepare",
                params: vec![Type::String],
                result: Type::Class {
                    name: "SQLiteStatement".into(),
                },
                is_static: false,
            },
            SQLiteMethod {
                name: "begin",
                symbol: "ube_sqlite_begin",
                params: vec![],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "commit",
                symbol: "ube_sqlite_commit",
                params: vec![],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "rollback",
                symbol: "ube_sqlite_rollback",
                params: vec![],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "inTransaction",
                symbol: "ube_sqlite_in_transaction",
                params: vec![],
                result: Type::Bool,
                is_static: false,
            },
            SQLiteMethod {
                name: "busyTimeout",
                symbol: "ube_sqlite_busy_timeout",
                params: vec![Type::Int],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "lastInsertRowId",
                symbol: "ube_sqlite_last_insert_row_id",
                params: vec![],
                result: Type::Long,
                is_static: false,
            },
            SQLiteMethod {
                name: "changes",
                symbol: "ube_sqlite_changes",
                params: vec![],
                result: Type::Long,
                is_static: false,
            },
            SQLiteMethod {
                name: "totalChanges",
                symbol: "ube_sqlite_total_changes",
                params: vec![],
                result: Type::Long,
                is_static: false,
            },
            SQLiteMethod {
                name: "close",
                symbol: "ube_sqlite_close",
                params: vec![],
                result: Type::Void,
                is_static: false,
            },
        ],
        "SQLiteStatement" => vec![
            SQLiteMethod {
                name: "bindInt",
                symbol: "ube_sqlite_statement_bind_int",
                params: vec![Type::Int, Type::Int],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "bindLong",
                symbol: "ube_sqlite_statement_bind_long",
                params: vec![Type::Int, Type::Long],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "bindDecimal",
                symbol: "ube_sqlite_statement_bind_decimal",
                params: vec![Type::Int, Type::Decimal],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "bindBool",
                symbol: "ube_sqlite_statement_bind_bool",
                params: vec![Type::Int, Type::Bool],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "bindString",
                symbol: "ube_sqlite_statement_bind_string",
                params: vec![Type::Int, Type::String],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "bindNull",
                symbol: "ube_sqlite_statement_bind_null",
                params: vec![Type::Int],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "bindBlobHex",
                symbol: "ube_sqlite_statement_bind_blob_hex",
                params: vec![Type::Int, Type::String],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "parameterCount",
                symbol: "ube_sqlite_statement_parameter_count",
                params: vec![],
                result: Type::Int,
                is_static: false,
            },
            SQLiteMethod {
                name: "parameterName",
                symbol: "ube_sqlite_statement_parameter_name",
                params: vec![Type::Int],
                result: Type::String,
                is_static: false,
            },
            SQLiteMethod {
                name: "parameterIndex",
                symbol: "ube_sqlite_statement_parameter_index",
                params: vec![Type::String],
                result: Type::Int,
                is_static: false,
            },
            SQLiteMethod {
                name: "clearBindings",
                symbol: "ube_sqlite_statement_clear_bindings",
                params: vec![],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "execute",
                symbol: "ube_sqlite_statement_execute",
                params: vec![],
                result: Type::Long,
                is_static: false,
            },
            SQLiteMethod {
                name: "query",
                symbol: "ube_sqlite_statement_query",
                params: vec![],
                result: Type::Class {
                    name: "SQLiteResult".into(),
                },
                is_static: false,
            },
            SQLiteMethod {
                name: "close",
                symbol: "ube_sqlite_statement_close",
                params: vec![],
                result: Type::Void,
                is_static: false,
            },
        ],
        "SQLiteResult" => vec![
            SQLiteMethod {
                name: "next",
                symbol: "ube_sqlite_result_next",
                params: vec![],
                result: Type::Bool,
                is_static: false,
            },
            SQLiteMethod {
                name: "reset",
                symbol: "ube_sqlite_result_reset",
                params: vec![],
                result: Type::Void,
                is_static: false,
            },
            SQLiteMethod {
                name: "rowCount",
                symbol: "ube_sqlite_result_row_count",
                params: vec![],
                result: Type::Long,
                is_static: false,
            },
            SQLiteMethod {
                name: "columnCount",
                symbol: "ube_sqlite_result_column_count",
                params: vec![],
                result: Type::Int,
                is_static: false,
            },
            SQLiteMethod {
                name: "columnName",
                symbol: "ube_sqlite_result_column_name",
                params: vec![Type::Int],
                result: Type::String,
                is_static: false,
            },
            SQLiteMethod {
                name: "columnIndex",
                symbol: "ube_sqlite_result_column_index",
                params: vec![Type::String],
                result: Type::Int,
                is_static: false,
            },
            SQLiteMethod {
                name: "columnType",
                symbol: "ube_sqlite_result_column_type",
                params: vec![Type::Int],
                result: Type::String,
                is_static: false,
            },
            SQLiteMethod {
                name: "getInt",
                symbol: "ube_sqlite_result_get_int",
                params: vec![Type::Int],
                result: Type::Int,
                is_static: false,
            },
            SQLiteMethod {
                name: "getLong",
                symbol: "ube_sqlite_result_get_long",
                params: vec![Type::Int],
                result: Type::Long,
                is_static: false,
            },
            SQLiteMethod {
                name: "getDecimal",
                symbol: "ube_sqlite_result_get_decimal",
                params: vec![Type::Int],
                result: Type::Decimal,
                is_static: false,
            },
            SQLiteMethod {
                name: "getBool",
                symbol: "ube_sqlite_result_get_bool",
                params: vec![Type::Int],
                result: Type::Bool,
                is_static: false,
            },
            SQLiteMethod {
                name: "getString",
                symbol: "ube_sqlite_result_get_string",
                params: vec![Type::Int],
                result: Type::String,
                is_static: false,
            },
            SQLiteMethod {
                name: "getBlobHex",
                symbol: "ube_sqlite_result_get_blob_hex",
                params: vec![Type::Int],
                result: Type::String,
                is_static: false,
            },
            SQLiteMethod {
                name: "isNull",
                symbol: "ube_sqlite_result_is_null",
                params: vec![Type::Int],
                result: Type::Bool,
                is_static: false,
            },
            SQLiteMethod {
                name: "close",
                symbol: "ube_sqlite_result_close",
                params: vec![],
                result: Type::Void,
                is_static: false,
            },
        ],
        _ => vec![],
    }
}

#[cfg(test)]
mod capitalization_tests {
    use super::*;
    use crate::{Lexer, Parser, TypeChecker};

    #[test]
    fn every_builtin_requires_canonical_case_in_constructors_types_and_static_calls() {
        for &name in BUILTIN_CLASS_NAMES {
            let canonical = format!("class Application {{ main() {{ final value = {name}() }} }}");
            let mut parser = Parser::new(Lexer::new(&canonical));
            parser.parse_program();
            if name == "Log" {
                assert!(parser.diagnostics.iter().any(|d| d.message.contains("Use log(value)")));
            } else {
                assert!(parser.diagnostics.is_empty(), "{name}: {:?}", parser.diagnostics);
            }
            let lower = name.to_ascii_lowercase();
            for source in [
                format!("class Application {{ main() {{ final value = {lower}() }} }}"),
                format!("class Application({lower} value) {{}}"),
            ] {
                let mut parser = Parser::new(Lexer::new(&source));
                parser.parse_program();
                if name == "Log" && source.contains("log()") {
                    assert!(parser.diagnostics.is_empty(), "{source}: {:?}", parser.diagnostics);
                } else {
                    assert!(parser.diagnostics.iter().any(|d| d.message.contains("capitalization")), "{source}: {:?}", parser.diagnostics);
                }
            }
            let source = format!("class Application {{ main() {{ {lower}.method() }} }}");
            let mut parser = Parser::new(Lexer::new(&source));
            let program = parser.parse_program();
            assert!(parser.diagnostics.is_empty(), "{source}: {:?}", parser.diagnostics);
            let mut checker = TypeChecker::new();
            assert!(!checker.check_program(&program), "accepted {source}");
            assert!(checker.diagnostics.iter().any(|d| d.message.contains("capitalization")), "{source}: {:?}", checker.diagnostics);
        }
    }
}
