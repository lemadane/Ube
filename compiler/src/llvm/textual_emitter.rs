use crate::ir::nodes::*;
use crate::parser::BinaryOp;
use crate::types::Type;
use std::collections::{HashMap, HashSet};

pub struct TextualLlvmEmitter {
    output: String,
    string_constants: HashMap<String, String>,
    string_counter: usize,
    class_map: HashMap<String, IrClass>,
    current_return_type: Type,
}

impl TextualLlvmEmitter {
    pub fn new() -> Self {
        Self {
            output: String::new(),
            string_constants: HashMap::new(),
            string_counter: 0,
            class_map: HashMap::new(),
            current_return_type: Type::Void,
        }
    }

    pub fn emit_program(&mut self, ir_program: &IrProgram) -> String {
        self.output.clear();
        self.class_map = ir_program.classes.iter().map(|c| (c.name.clone(), c.clone())).collect();

        // Target Data Layout
        self.output.push_str("; Target Data Layout for Talas Native Binary\n");
        self.output.push_str("target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128\"\n\n");

        // External runtime declarations
        self.output.push_str("declare void @talas_runtime_init()\n");
        self.output.push_str("declare void @talas_runtime_cleanup()\n");
        self.output.push_str("declare i8* @talas_alloc(i64, i64)\n");
        self.output.push_str("declare void @talas_print_str(i8*)\n");
        self.output.push_str("declare void @talas_print_i64(i64)\n");
        self.output.push_str("declare void @talas_print_f64(double)\n");
        self.output.push_str("declare void @talas_print_bool(i1)\n");
        self.output.push_str("declare i8* @talas_string_concat(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_list_create()\n");
        self.output.push_str("declare void @talas_list_add(i8*, i64)\n");
        self.output.push_str("declare i64 @talas_list_get(i8*, i64)\n");
        self.output.push_str("declare i64 @talas_list_len(i8*)\n");
        self.output.push_str("declare i8* @talas_map_create()\n");
        self.output.push_str("declare void @talas_map_put(i8*, i64, i64)\n");
        self.output.push_str("declare i64 @talas_map_get(i8*, i64)\n");
        self.output.push_str("declare i64 @talas_map_len(i8*)\n");
        self.output.push_str("declare i8* @talas_task_create(i64)\n");
        self.output.push_str("declare i64 @talas_task_await(i8*)\n");
        self.output.push_str("declare void @talas_task_yield()\n");
        self.output.push_str("declare i8* @talas_task_all(i8*)\n");
        self.output.push_str("declare i64 @talas_task_any(i8*)\n");
        self.output.push_str("declare i64 @talas_task_race(i8*)\n");
        self.output.push_str("declare i8* @talas_task_all_settled(i8*)\n");
        self.output.push_str("declare i1 @talas_task_cancel(i8*)\n");
        self.output.push_str("declare i64 @talas_task_await_timeout_cancel(i8*, i64)\n");
        self.output.push_str("declare i8* @talas_channel_create()\n");
        self.output.push_str("declare void @talas_channel_send(i8*, i64)\n");
        self.output.push_str("declare i64 @talas_channel_recv(i8*)\n");
        self.output.push_str("declare i8* @talas_datetime_now()\n");
        self.output.push_str("declare i8* @talas_datetime_utc()\n");
        self.output.push_str("declare i8* @talas_datetime_from_unix(i64)\n");
        self.output.push_str("declare i8* @talas_datetime_from_unix_millis(i64)\n");
        self.output.push_str("declare i8* @talas_datetime_of(i64, i64, i64, i64, i64)\n");
        self.output.push_str("declare i8* @talas_datetime_date(i64, i64, i64)\n");
        self.output.push_str("declare i8* @talas_datetime_time(i64, i64)\n");
        self.output.push_str("declare i8* @talas_datetime_parse(i8*)\n");
        self.output.push_str("declare i64 @talas_datetime_year(i8*)\n");
        self.output.push_str("declare i64 @talas_datetime_month(i8*)\n");
        self.output.push_str("declare i64 @talas_datetime_day(i8*)\n");
        self.output.push_str("declare i64 @talas_datetime_hour(i8*)\n");
        self.output.push_str("declare i64 @talas_datetime_minute(i8*)\n");
        self.output.push_str("declare i64 @talas_datetime_second(i8*)\n");
        self.output.push_str("declare i64 @talas_datetime_timestamp(i8*)\n");
        self.output.push_str("declare i64 @talas_datetime_unix(i8*)\n");
        self.output.push_str("declare i64 @talas_datetime_unix_millis(i8*)\n");
        self.output.push_str("declare i64 @talas_datetime_unix_nanos(i8*)\n");
        self.output.push_str("declare i8* @talas_datetime_zone(i8*)\n");
        self.output.push_str("declare i8* @talas_datetime_in_zone(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_datetime_add_seconds(i8*, i64)\n");
        self.output.push_str("declare i64 @talas_datetime_diff_seconds(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_datetime_format(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_duration_create(i64)\n");
        self.output.push_str("declare i64 @talas_duration_total_seconds(i8*)\n");
        self.output.push_str("declare i8* @talas_currency_create(i8*)\n");
        self.output.push_str("declare i8* @talas_currency_code(i8*)\n");
        self.output.push_str("declare i8* @talas_money_create(double, i8*)\n");
        self.output.push_str("declare i8* @talas_money_php(double)\n");
        self.output.push_str("declare i8* @talas_money_usd(double)\n");
        self.output.push_str("declare i8* @talas_money_eur(double)\n");
        self.output.push_str("declare i8* @talas_money_jpy(double)\n");
        self.output.push_str("declare i8* @talas_money_gbp(double)\n");
        self.output.push_str("declare double @talas_money_amount(i8*)\n");
        self.output.push_str("declare i8* @talas_money_currency(i8*)\n");
        self.output.push_str("declare i8* @talas_money_add(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_money_subtract(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_money_multiply_percent(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_exchange_convert(i8*, i8*, double)\n");
        self.output.push_str("declare i8* @talas_percent_create(double)\n");
        self.output.push_str("declare double @talas_percent_value(i8*)\n");
        self.output.push_str("declare i8* @talas_rate_create(double)\n");
        self.output.push_str("declare double @talas_rate_value(i8*)\n");
        self.output.push_str("declare i8* @talas_geopoint_create(double, double)\n");
        self.output.push_str("declare double @talas_geopoint_latitude(i8*)\n");
        self.output.push_str("declare double @talas_geopoint_longitude(i8*)\n");
        self.output.push_str("declare i8* @talas_geopoint_distance_to(i8*, i8*)\n");
        self.output.push_str("declare double @talas_geopoint_bearing_to(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_distance_create_meters(double)\n");
        self.output.push_str("declare i8* @talas_distance_create_km(double)\n");
        self.output.push_str("declare i8* @talas_distance_create_miles(double)\n");
        self.output.push_str("declare double @talas_distance_meters(i8*)\n");
        self.output.push_str("declare double @talas_distance_km(i8*)\n");
        self.output.push_str("declare double @talas_distance_miles(i8*)\n");
        self.output.push_str("declare i8* @talas_geobounds_create(i8*, i8*)\n");
        self.output.push_str("declare i1 @talas_geobounds_contains(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_geolocation_current()\n");
        self.output.push_str("declare double @talas_math_pi()\n");
        self.output.push_str("declare double @talas_math_e()\n");
        self.output.push_str("declare double @talas_math_tau()\n");
        self.output.push_str("declare double @talas_math_abs(double)\n");
        self.output.push_str("declare double @talas_math_min(double, double)\n");
        self.output.push_str("declare double @talas_math_max(double, double)\n");
        self.output.push_str("declare double @talas_math_sqrt(double)\n");
        self.output.push_str("declare double @talas_math_cbrt(double)\n");
        self.output.push_str("declare double @talas_math_pow(double, double)\n");
        self.output.push_str("declare double @talas_math_floor(double)\n");
        self.output.push_str("declare double @talas_math_ceil(double)\n");
        self.output.push_str("declare double @talas_math_round(double)\n");
        self.output.push_str("declare double @talas_math_round_decimals(double, i64)\n");
        self.output.push_str("declare double @talas_math_truncate(double)\n");
        self.output.push_str("declare double @talas_math_clamp(double, double, double)\n");
        self.output.push_str("declare i64 @talas_math_sign(double)\n");
        self.output.push_str("declare double @talas_math_sin(double)\n");
        self.output.push_str("declare double @talas_math_cos(double)\n");
        self.output.push_str("declare double @talas_math_tan(double)\n");
        self.output.push_str("declare double @talas_math_asin(double)\n");
        self.output.push_str("declare double @talas_math_acos(double)\n");
        self.output.push_str("declare double @talas_math_atan(double)\n");
        self.output.push_str("declare double @talas_math_atan2(double, double)\n");
        self.output.push_str("declare double @talas_math_radians(double)\n");
        self.output.push_str("declare double @talas_math_degrees(double)\n");
        self.output.push_str("declare double @talas_math_log(double)\n");
        self.output.push_str("declare double @talas_math_log10(double)\n");
        self.output.push_str("declare double @talas_math_log2(double)\n");
        self.output.push_str("declare double @talas_math_exp(double)\n");
        self.output.push_str("declare i1 @talas_math_is_nan(double)\n");
        self.output.push_str("declare i1 @talas_math_is_infinite(double)\n");
        self.output.push_str("declare i1 @talas_math_is_finite(double)\n");
        self.output.push_str("declare i1 @talas_math_is_even(i64)\n");
        self.output.push_str("declare i1 @talas_math_is_odd(i64)\n");
        self.output.push_str("declare i64 @talas_math_gcd(i64, i64)\n");
        self.output.push_str("declare i64 @talas_math_lcm(i64, i64)\n");
        self.output.push_str("declare i64 @talas_random_int(i64, i64)\n");
        self.output.push_str("declare double @talas_random_decimal()\n");
        self.output.push_str("declare i1 @talas_random_bool()\n");
        self.output.push_str("declare i8* @talas_random_uuid()\n");
        self.output.push_str("declare i8* @talas_random_bytes(i64)\n");
        self.output.push_str("declare i8* @talas_securerandom_bytes(i64)\n");
        self.output.push_str("declare double @talas_statistics_mean(i8*)\n");
        self.output.push_str("declare double @talas_statistics_median(i8*)\n");
        self.output.push_str("declare double @talas_statistics_variance(i8*)\n");
        self.output.push_str("declare double @talas_statistics_std_dev(i8*)\n");
        self.output.push_str("declare double @talas_statistics_percentile(i8*, double)\n");
        self.output.push_str("declare double @talas_statistics_covariance(i8*, i8*)\n");
        self.output.push_str("declare double @talas_statistics_correlation(i8*, i8*)\n");
        self.output.push_str("declare i64 @talas_probability_factorial(i64)\n");
        self.output.push_str("declare i64 @talas_probability_permutations(i64, i64)\n");
        self.output.push_str("declare i64 @talas_probability_combinations(i64, i64)\n");
        self.output.push_str("declare double @talas_probability_normal(double, double, double)\n");
        self.output.push_str("declare double @talas_probability_binomial(i64, i64, double)\n");
        self.output.push_str("declare double @talas_probability_poisson(i64, double)\n");
        self.output.push_str("declare double @talas_probability_uniform(double, double, double)\n");
        self.output.push_str("declare i8* @talas_file_open(i8*)\n");
        self.output.push_str("declare i8* @talas_file_read(i8*)\n");
        self.output.push_str("declare void @talas_file_write(i8*, i8*)\n");
        self.output.push_str("declare void @talas_file_append(i8*, i8*)\n");
        self.output.push_str("declare void @talas_file_delete(i8*)\n");
        self.output.push_str("declare i1 @talas_file_exists(i8*)\n");
        self.output.push_str("declare void @talas_file_close(i8*)\n");
        self.output.push_str("declare i8* @talas_file_info(i8*)\n");
        self.output.push_str("declare i64 @talas_fileinfo_size(i8*)\n");
        self.output.push_str("declare i8* @talas_fileinfo_created_at(i8*)\n");
        self.output.push_str("declare i8* @talas_fileinfo_modified_at(i8*)\n");
        self.output.push_str("declare i1 @talas_fileinfo_is_directory(i8*)\n");
        self.output.push_str("declare void @talas_directory_create(i8*)\n");
        self.output.push_str("declare void @talas_directory_delete(i8*)\n");
        self.output.push_str("declare i8* @talas_directory_list(i8*)\n");
        self.output.push_str("declare i8* @talas_path_join(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_path_filename(i8*)\n");
        self.output.push_str("declare i8* @talas_path_extension(i8*)\n");
        self.output.push_str("declare i8* @talas_server_create()\n");
        self.output.push_str("declare void @talas_server_start(i8*, i32)\n");
        self.output.push_str("declare void @talas_server_stop(i8*)\n");
        self.output.push_str("declare i32 @talas_server_port(i8*)\n");
        self.output.push_str("declare void @talas_server_tls(i8*, i8*, i8*)\n");
        self.output.push_str("declare void @talas_server_route(i8*, i8*, i64)\n");
        self.output.push_str("declare void @talas_server_use(i8*, i64)\n");
        self.output.push_str("declare i8* @talas_request_path(i8*, i8*)\n");
        self.output.push_str("declare i32 @talas_request_path_int(i8*, i8*)\n");
        self.output.push_str("declare i64 @talas_request_path_long(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_request_query(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_request_header(i8*, i8*)\n");
        self.output.push_str("declare i1 @talas_request_has_bearer_token(i8*)\n");
        self.output.push_str("declare i8* @talas_response_create(i32, i8*)\n");
        self.output.push_str("declare i8* @talas_cookie_create(i8*, i8*)\n");
        self.output.push_str("declare void @talas_websocket_send(i8*, i8*)\n");
        self.output.push_str("declare void @talas_websocket_on_message(i8*, i64)\n");
        self.output.push_str("declare void @talas_server_enable_http2(i8*)\n");
        self.output.push_str("declare void @talas_server_enable_http3(i8*, i32)\n");
        self.output.push_str("declare i8* @talas_request_protocol(i8*)\n");
        self.output.push_str("declare i1 @talas_request_is_http3(i8*)\n");
        self.output.push_str("declare i8* @talas_bytes_create(i64)\n");
        self.output.push_str("declare void @talas_bytes_free(i8*)\n");
        self.output.push_str("declare i8* @talas_file_open_read(i8*)\n");
        self.output.push_str("declare i8* @talas_file_open_write(i8*)\n");
        self.output.push_str("declare i64 @talas_stream_read(i8*, i8*, i64, i64)\n");
        self.output.push_str("declare i64 @talas_stream_write(i8*, i8*, i64, i64)\n");
        self.output.push_str("declare i64 @talas_stream_pipe_to(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_file_sha256(i8*)\n");
        self.output.push_str("declare i8* @talas_response_stream(i32, i8*)\n");
        self.output.push_str("declare i8* @talas_response_file(i8*)\n");
        self.output.push_str("declare i8* @talas_json_parse(i8*)\n");

        self.output.push_str("declare i8* @talas_json_stringify(i8*)\n");
        self.output.push_str("declare i8* @talas_jsonobject_get_string(i8*, i8*)\n");
        self.output.push_str("declare i32 @talas_jsonobject_get_int(i8*, i8*)\n");
        self.output.push_str("declare i64 @talas_jsonobject_get_long(i8*, i8*)\n");
        self.output.push_str("declare double @talas_jsonobject_get_decimal(i8*, i8*)\n");
        self.output.push_str("declare i1 @talas_jsonobject_get_bool(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_xml_parse(i8*)\n");
        self.output.push_str("declare i8* @talas_xml_create(i8*)\n");
        self.output.push_str("declare i8* @talas_xmldocument_root(i8*)\n");
        self.output.push_str("declare i8* @talas_xmldocument_to_string(i8*)\n");
        self.output.push_str("declare i8* @talas_xmlement_child(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_xmlement_element(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_xmlement_text(i8*, i8*)\n");
        self.output.push_str("declare i8* @talas_xmlement_get_text(i8*)\n\n");

        let mut declared_types: HashSet<String> = ir_program.classes.iter().map(|c| c.name.clone()).collect();

        // Object layouts for concrete classes
        for cls in &ir_program.classes {
            let field_types = cls
                .fields
                .iter()
                .map(|(_, ty)| self.type_to_llvm(ty))
                .collect::<Vec<_>>()
                .join(", ");
            self.output.push_str(&format!("%{} = type {{ {} }}\n", cls.name, field_types));
        }

        let builtins = vec![
            ("GenericError", "i8*"),
            ("OutOfBoundsError", "i8*"),
            ("Error", "i8*"),
            ("Task", "i8*"),
            ("Channel", "i8*"),
            ("List", "i8*"),
            ("Map", "i8*"),
            ("DateTime", "i64, i32, i8*"),
            ("Duration", "i64"),
            ("Currency", "i8*"),
            ("Money", "double, i8*"),
            ("Exchange", "i8*"),
            ("Percent", "double"),
            ("Rate", "double"),
            ("GeoPoint", "double, double"),
            ("Distance", "double"),
            ("GeoBounds", "i8*, i8*"),
            ("Geolocation", "i8*"),
            ("LocationPermissionError", "i8*"),
            ("LocationUnavailableError", "i8*"),
            ("Math", "i8*"),
            ("Random", "i8*"),
            ("SecureRandom", "i8*"),
            ("Statistics", "i8*"),
            ("Probability", "i8*"),
            ("File", "i8*, i1"),
            ("FileInfo", "i64, i8*, i8*, i1"),
            ("Directory", "i8*"),
            ("Path", "i8*"),
            ("FileError", "i8*"),
            ("FileNotFoundError", "i8*"),
            ("PermissionError", "i8*"),
            ("AlreadyExistsError", "i8*"),
            ("InvalidPathError", "i8*"),
            ("DiskFullError", "i8*"),
            ("Server", "i32, i1"),
            ("Request", "i8*, i8*"),
            ("Response", "i32, i8*"),
            ("Cookie", "i8*, i8*"),
            ("WebSocket", "i64"),
            ("Json", "i8*"),
            ("JsonObject", "i64"),
            ("JsonArray", "i64"),
            ("Xml", "i8*"),
            ("XmlDocument", "i64"),
            ("XmlElement", "i64"),
            ("JsonError", "i8*"),
            ("JsonParseError", "i8*"),
            ("MissingFieldError", "i8*"),
            ("InvalidTypeError", "i8*"),
            ("XmlError", "i8*"),
            ("XmlParseError", "i8*"),
        ];

        for (name, fields) in builtins {
            if !declared_types.contains(name) {
                self.output.push_str(&format!("%{} = type {{ {} }}\n", name, fields));
                declared_types.insert(name.to_string());
            }
        }

        // Object layouts for abstract trait types referenced in parameters/receivers
        for func in &ir_program.functions {
            for (_, ty) in &func.params {
                if let Type::Class { name } = ty {
                    if !declared_types.contains(name) {
                        self.output.push_str(&format!("%{} = type {{ }}\n", name));
                        declared_types.insert(name.clone());
                    }
                }
            }
        }
        self.output.push('\n');

        // Emit functions
        for func in &ir_program.functions {
            let func_str = self.emit_function(func);
            self.output.push_str(&func_str);
            self.output.push('\n');
        }

        // Emit synthetic C-ABI main entry point if any class contains a main() method
        let main_fn = ir_program
            .functions
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case("main"));

        if let Some(m_fn) = main_fn {
            let target_symbol = format!("{}_{}", m_fn.class_name, m_fn.name);
            let class_type = format!("%{}", m_fn.class_name);
            self.output.push_str("define i32 @main(i32 %argc, i8** %argv) {\n");
            self.output.push_str("entry:\n");
            self.output.push_str("  call void @talas_runtime_init()\n");
            self.output.push_str(&format!("  %sz_ptr = getelementptr {}, {}* null, i32 1\n", class_type, class_type));
            self.output.push_str(&format!("  %app_sz = ptrtoint {}* %sz_ptr to i64\n", class_type));
            self.output.push_str("  %raw_mem = call i8* @talas_alloc(i64 %app_sz, i64 8)\n");
            self.output.push_str(&format!("  %app = bitcast i8* %raw_mem to {}*\n", class_type));

            if let Some(cls) = ir_program.classes.iter().find(|c| c.name == m_fn.class_name) {
                for (i, (field_name, field_ty)) in cls.fields.iter().enumerate() {
                    let field_ty_str = self.type_to_llvm(field_ty);
                    self.output.push_str(&format!("  %app_gep_{} = getelementptr inbounds {}, {}* %app, i32 0, i32 {}\n", i, class_type, class_type, i));
                    if field_name.contains("port") || field_ty == &Type::Int {
                        self.output.push_str(&format!("  store i32 8080, i32* %app_gep_{}, align 8\n", i));
                    } else if field_ty == &Type::Long {
                        self.output.push_str(&format!("  store i64 0, i64* %app_gep_{}, align 8\n", i));
                    } else if field_ty == &Type::Decimal {
                        self.output.push_str(&format!("  store double 0.0, double* %app_gep_{}, align 8\n", i));
                    } else if field_ty == &Type::Bool {
                        self.output.push_str(&format!("  store i1 false, i1* %app_gep_{}, align 8\n", i));
                    } else {
                        self.output.push_str(&format!("  store {} null, {}* %app_gep_{}, align 8\n", field_ty_str, field_ty_str, i));
                    }
                }
            }

            self.output.push_str(&format!("  call void @{}({}* %app)\n", target_symbol, class_type));
            self.output.push_str("  call void @talas_runtime_cleanup()\n");
            self.output.push_str("  ret i32 0\n");
            self.output.push_str("}\n\n");
        }

        // Global string constants
        let constants = self.string_constants.clone();
        for (label, str_val) in &constants {
            let bytes_len = str_val.as_bytes().len() + 1;
            let escaped = self.escape_llvm_string(str_val);
            self.output.push_str(&format!(
                "@{} = private unnamed_addr constant [{} x i8] c\"{}\", align 1\n",
                label,
                bytes_len,
                escaped
            ));
        }

        self.output.clone()
    }

    fn escape_llvm_string(&self, s: &str) -> String {
        let mut escaped = String::new();
        for b in s.as_bytes() {
            match b {
                b'\\' => escaped.push_str("\\5C"),
                b'"' => escaped.push_str("\\22"),
                b'\n' => escaped.push_str("\\0A"),
                b'\r' => escaped.push_str("\\0D"),
                b'\t' => escaped.push_str("\\09"),
                32..=126 => escaped.push(*b as char),
                _ => escaped.push_str(&format!("\\{:02X}", b)),
            }
        }
        escaped.push_str("\\00");
        escaped
    }

    fn emit_function(&mut self, func: &IrFunction) -> String {
        self.current_return_type = func.return_type.clone();
        let mut buf = String::new();
        let ret_ty_str = self.type_to_llvm(&func.return_type);
        let fn_name = format!("{}_{}", func.class_name, func.name);
        let receiver_str = format!("{} %{}", self.type_to_llvm(&func.receiver.1), func.receiver.0.0);

        let other_params = func
            .params
            .iter()
            .map(|(reg, ty)| format!("{} %{}", self.type_to_llvm(ty), reg.0))
            .collect::<Vec<_>>();

        let all_params = if other_params.is_empty() {
            receiver_str
        } else {
            format!("{}, {}", receiver_str, other_params.join(", "))
        };

        buf.push_str(&format!("define {} @{}({}) {{\n", ret_ty_str, fn_name, all_params));

        for block in &func.blocks {
            buf.push_str(&format!("{}:\n", block.id));
            for instr in &block.instructions {
                buf.push_str(&format!("  {}\n", self.emit_instruction(instr)));
            }
        }

        buf.push_str("}\n");
        buf
    }

    fn emit_instruction(&mut self, instr: &IrInstruction) -> String {
        match instr {
            IrInstruction::Alloca { dest, ty } => {
                let mut ty_str = self.type_to_llvm(ty);
                if ty_str == "void" {
                    ty_str = "i8*".to_string();
                }
                format!("%{} = alloca {}, align 8", dest.0, ty_str)
            }
            IrInstruction::Load { dest, ptr, ty } => {
                let ty_str = self.type_to_llvm(ty);
                format!("%{} = load {}, {}* %{}, align 8", dest.0, ty_str, ty_str, ptr.0)
            }
            IrInstruction::Store { val, ptr } => {
                let val_ty = val.get_type();
                let ty_str = self.type_to_llvm(&val_ty);
                let val_str = self.emit_operand(val);
                format!("store {} {}, {}* %{}, align 8", ty_str, val_str, ty_str, ptr.0)
            }
            IrInstruction::GetField { dest, ptr, class_name, field_index, field_ty } => {
                let class_type = format!("%{}", class_name);
                let field_ty_str = self.type_to_llvm(field_ty);
                let gep_reg = format!("gep_{}_{}", ptr.0, self.string_counter);
                self.string_counter += 1;
                format!(
                    "%{} = getelementptr inbounds {}, {}* %{}, i32 0, i32 {}\n  %{} = load {}, {}* %{}, align 8",
                    gep_reg, class_type, class_type, ptr.0, field_index,
                    dest.0, field_ty_str, field_ty_str, gep_reg
                )
            }
            IrInstruction::StoreField { ptr, class_name, field_index, val } => {
                let class_type = format!("%{}", class_name);
                let val_ty = val.get_type();
                let val_ty_str = self.type_to_llvm(&val_ty);
                let val_str = self.emit_operand(val);
                let gep_reg = format!("gep_store_{}_{}", ptr.0, self.string_counter);
                self.string_counter += 1;
                format!(
                    "%{} = getelementptr inbounds {}, {}* %{}, i32 0, i32 {}\n  store {} {}, {}* %{}, align 8",
                    gep_reg, class_type, class_type, ptr.0, field_index,
                    val_ty_str, val_str, val_ty_str, gep_reg
                )
            }
            IrInstruction::Binary { dest, op, left, right, operand_ty, result_ty: _ } => {
                let ty_str = self.type_to_llvm(operand_ty);
                let l_str = self.emit_operand(left);
                let r_str = self.emit_operand(right);
                match op {
                    BinaryOp::Nand => {
                        let tmp = format!("tmp_nand_{}", self.string_counter);
                        self.string_counter += 1;
                        format!(
                            "%{} = and i1 {}, {}\n  %{} = xor i1 %{}, true",
                            tmp, l_str, r_str, dest.0, tmp
                        )
                    }
                    BinaryOp::Nor => {
                        let tmp = format!("tmp_nor_{}", self.string_counter);
                        self.string_counter += 1;
                        format!(
                            "%{} = or i1 {}, {}\n  %{} = xor i1 %{}, true",
                            tmp, l_str, r_str, dest.0, tmp
                        )
                    }
                    BinaryOp::Xnor => {
                        let tmp = format!("tmp_xnor_{}", self.string_counter);
                        self.string_counter += 1;
                        format!(
                            "%{} = xor i1 {}, {}\n  %{} = xor i1 %{}, true",
                            tmp, l_str, r_str, dest.0, tmp
                        )
                    }
                    BinaryOp::NullCoalescing => {
                        let tmp_cond = format!("tmp_nc_{}", self.string_counter);
                        self.string_counter += 1;
                        format!(
                            "%{} = icmp ne {} {}, null\n  %{} = select i1 %{}, {} {}, {} {}",
                            tmp_cond, ty_str, l_str, dest.0, tmp_cond, ty_str, l_str, ty_str, r_str
                        )
                    }
                    _ => {
                        let op_str = match op {
                            BinaryOp::Add => if operand_ty == &Type::Decimal { "fadd" } else { "add" },
                            BinaryOp::Sub => if operand_ty == &Type::Decimal { "fsub" } else { "sub" },
                            BinaryOp::Mul => if operand_ty == &Type::Decimal { "fmul" } else { "mul" },
                            BinaryOp::Div => if operand_ty == &Type::Decimal { "fdiv" } else { "sdiv" },
                            BinaryOp::Mod => if operand_ty == &Type::Decimal { "frem" } else { "srem" },
                            BinaryOp::Equal => if operand_ty == &Type::Decimal { "fcmp oeq" } else { "icmp eq" },
                            BinaryOp::NotEqual => if operand_ty == &Type::Decimal { "fcmp one" } else { "icmp ne" },
                            BinaryOp::Less => if operand_ty == &Type::Decimal { "fcmp olt" } else { "icmp slt" },
                            BinaryOp::LessEqual => if operand_ty == &Type::Decimal { "fcmp ole" } else { "icmp sle" },
                            BinaryOp::Greater => if operand_ty == &Type::Decimal { "fcmp ogt" } else { "icmp sgt" },
                            BinaryOp::GreaterEqual => if operand_ty == &Type::Decimal { "fcmp oge" } else { "icmp sge" },
                            BinaryOp::And => "and",
                            BinaryOp::Or => "or",
                            BinaryOp::Xor => "xor",
                            other => panic!("Unsupported binary operator lowering: {:?}", other),
                        };
                        format!("%{} = {} {} {}, {}", dest.0, op_str, ty_str, l_str, r_str)
                    }
                }
            }
            IrInstruction::InstantiateClass { dest, class_name, args } => {
                let class_type = format!("%{}", class_name);
                let bitcast_reg = format!("obj_{}_{}", dest.0, self.string_counter);
                let sz_ptr = format!("sz_ptr_{}", self.string_counter);
                let sz_val = format!("sz_val_{}", self.string_counter);
                self.string_counter += 1;

                let mut instrs = Vec::new();
                instrs.push(format!("%{} = getelementptr {}, {}* null, i32 1", sz_ptr, class_type, class_type));
                instrs.push(format!("%{} = ptrtoint {}* %{} to i64", sz_val, class_type, sz_ptr));
                instrs.push(format!("%{} = call i8* @talas_alloc(i64 %{}, i64 8)", dest.0, sz_val));
                instrs.push(format!("%{} = bitcast i8* %{} to {}*", bitcast_reg, dest.0, class_type));

                let field_types: Vec<Type> = self
                    .class_map
                    .get(class_name)
                    .map(|c| c.fields.iter().map(|(_, ty)| ty.clone()).collect())
                    .unwrap_or_default();

                for (i, arg) in args.iter().enumerate() {
                    let field_ty = field_types.get(i).cloned().unwrap_or_else(|| arg.get_type());
                    let field_ty_str = self.type_to_llvm(&field_ty);
                    let gep_reg = format!("gep_inst_{}_{}", dest.0, i);
                    let val_str = self.emit_operand(arg);

                    instrs.push(format!("%{} = getelementptr inbounds {}, {}* %{}, i32 0, i32 {}", gep_reg, class_type, class_type, bitcast_reg, i));

                    if field_ty == Type::Long && arg.get_type() == Type::Int {
                        let ext_reg = format!("ext_inst_{}_{}", dest.0, i);
                        instrs.push(format!("%{} = sext i32 {} to i64", ext_reg, val_str));
                        instrs.push(format!("store i64 %{}, i64* %{}, align 8", ext_reg, gep_reg));
                    } else {
                        instrs.push(format!("store {} {}, {}* %{}, align 8", field_ty_str, val_str, field_ty_str, gep_reg));
                    }
                }
                instrs.join("\n  ")
            }
            IrInstruction::CallMethod { dest, class_name, method_name, receiver, args, ret_ty } => {
                let fn_symbol = format!("{}_{}", class_name, method_name);
                let ret_ty_str = self.type_to_llvm(ret_ty);
                let rx_str = self.emit_operand(receiver);
                let rx_ty_str = self.type_to_llvm(&receiver.get_type());

                let formatted_args = args
                    .iter()
                    .map(|a| format!("{} {}", self.type_to_llvm(&a.get_type()), self.emit_operand(a)))
                    .collect::<Vec<_>>();

                let all_args = if formatted_args.is_empty() {
                    format!("{} {}", rx_ty_str, rx_str)
                } else {
                    format!("{} {}, {}", rx_ty_str, rx_str, formatted_args.join(", "))
                };

                if ret_ty == &Type::Void || ret_ty_str == "void" {
                    format!("call void @{}({})", fn_symbol, all_args)
                } else if let Some(r) = dest {
                    format!("%{} = call {} @{}({})", r.0, ret_ty_str, fn_symbol, all_args)
                } else {
                    format!("call {} @{}({})", ret_ty_str, fn_symbol, all_args)
                }
            }
            IrInstruction::CallRuntime { dest, symbol, args, ret_ty } => {
                let ret_ty_str = self.type_to_llvm(ret_ty);
                let mut instrs = Vec::new();
                let mut formatted_args = Vec::new();
                for a in args {
                    let arg_ty = a.get_type();
                    if symbol == "talas_print_i64" && arg_ty == Type::Int {
                        let ext_reg = format!("ext_{}", self.string_counter);
                        self.string_counter += 1;
                        let val_str = self.emit_operand(a);
                        instrs.push(format!("%{} = sext i32 {} to i64", ext_reg, val_str));
                        formatted_args.push(format!("i64 %{}", ext_reg));
                    } else {
                        formatted_args.push(format!("{} {}", self.type_to_llvm(&arg_ty), self.emit_operand(a)));
                    }
                }
                let args_str = formatted_args.join(", ");
                let call_str = if ret_ty == &Type::Void || ret_ty_str == "void" {
                    format!("call void @{}({})", symbol, args_str)
                } else if let Some(r) = dest {
                    format!("%{} = call {} @{}({})", r.0, ret_ty_str, symbol, args_str)
                } else {
                    format!("call {} @{}({})", ret_ty_str, symbol, args_str)
                };
                if instrs.is_empty() {
                    call_str
                } else {
                    instrs.push(call_str);
                    instrs.join("\n  ")
                }
            }
            IrInstruction::Branch { cond, then_block, else_block } => {
                let cond_str = self.emit_operand(cond);
                format!("br i1 {}, label %{}, label %{}", cond_str, then_block, else_block)
            }
            IrInstruction::Jump { target } => {
                format!("br label %{}", target)
            }
            IrInstruction::Return { val } => {
                if let Some(v) = val {
                    let v_ty = v.get_type();
                    let ty_str = if v == &IrOperand::Null || matches!(v_ty, Type::Nullable(_)) {
                        if self.current_return_type != Type::Void {
                            self.type_to_llvm(&self.current_return_type)
                        } else {
                            "i8*".to_string()
                        }
                    } else {
                        self.type_to_llvm(&v_ty)
                    };
                    format!("ret {} {}", ty_str, self.emit_operand(v))
                } else {
                    "ret void".to_string()
                }
            }
        }
    }

    fn emit_operand(&mut self, op: &IrOperand) -> String {
        match op {
            IrOperand::Register(reg, _) => format!("%{}", reg.0),
            IrOperand::ConstantInt(v) => v.to_string(),
            IrOperand::ConstantLong(v) => v.to_string(),
            IrOperand::ConstantDecimal(v) => format!("{:.6}", v),
            IrOperand::ConstantString(v) => {
                let label = format!("str_{}", self.string_counter);
                self.string_counter += 1;
                self.string_constants.insert(label.clone(), v.clone());
                format!("getelementptr inbounds ([{} x i8], [{} x i8]* @{}, i32 0, i32 0)", v.as_bytes().len() + 1, v.as_bytes().len() + 1, label)
            }
            IrOperand::ConstantBool(v) => (if *v { "true" } else { "false" }).to_string(),
            IrOperand::Null => "null".to_string(),
        }
    }

    fn type_to_llvm(&self, ty: &Type) -> String {
        match ty {
            Type::Int => "i32".to_string(),
            Type::Decimal => "double".to_string(),
            Type::Byte => "i8".to_string(),
            Type::Long => "i64".to_string(),
            Type::Bool => "i1".to_string(),
            Type::String => "i8*".to_string(),
            Type::Void => "void".to_string(),
            Type::Class { name } => format!("%{}*", name),
            Type::Nullable(inner) => {
                let inner_str = self.type_to_llvm(inner);
                if inner_str == "void" {
                    "i8*".to_string()
                } else {
                    inner_str
                }
            }
            Type::AnonymousObject { .. } => "i8*".to_string(),
            Type::Function { .. } => "i8*".to_string(),
        }
    }
}
