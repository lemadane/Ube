//! One ABI table drives checking, native declarations, and lowering.
use crate::{builtins::BuiltinMethod, types::Type};
pub const CLASSES: &[&str] = &["RestInPeace", "RestInPeaceRequest", "RestInPeaceResponse"];
pub fn methods(class: &str) -> Vec<BuiltinMethod> {
    let request = Type::Class {
        name: "RestInPeaceRequest".into(),
    };
    let response = Type::Class {
        name: "RestInPeaceResponse".into(),
    };
    let mut methods = Vec::new();
    let mut add = |name, symbol, params, result, is_static| {
        methods.push(BuiltinMethod {
            name,
            symbol,
            params,
            result,
            is_static,
        })
    };
    match class {
        "RestInPeace" => {
            add("given", "ube_rest_given", vec![], request.clone(), true);
            add("at", "ube_rest_at", vec![Type::String], request, true);
        }
        "RestInPeaceRequest" => {
            for (name, symbol) in [
                ("baseUrl", "ube_rest_base_url"),
                ("caFile", "ube_rest_ca_file"),
                ("body", "ube_rest_body"),
                ("json", "ube_rest_json"),
                ("contentType", "ube_rest_content_type"),
                ("accept", "ube_rest_accept"),
                ("bearer", "ube_rest_bearer"),
            ] {
                add(name, symbol, vec![Type::String], request.clone(), false);
            }
            for (name, symbol) in [("header", "ube_rest_header"), ("query", "ube_rest_query")] {
                add(name, symbol, vec![Type::String; 2], request.clone(), false);
            }
            for (name, symbol) in [
                ("timeout", "ube_rest_timeout"),
                ("maxBytes", "ube_rest_max_bytes"),
            ] {
                add(name, symbol, vec![Type::Int], request.clone(), false);
            }
            for (name, symbol) in [
                ("get", "ube_rest_get"),
                ("post", "ube_rest_post"),
                ("put", "ube_rest_put"),
                ("patch", "ube_rest_patch"),
                ("delete", "ube_rest_delete"),
                ("head", "ube_rest_head"),
                ("options", "ube_rest_options"),
            ] {
                add(name, symbol, vec![Type::String], response.clone(), false);
            }
            add(
                "send",
                "ube_rest_send",
                vec![Type::String; 2],
                response,
                false,
            );
            add("close", "ube_rest_request_close", vec![], Type::Void, false);
        }
        "RestInPeaceResponse" => {
            add("status", "ube_rest_status", vec![], Type::Int, false);
            add(
                "body",
                "ube_rest_response_body",
                vec![],
                Type::String,
                false,
            );
            add(
                "header",
                "ube_rest_response_header",
                vec![Type::String],
                Type::Nullable(Box::new(Type::String)),
                false,
            );
            add(
                "hasHeader",
                "ube_rest_has_header",
                vec![Type::String],
                Type::Bool,
                false,
            );
            add(
                "elapsedMs",
                "ube_rest_elapsed_ms",
                vec![],
                Type::Long,
                false,
            );
            add("protocol", "ube_rest_protocol", vec![], Type::String, false);
            for (name, symbol, result) in [
                ("jsonString", "ube_rest_json_string", Type::String),
                ("jsonInt", "ube_rest_json_int", Type::Int),
                ("jsonLong", "ube_rest_json_long", Type::Long),
                ("jsonDecimal", "ube_rest_json_decimal", Type::Decimal),
                ("jsonBool", "ube_rest_json_bool", Type::Bool),
                ("jsonIsNull", "ube_rest_json_is_null", Type::Bool),
            ] {
                add(name, symbol, vec![Type::String], result, false);
            }
            add(
                "expectStatus",
                "ube_rest_expect_status",
                vec![Type::Int],
                response.clone(),
                false,
            );
            add(
                "expectBody",
                "ube_rest_expect_body",
                vec![Type::String],
                response.clone(),
                false,
            );
            add(
                "expectHeader",
                "ube_rest_expect_header",
                vec![Type::String; 2],
                response.clone(),
                false,
            );
            add(
                "expectJsonString",
                "ube_rest_expect_json_string",
                vec![Type::String; 2],
                response,
                false,
            );
            add(
                "close",
                "ube_rest_response_close",
                vec![],
                Type::Void,
                false,
            );
        }
        _ => {}
    }
    methods
}
