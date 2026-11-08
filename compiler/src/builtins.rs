const BUILTIN_CLASS_NAMES: &[&str] = &[
    "AlreadyExistsError",
    "Testicle",
    "AssertionError",
    "Channel",
    "Cache",
    "Console",
    "ConsoleArgs",
    "Cookie",
    "Currency",
    "CurrencyMismatchError",
    "DateTime",
    "Directory",
    "DiskFullError",
    "Distance",
    "Duration",
    "Error",
    "Env",
    "Exchange",
    "File",
    "FileError",
    "FileInfo",
    "FileNotFoundError",
    "GenericError",
    "Gc",
    "GeoBounds",
    "GeoPoint",
    "Geolocation",
    "InvalidPathError",
    "InvalidTypeError",
    "Jwt",
    "JwtClaims",
    "OAuth2",
    "OAuth2Token",
    "Orm",
    "OrmModel",
    "OrmQuery",
    "OrmRows",
    "Json",
    "KeyValueStore",
    "JsonArray",
    "JsonError",
    "JsonObject",
    "JsonParseError",
    "List",
    "Log",
    "Metrics",
    "Health",
    "Observability",
    "LocationPermissionError",
    "LocationUnavailableError",
    "Map",
    "Math",
    "MissingFieldError",
    "Money",
    "MoneyError",
    "Mockery",
    "MockeryError",
    "Path",
    "Percent",
    "PermissionError",
    "Probability",
    "Random",
    "Rate",
    "Request",
    "Response",
    "RestInPeace",
    "RestInPeaceRequest",
    "RestInPeaceResponse",
    "RestInPeaceError",
    "SecureRandom",
    "Server",
    "Session",
    "Statistics",
    "SQL",
    "SQLParams",
    "SQLTransaction",
    "SQLResult",
    "SQLError",
    "SQLite",
    "SQLiteResult",
    "SQLiteStatement",
    "SQLiteError",
    "Task",
    "TaskOutcome",
    "TaskOutcomes",
    "WebSocket",
    "Xml",
    "XmlOptions",
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

/// Shared built-in ABI: keep semantic checks and emitted calls in agreement.
pub struct BuiltinMethod {
    pub name: &'static str,
    pub symbol: &'static str,
    pub params: Vec<crate::types::Type>,
    pub result: crate::types::Type,
    pub is_static: bool,
}
pub type SQLiteMethod = BuiltinMethod;
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
            let canonical = format!("class App {{ start() {{ final value = {name}() }} }}");
            let mut parser = Parser::new(Lexer::new(&canonical));
            parser.parse_program();
            if name == "Log" {
                assert!(parser.diagnostics.iter().any(|d| d.message.contains("Use log(value)")));
            } else {
                assert!(parser.diagnostics.is_empty(), "{name}: {:?}", parser.diagnostics);
            }
            let lower = name.to_ascii_lowercase();
            for source in [
                format!("class App {{ start() {{ final value = {lower}() }} }}"),
                format!("class App({lower} value) {{}}"),
            ] {
                let mut parser = Parser::new(Lexer::new(&source));
                parser.parse_program();
                if name == "Log" && source.contains("log()") {
                    assert!(parser.diagnostics.is_empty(), "{source}: {:?}", parser.diagnostics);
                } else {
                    assert!(parser.diagnostics.iter().any(|d| d.message.contains("capitalization")), "{source}: {:?}", parser.diagnostics);
                }
            }
            let source = format!("class App {{ start() {{ {lower}.method() }} }}");
            let mut parser = Parser::new(Lexer::new(&source));
            let program = parser.parse_program();
            assert!(parser.diagnostics.is_empty(), "{source}: {:?}", parser.diagnostics);
            let mut checker = TypeChecker::new();
            assert!(!checker.check_program(&program), "accepted {source}");
            assert!(checker.diagnostics.iter().any(|d| d.message.contains("capitalization")), "{source}: {:?}", checker.diagnostics);
        }
    }
}

/// Authentication ABI shared by semantic checks, lowering, and LLVM declarations.
pub const AUTH_CLASSES: &[&str] = &["Jwt", "JwtClaims", "OAuth2", "OAuth2Token"];
pub fn auth_methods(class: &str) -> Vec<BuiltinMethod> {
    use crate::types::Type;
    let string = Type::String;
    let nullable = Type::Nullable(Box::new(Type::String));
    let cls = |name: &str| Type::Class { name: name.into() };
    let methods: Vec<(&'static str, &'static str, Vec<Type>, Type, bool)> = match class {
        "Jwt" => vec![
            (
                "sign",
                "ube_jwt_sign",
                vec![string.clone(); 3],
                string.clone(),
                true,
            ),
            (
                "verify",
                "ube_jwt_verify",
                vec![string.clone(); 5],
                cls("JwtClaims"),
                true,
            ),
        ],
        "JwtClaims" => vec![
            (
                "get",
                "ube_jwt_claims_get",
                vec![string.clone()],
                nullable.clone(),
                false,
            ),
            ("json", "ube_jwt_claims_json", vec![], string.clone(), false),
            ("close", "ube_jwt_claims_close", vec![], Type::Void, false),
        ],
        "OAuth2" => vec![
            (
                "create",
                "ube_oauth2_create",
                vec![string.clone(); 5],
                cls("OAuth2"),
                true,
            ),
            (
                "authorize",
                "ube_oauth2_authorize",
                vec![cls("Session"), string.clone()],
                string.clone(),
                false,
            ),
            (
                "exchange",
                "ube_oauth2_exchange",
                vec![cls("Session"), string.clone(), string.clone()],
                cls("OAuth2Token"),
                false,
            ),
            (
                "refresh",
                "ube_oauth2_refresh",
                vec![string.clone()],
                cls("OAuth2Token"),
                false,
            ),
            ("close", "ube_oauth2_close", vec![], Type::Void, false),
        ],
        "OAuth2Token" => vec![
            (
                "accessToken",
                "ube_oauth2_token_access",
                vec![],
                string.clone(),
                false,
            ),
            (
                "tokenType",
                "ube_oauth2_token_type",
                vec![],
                string.clone(),
                false,
            ),
            (
                "refreshToken",
                "ube_oauth2_token_refresh",
                vec![],
                nullable.clone(),
                false,
            ),
            (
                "expiresIn",
                "ube_oauth2_token_expires",
                vec![],
                Type::Long,
                false,
            ),
            (
                "scope",
                "ube_oauth2_token_scope",
                vec![],
                nullable.clone(),
                false,
            ),
            ("close", "ube_oauth2_token_close", vec![], Type::Void, false),
        ],
        _ => vec![],
    };
    methods
        .into_iter()
        .map(|(name, symbol, params, result, is_static)| BuiltinMethod {
            name,
            symbol,
            params,
            result,
            is_static,
        })
        .collect()
}

/// Shared execution ABI for raw SQL, future query builders, and ORM adapters.
pub const SQL_CLASSES: &[&str] = &["SQL", "SQLParams", "SQLTransaction", "SQLResult"];
pub fn sql_methods(class: &str) -> Vec<BuiltinMethod> {
    use crate::types::Type;
    let object = |name: &str| Type::Class { name: name.into() };
    let method = |name, symbol, params, result, is_static| BuiltinMethod { name, symbol, params, result, is_static };
    match class {
        "SQL" => vec![
            method("open", "ube_sql_open", vec![Type::String], object("SQL"), true),
            method("openPool", "ube_sql_open_pool", vec![Type::String, Type::Int, Type::Int], object("SQL"), true),
            method("dialect", "ube_sql_dialect", vec![], Type::String, false),
            method("placeholder", "ube_sql_placeholder", vec![Type::Int], Type::String, false),
            method("quoteIdentifier", "ube_sql_quote_identifier", vec![Type::String], Type::String, false),
            method("execute", "ube_sql_execute", vec![Type::String, object("SQLParams")], Type::Long, false),
            method("query", "ube_sql_query", vec![Type::String, object("SQLParams")], object("SQLResult"), false),
            method("begin", "ube_sql_begin", vec![], object("SQLTransaction"), false),
            method("close", "ube_sql_close", vec![], Type::Void, false),
        ],
        "SQLParams" => vec![
            method("create", "ube_sql_params_create", vec![], object("SQLParams"), true),
            method("addInt", "ube_sql_params_add_int", vec![Type::Int], Type::Void, false),
            method("addLong", "ube_sql_params_add_long", vec![Type::Long], Type::Void, false),
            method("addDecimal", "ube_sql_params_add_decimal", vec![Type::Decimal], Type::Void, false),
            method("addBool", "ube_sql_params_add_bool", vec![Type::Bool], Type::Void, false),
            method("addString", "ube_sql_params_add_string", vec![Type::String], Type::Void, false),
            method("addBlobHex", "ube_sql_params_add_blob_hex", vec![Type::String], Type::Void, false),
            method("addNull", "ube_sql_params_add_null", vec![Type::String], Type::Void, false),
            method("count", "ube_sql_params_count", vec![], Type::Int, false),
            method("clear", "ube_sql_params_clear", vec![], Type::Void, false),
            method("close", "ube_sql_params_close", vec![], Type::Void, false),
        ],
        "SQLTransaction" => vec![
            method("execute", "ube_sql_transaction_execute", vec![Type::String, object("SQLParams")], Type::Long, false),
            method("query", "ube_sql_transaction_query", vec![Type::String, object("SQLParams")], object("SQLResult"), false),
            method("savepoint", "ube_sql_transaction_savepoint", vec![Type::String], Type::Void, false),
            method("rollbackTo", "ube_sql_transaction_rollback_to", vec![Type::String], Type::Void, false),
            method("release", "ube_sql_transaction_release", vec![Type::String], Type::Void, false),
            method("commit", "ube_sql_transaction_commit", vec![], Type::Void, false),
            method("rollback", "ube_sql_transaction_rollback", vec![], Type::Void, false),
            method("close", "ube_sql_transaction_close", vec![], Type::Void, false),
        ],
        "SQLResult" => vec![
            method("next", "ube_sql_result_next", vec![], Type::Bool, false),
            method("reset", "ube_sql_result_reset", vec![], Type::Void, false),
            method("rowCount", "ube_sql_result_row_count", vec![], Type::Long, false),
            method("columnCount", "ube_sql_result_column_count", vec![], Type::Int, false),
            method("columnName", "ube_sql_result_column_name", vec![Type::Int], Type::String, false),
            method("columnIndex", "ube_sql_result_column_index", vec![Type::String], Type::Int, false),
            method("columnType", "ube_sql_result_column_type", vec![Type::Int], Type::String, false),
            method("isNull", "ube_sql_result_is_null", vec![Type::Int], Type::Bool, false),
            method("getInt", "ube_sql_result_get_int", vec![Type::Int], Type::Int, false),
            method("getLong", "ube_sql_result_get_long", vec![Type::Int], Type::Long, false),
            method("getDecimal", "ube_sql_result_get_decimal", vec![Type::Int], Type::Decimal, false),
            method("getBool", "ube_sql_result_get_bool", vec![Type::Int], Type::Bool, false),
            method("getString", "ube_sql_result_get_string", vec![Type::Int], Type::String, false),
            method("getBlobHex", "ube_sql_result_get_blob_hex", vec![Type::Int], Type::String, false),
            method("close", "ube_sql_result_close", vec![], Type::Void, false),
        ],
        _ => vec![],
    }
}

/// Environment access never exports decrypted vault values into the process environment.
pub fn env_methods() -> Vec<BuiltinMethod> {
    use crate::types::Type;
    vec![
        BuiltinMethod { name: "get", symbol: "ube_env_get", params: vec![Type::String], result: Type::Nullable(Box::new(Type::String)), is_static: true },
        BuiltinMethod { name: "require", symbol: "ube_env_require", params: vec![Type::String], result: Type::String, is_static: true },
        BuiltinMethod { name: "has", symbol: "ube_env_has", params: vec![Type::String], result: Type::Bool, is_static: true },
    ]
}

/// Immutable settled outcomes, separate from untyped collection payloads.
pub fn task_outcome_methods(class: &str) -> Vec<BuiltinMethod> {
    use crate::types::Type;
    let methods = match class {
        "TaskOutcomes" => vec![
            ("len", "ube_task_outcomes_len", vec![], Type::Int),
            ("get", "ube_task_outcomes_get", vec![Type::Int], Type::Class { name: "TaskOutcome".into() }),
        ],
        "TaskOutcome" => vec![
            ("succeeded", "ube_task_outcome_succeeded", vec![], Type::Bool),
            ("value", "ube_task_outcome_value", vec![], Type::Long),
            ("boolValue", "ube_task_outcome_bool", vec![], Type::Bool),
            ("decimalValue", "ube_task_outcome_decimal", vec![], Type::Decimal),
            ("stringValue", "ube_task_outcome_string", vec![], Type::String),
            ("error", "ube_task_outcome_error", vec![], Type::Nullable(Box::new(Type::Class { name: "Error".into() }))),
        ],
        _ => return vec![],
    };
    methods.into_iter().map(|(name, symbol, params, result)| BuiltinMethod { name, symbol, params, result, is_static: false }).collect()
}

/// Optional in-process management listener and bounded application measurements.
pub const OBSERVABILITY_CLASSES: &[&str] = &["Observability", "Metrics", "Health"];
pub fn observability_methods(class: &str) -> Vec<BuiltinMethod> {
    use crate::types::Type;
    let method = |name, symbol, params, result| BuiltinMethod { name, symbol, params, result, is_static: true };
    match class {
        "Observability" => vec![
            method("start", "ube_observability_start", vec![Type::Int], Type::Void),
            method("stop", "ube_observability_stop", vec![], Type::Void),
        ],
        "Metrics" => vec![
            method("increment", "ube_metrics_increment", vec![Type::String], Type::Void),
            method("add", "ube_metrics_add", vec![Type::String, Type::Decimal], Type::Void),
            method("gauge", "ube_metrics_gauge", vec![Type::String, Type::Decimal], Type::Void),
            method("record", "ube_metrics_record", vec![Type::String, Type::Decimal], Type::Void),
        ],
        "Health" => vec![method("ready", "ube_health_ready", vec![Type::Bool], Type::Void)],
        _ => vec![],
    }
}

/// Read-only command-line strings; unlike untyped List, indexing preserves string type.
pub fn console_args_methods() -> Vec<BuiltinMethod> {
    use crate::types::Type;
    vec![
        BuiltinMethod { name: "len", symbol: "ube_args_len", params: vec![], result: Type::Int, is_static: false },
        BuiltinMethod { name: "get", symbol: "ube_args_get", params: vec![Type::Int], result: Type::String, is_static: false },
    ]
}

/// Cache creation is explicit; memory, persistent, and remote backends share instance methods.
pub fn cache_methods() -> Vec<BuiltinMethod> {
    use crate::types::Type;
    let cache = || Type::Class { name: "Cache".into() };
    let method = |name, symbol, params, result, is_static| BuiltinMethod { name, symbol, params, result, is_static };
    vec![
        method("memory", "ube_cache_memory", vec![Type::Long], cache(), true),
        method("open", "ube_cache_open", vec![Type::String, Type::Long], cache(), true),
        method("openBuffered", "ube_cache_open_buffered", vec![Type::String, Type::Long], cache(), true),
        method("connect", "ube_cache_connect", vec![Type::String, Type::String], cache(), true),
        method("get", "ube_cache_get", vec![Type::String], Type::Nullable(Box::new(Type::String)), false),
        method("set", "ube_cache_set", vec![Type::String, Type::String, Type::Long], Type::Void, false),
        method("add", "ube_cache_add", vec![Type::String, Type::String, Type::Long], Type::Bool, false),
        method("increment", "ube_cache_increment", vec![Type::String, Type::Long], Type::Long, false),
        method("delete", "ube_cache_delete", vec![Type::String], Type::Bool, false),
        method("listen", "ube_cache_listen", vec![Type::String, Type::String], Type::Void, false),
        method("close", "ube_cache_close", vec![], Type::Void, false),
    ]
}

pub fn key_value_store_methods() -> Vec<BuiltinMethod> {
    use crate::types::Type;
    let store = || Type::Class { name: "KeyValueStore".into() };
    let method = |name, symbol, params, result, is_static| BuiltinMethod { name, symbol, params, result, is_static };
    vec![
        method("open", "ube_kv_open", vec![Type::String], store(), true),
        method("openBuffered", "ube_kv_open_buffered", vec![Type::String], store(), true),
        method("get", "ube_kv_get", vec![Type::String], Type::Nullable(Box::new(Type::String)), false),
        method("set", "ube_kv_set", vec![Type::String, Type::String], Type::Void, false),
        method("add", "ube_kv_add", vec![Type::String, Type::String], Type::Bool, false),
        method("has", "ube_kv_has", vec![Type::String], Type::Bool, false),
        method("delete", "ube_kv_delete", vec![Type::String], Type::Bool, false),
        method("increment", "ube_kv_increment", vec![Type::String, Type::Long], Type::Long, false),
        method("close", "ube_kv_close", vec![], Type::Void, false),
    ]
}

pub const XML_CLASSES: &[&str] = &["Xml", "XmlDocument", "XmlElement", "XmlOptions"];
pub fn xml_methods(class: &str) -> Vec<BuiltinMethod> {
    use crate::types::Type;
    let cls=|name:&str|Type::Class{name:name.into()};
    let method=|name,symbol,params,result,is_static|BuiltinMethod{name,symbol,params,result,is_static};
    match class {
        "Xml"=>vec![
            method("toObject","ube_xml_parse",vec![Type::String],cls("XmlDocument"),true),
            method("parse","ube_xml_parse",vec![Type::String],cls("XmlDocument"),true),
            method("create","ube_xml_create",vec![Type::String],cls("XmlDocument"),true),
            method("partialParse","ube_xml_parse",vec![Type::String,cls("XmlDocument")],cls("XmlDocument"),true),
            method("to","ube_xml_parse",vec![Type::String,cls("XmlDocument")],cls("XmlDocument"),true),
            method("stringify","ube_xmldocument_to_string",vec![cls("XmlDocument")],Type::String,true),
            method("from","ube_xmldocument_to_string",vec![cls("XmlDocument")],Type::String,true),
        ],
        "XmlDocument"=>vec![
            method("root","ube_xmldocument_root",vec![],cls("XmlElement"),false),
            method("toString","ube_xmldocument_to_string",vec![],Type::String,false),
        ],
        "XmlElement"=>vec![
            method("child","ube_xmlement_child",vec![Type::String],cls("XmlElement"),false),
            method("childAt","ube_xml_child_at",vec![Type::String,Type::Int],cls("XmlElement"),false),
            method("childCount","ube_xml_child_count",vec![Type::String],Type::Int,false),
            method("childNs","ube_xml_child_ns",vec![Type::String,Type::String],cls("XmlElement"),false),
            method("element","ube_xmlement_element",vec![Type::String],cls("XmlElement"),false),
            method("text","ube_xmlement_text",vec![Type::String],cls("XmlElement"),false),
            method("getText","ube_xmlement_get_text",vec![],Type::String,false),
            method("attribute","ube_xml_attribute",vec![Type::String,Type::String],cls("XmlElement"),false),
            method("getAttribute","ube_xml_get_attribute",vec![Type::String],Type::String,false),
            method("namespace","ube_xml_namespace",vec![Type::String,Type::String],cls("XmlElement"),false),
            method("name","ube_xml_name",vec![],Type::String,false),
            method("namespaceUri","ube_xml_namespace_uri",vec![],Type::String,false),
            method("isNull","ube_xml_is_null",vec![],Type::Bool,false),
        ],
        "XmlOptions"=>vec![
            method("root","ube_xml_options_root",vec![Type::String],cls("XmlOptions"),false),
            method("attribute","ube_xml_options_attribute",vec![Type::String],cls("XmlOptions"),false),
            method("rename","ube_xml_options_rename",vec![Type::String,Type::String],cls("XmlOptions"),false),
            method("namespace","ube_xml_options_namespace",vec![Type::String,Type::String],cls("XmlOptions"),false),
            method("listItem","ube_xml_options_item",vec![Type::String],cls("XmlOptions"),false),
        ],
        _=>vec![],
    }
}
