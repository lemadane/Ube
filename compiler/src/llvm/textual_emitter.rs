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
    task_thunks: Vec<String>,
    slots: HashMap<Register, Type>,
    signatures: HashMap<(String,String), Vec<Type>>,
}

impl TextualLlvmEmitter {
    pub fn new() -> Self {
        Self {
            output: String::new(),
            string_constants: HashMap::new(),
            string_counter: 0,
            class_map: HashMap::new(),
            current_return_type: Type::Void,
            task_thunks: Vec::new(),
            slots: HashMap::new(),
            signatures: HashMap::new(),
        }
    }

    pub fn emit_program(&mut self, ir_program: &IrProgram) -> String {
        self.output.clear();
        self.signatures = ir_program.functions.iter().map(|f| ((f.class_name.clone(), f.name.clone()), f.params.iter().map(|(_,t)| t.clone()).collect())).collect();
        self.class_map = ir_program
            .classes
            .iter()
            .map(|c| (c.name.clone(), c.clone()))
            .collect();
        self.task_thunks.clear();

        // Target Data Layout
        self.output
            .push_str("; Target Data Layout for Ube Native Binary\n");
        self.output.push_str("target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128\"\n\n");

        self.output.push_str("declare void @ube_type_method(i8*, i8*, i8*)\ndeclare i1 @ube_object_satisfies(i8*, i8*)\ndeclare i8* @ube_object_method(i8*, i8*)\ndeclare void @ube_object_register(i8*, i8*)\ndeclare i1 @ube_object_is(i8*, i8*)\ndeclare i8* @ube_exception_create(i8*, i8*, i8*, i8*)\ndeclare i8* @ube_exception_object(i8*)\ndeclare i8* @ube_exception_take()\ndeclare void @ube_exception_set(i8*)\ndeclare void @ube_exception_suppress(i8*, i8*)\ndeclare i8* @ube_exception_poll_cancel()\ndeclare void @ube_exception_fail_unhandled()\n");
        self.output.push_str("declare { i8*, i64 } @ube_gc_allocate(i64, i64, i64*, i64)\ndeclare void @ube_gc_frame_enter()\ndeclare void @ube_gc_frame_pin(i64, i8*)\ndeclare void @ube_gc_frame_leave(i8*)\ndeclare void @ube_gc_escape(i8*)\ndeclare i1 @ube_gc_root_remove(i64)\ndeclare void @ube_gc_store_reference(i8*, i64, i8*)\ndeclare i8* @ube_gc_load_reference(i8*, i64, i64)\ndeclare i1 @ube_gc_try_collect()\ndeclare i64 @ube_gc_object_count()\ndeclare i64 @ube_gc_reclaimed_count()\n");
        // External runtime declarations
        self.output.push_str("declare void @ube_runtime_init()\n");
        self.output
            .push_str("declare void @ube_runtime_cleanup()\n");
        self.output.push_str("declare i8* @ube_alloc(i64, i64)\n");
        self.output.push_str("declare i8* @ube_console_read()\n");
        self.output.push_str("declare i8* @ube_console_read_line()\n");
        self.output.push_str("declare void @ube_output_str(i32, i8*)\n");
        self.output.push_str("declare void @ube_output_i64(i32, i64)\n");
        self.output.push_str("declare void @ube_output_f64(i32, double)\n");
        self.output.push_str("declare void @ube_output_bool(i32, i1)\n");
        self.output.push_str("declare void @ube_log_set_level(i8*)\n");
        self.output.push_str("declare void @ube_log_put(i8*, i8*)\n");
        self.output.push_str("declare i8* @ube_log_get(i8*)\n");
        self.output.push_str("declare void @ube_log_remove(i8*)\n");
        self.output.push_str("declare void @ube_log_clear()\n");
        self.output.push_str("declare i8* @ube_format_value(i32, i64, i64)\n");
        self.output.push_str("declare i1 @ube_string_equal(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_string_concat(i8*, i8*)\n");
        self.output.push_str("declare i8* @ube_list_create()\n");
        self.output
            .push_str("declare void @ube_list_add(i8*, i64)\n");
        self.output
            .push_str("declare i64 @ube_list_get(i8*, i64)\n");
        self.output.push_str("declare i64 @ube_list_len(i8*)\n");
        self.output.push_str("declare i8* @ube_map_create()\n");
        self.output
            .push_str("declare void @ube_map_put(i8*, i64, i64)\n");
        self.output.push_str("declare i64 @ube_map_get(i8*, i64)\n");
        self.output.push_str("declare i1 @ube_map_contains_key(i8*, i64)\n");
        self.output.push_str("declare i64 @ube_map_len(i8*)\n");
        self.output.push_str("declare i8* @ube_task_create(i64)\n");
        self.output
            .push_str("declare i8* @ube_task_spawn_value(i64, i8*)\n");
        self.output
            .push_str("declare i8* @ube_task_context_create(i64)\n");
        self.output
            .push_str("declare void @ube_task_context_set_value(i8*, i64, i64, i64)\n");
        self.output
            .push_str("declare { i64, i64 } @ube_task_context_get_value(i8*, i64)\n");
        self.output.push_str("declare { i64, i64 } @ube_task_await_value(i8*)\n");
        self.output
            .push_str("declare %Error* @ube_task_last_error()\n");
        self.output
            .push_str("declare void @ube_task_set_error(i8*)\n");
        self.output
            .push_str("declare i1 @ube_task_is_cancelled()\n");
        self.output
            .push_str("declare void @ube_task_set_cancelled()\n");
        self.output.push_str("declare void @ube_task_yield()\n");
        self.output.push_str("declare i8* @ube_task_all(i8*)\n");
        self.output.push_str("declare i64 @ube_task_any(i8*)\n");
        self.output.push_str("declare i64 @ube_task_race(i8*)\n");
        self.output
            .push_str("declare i8* @ube_task_all_settled(i8*)\n");
        self.output
            .push_str("declare i8* @ube_task_all_async(i8*)\n");
        self.output
            .push_str("declare i8* @ube_task_any_async(i8*)\n");
        self.output
            .push_str("declare i8* @ube_task_race_async(i8*)\n");
        self.output
            .push_str("declare i8* @ube_task_all_settled_async(i8*)\n");
        self.output.push_str("declare i1 @ube_task_cancel(i8*)\n");
        self.output
            .push_str("declare i8* @ube_task_timeout(i8*, i64)\n");
        self.output
            .push_str("declare void @ube_task_fail_unhandled()\n");
        self.output
            .push_str("declare i1 @ube_task_propagate_error()\n");
        self.output.push_str("declare i8* @ube_channel_create()\n");
        self.output
            .push_str("declare i8* @ube_channel_create_capacity(i64)\n");
        self.output
            .push_str("declare i1 @ube_channel_send(i8*, i64)\n");
        self.output
            .push_str("declare i8* @ube_channel_send_value_async(i8*, i64, i64)\n");
        self.output.push_str("declare i64 @ube_channel_recv(i8*)\n");
        self.output
            .push_str("declare i8* @ube_channel_recv_async(i8*)\n");
        self.output
            .push_str("declare void @ube_channel_close(i8*)\n");
        self.output.push_str("declare i8* @ube_datetime_now()\n");
        self.output.push_str("declare i8* @ube_datetime_utc()\n");
        self.output
            .push_str("declare i8* @ube_datetime_from_unix(i64)\n");
        self.output
            .push_str("declare i8* @ube_datetime_from_unix_millis(i64)\n");
        self.output
            .push_str("declare i8* @ube_datetime_of(i64, i64, i64, i64, i64)\n");
        self.output
            .push_str("declare i8* @ube_datetime_date(i64, i64, i64)\n");
        self.output
            .push_str("declare i8* @ube_datetime_time(i64, i64)\n");
        self.output
            .push_str("declare i8* @ube_datetime_parse(i8*)\n");
        self.output
            .push_str("declare i64 @ube_datetime_year(i8*)\n");
        self.output
            .push_str("declare i64 @ube_datetime_month(i8*)\n");
        self.output.push_str("declare i64 @ube_datetime_day(i8*)\n");
        self.output
            .push_str("declare i64 @ube_datetime_hour(i8*)\n");
        self.output
            .push_str("declare i64 @ube_datetime_minute(i8*)\n");
        self.output
            .push_str("declare i64 @ube_datetime_second(i8*)\n");
        self.output
            .push_str("declare i64 @ube_datetime_timestamp(i8*)\n");
        self.output
            .push_str("declare i64 @ube_datetime_unix(i8*)\n");
        self.output
            .push_str("declare i64 @ube_datetime_unix_millis(i8*)\n");
        self.output
            .push_str("declare i64 @ube_datetime_unix_nanos(i8*)\n");
        self.output
            .push_str("declare i8* @ube_datetime_zone(i8*)\n");
        self.output
            .push_str("declare i8* @ube_datetime_in_zone(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_datetime_add_seconds(i8*, i64)\n");
        self.output
            .push_str("declare i64 @ube_datetime_diff_seconds(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_datetime_format(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_duration_create(i64)\n");
        self.output
            .push_str("declare i64 @ube_duration_total_seconds(i8*)\n");
        self.output
            .push_str("declare i8* @ube_currency_create(i8*)\n");
        self.output
            .push_str("declare i8* @ube_currency_code(i8*)\n");
        self.output
            .push_str("declare i8* @ube_money_create(double, i8*)\n");
        self.output.push_str("declare i8* @ube_money_parse(i8*, i8*)\n");
        self.output.push_str("declare i8* @ube_money_amount_text(i8*)\n");
        self.output.push_str("declare i8* @ube_money_multiply_percent_text(i8*, i8*)\n");
        self.output.push_str("declare i8* @ube_money_convert_exact(i8*, i8*, i8*)\n");
        self.output.push_str("declare i8* @ube_money_php(double)\n");
        self.output.push_str("declare i8* @ube_money_usd(double)\n");
        self.output.push_str("declare i8* @ube_money_eur(double)\n");
        self.output.push_str("declare i8* @ube_money_jpy(double)\n");
        self.output.push_str("declare i8* @ube_money_gbp(double)\n");
        self.output
            .push_str("declare double @ube_money_amount(i8*)\n");
        self.output
            .push_str("declare i8* @ube_money_currency(i8*)\n");
        self.output
            .push_str("declare i8* @ube_money_add(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_money_subtract(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_money_multiply_percent(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_exchange_convert(i8*, i8*, double)\n");
        self.output
            .push_str("declare i8* @ube_percent_create(double)\n");
        self.output
            .push_str("declare double @ube_percent_value(i8*)\n");
        self.output
            .push_str("declare i8* @ube_rate_create(double)\n");
        self.output
            .push_str("declare double @ube_rate_value(i8*)\n");
        self.output
            .push_str("declare i8* @ube_geopoint_create(double, double)\n");
        self.output
            .push_str("declare double @ube_geopoint_latitude(i8*)\n");
        self.output
            .push_str("declare double @ube_geopoint_longitude(i8*)\n");
        self.output
            .push_str("declare i8* @ube_geopoint_distance_to(i8*, i8*)\n");
        self.output
            .push_str("declare double @ube_geopoint_bearing_to(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_distance_create_meters(double)\n");
        self.output
            .push_str("declare i8* @ube_distance_create_km(double)\n");
        self.output
            .push_str("declare i8* @ube_distance_create_miles(double)\n");
        self.output
            .push_str("declare double @ube_distance_meters(i8*)\n");
        self.output
            .push_str("declare double @ube_distance_km(i8*)\n");
        self.output
            .push_str("declare double @ube_distance_miles(i8*)\n");
        self.output
            .push_str("declare i8* @ube_geobounds_create(i8*, i8*)\n");
        self.output
            .push_str("declare i1 @ube_geobounds_contains(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_geolocation_current()\n");
        self.output.push_str("declare double @ube_math_pi()\n");
        self.output.push_str("declare double @ube_math_e()\n");
        self.output.push_str("declare double @ube_math_tau()\n");
        self.output
            .push_str("declare double @ube_math_abs(double)\n");
        self.output
            .push_str("declare double @ube_math_min(double, double)\n");
        self.output
            .push_str("declare double @ube_math_max(double, double)\n");
        self.output
            .push_str("declare double @ube_math_sqrt(double)\n");
        self.output
            .push_str("declare double @ube_math_cbrt(double)\n");
        self.output
            .push_str("declare double @ube_math_pow(double, double)\n");
        self.output
            .push_str("declare double @ube_math_floor(double)\n");
        self.output
            .push_str("declare double @ube_math_ceil(double)\n");
        self.output
            .push_str("declare double @ube_math_round(double)\n");
        self.output
            .push_str("declare double @ube_math_round_decimals(double, i64)\n");
        self.output
            .push_str("declare double @ube_math_truncate(double)\n");
        self.output
            .push_str("declare double @ube_math_clamp(double, double, double)\n");
        self.output.push_str("declare i64 @ube_math_sign(double)\n");
        self.output
            .push_str("declare double @ube_math_sin(double)\n");
        self.output
            .push_str("declare double @ube_math_cos(double)\n");
        self.output
            .push_str("declare double @ube_math_tan(double)\n");
        self.output
            .push_str("declare double @ube_math_asin(double)\n");
        self.output
            .push_str("declare double @ube_math_acos(double)\n");
        self.output
            .push_str("declare double @ube_math_atan(double)\n");
        self.output
            .push_str("declare double @ube_math_atan2(double, double)\n");
        self.output
            .push_str("declare double @ube_math_radians(double)\n");
        self.output
            .push_str("declare double @ube_math_degrees(double)\n");
        self.output
            .push_str("declare double @ube_math_log(double)\n");
        self.output
            .push_str("declare double @ube_math_log10(double)\n");
        self.output
            .push_str("declare double @ube_math_log2(double)\n");
        self.output
            .push_str("declare double @ube_math_exp(double)\n");
        self.output
            .push_str("declare i1 @ube_math_is_nan(double)\n");
        self.output
            .push_str("declare i1 @ube_math_is_infinite(double)\n");
        self.output
            .push_str("declare i1 @ube_math_is_finite(double)\n");
        self.output.push_str("declare i1 @ube_math_is_even(i64)\n");
        self.output.push_str("declare i1 @ube_math_is_odd(i64)\n");
        self.output
            .push_str("declare i64 @ube_math_gcd(i64, i64)\n");
        self.output
            .push_str("declare i64 @ube_math_lcm(i64, i64)\n");
        self.output
            .push_str("declare i64 @ube_random_int(i64, i64)\n");
        self.output
            .push_str("declare double @ube_random_decimal()\n");
        self.output.push_str("declare i1 @ube_random_bool()\n");
        self.output.push_str("declare i8* @ube_random_uuid()\n");
        self.output.push_str("declare i8* @ube_random_bytes(i64)\n");
        self.output
            .push_str("declare i8* @ube_securerandom_bytes(i64)\n");
        self.output
            .push_str("declare double @ube_statistics_mean(i8*)\n");
        self.output
            .push_str("declare double @ube_statistics_median(i8*)\n");
        self.output
            .push_str("declare double @ube_statistics_variance(i8*)\n");
        self.output
            .push_str("declare double @ube_statistics_std_dev(i8*)\n");
        self.output
            .push_str("declare double @ube_statistics_percentile(i8*, double)\n");
        self.output
            .push_str("declare double @ube_statistics_covariance(i8*, i8*)\n");
        self.output
            .push_str("declare double @ube_statistics_correlation(i8*, i8*)\n");
        self.output
            .push_str("declare i64 @ube_probability_factorial(i64)\n");
        self.output
            .push_str("declare i64 @ube_probability_permutations(i64, i64)\n");
        self.output
            .push_str("declare i64 @ube_probability_combinations(i64, i64)\n");
        self.output
            .push_str("declare double @ube_probability_normal(double, double, double)\n");
        self.output
            .push_str("declare double @ube_probability_binomial(i64, i64, double)\n");
        self.output
            .push_str("declare double @ube_probability_poisson(i64, double)\n");
        self.output
            .push_str("declare double @ube_probability_uniform(double, double, double)\n");
        for &class in crate::builtins::SQLITE_CLASSES {
            for method in crate::builtins::sqlite_methods(class) {
                let mut params = Vec::new();
                if !method.is_static { params.push(format!("%{}*", class)); }
                params.extend(method.params.iter().map(|ty| self.type_to_llvm(ty)));
                self.output.push_str(&format!("declare {} @{}({})\n", self.type_to_llvm(&method.result), method.symbol, params.join(", ")));
            }
        }
        self.output.push_str("declare %OrmModel* @ube_orm_model(%SQL*, i8*, i8*)\ndeclare %OrmModel* @ube_orm_using(%OrmModel*, %SQLTransaction*)\ndeclare void @ube_orm_migrate(%OrmModel*)\ndeclare i64 @ube_orm_create(%OrmModel*, %SQLParams*)\ndeclare i64 @ube_orm_save(%OrmModel*, %SQLParams*)\ndeclare %SQLResult* @ube_orm_find(%OrmModel*, i64)\ndeclare %SQLResult* @ube_orm_get(%OrmModel*, i64)\ndeclare i64 @ube_orm_delete(%OrmModel*, i64)\ndeclare void @ube_orm_model_close(%OrmModel*)\ndeclare %OrmQuery* @ube_orm_query(%OrmModel*)\ndeclare %OrmQuery* @ube_orm_join(%OrmQuery*, %OrmModel*, i8*, i8*, i8*, i1)\ndeclare %OrmQuery* @ube_orm_order_by(%OrmQuery*, i8*, i8*)\ndeclare %OrmQuery* @ube_orm_limit(%OrmQuery*, i32)\ndeclare %OrmQuery* @ube_orm_offset(%OrmQuery*, i32)\ndeclare %OrmQuery* @ube_orm_where_null(%OrmQuery*, i8*, i1)\ndeclare %SQLResult* @ube_orm_all(%OrmQuery*)\ndeclare %SQLResult* @ube_orm_first(%OrmQuery*)\ndeclare i64 @ube_orm_count(%OrmQuery*)\ndeclare void @ube_orm_query_close(%OrmQuery*)\n");
        for (suffix, ty) in [("int", "i32"), ("long", "i64"), ("decimal", "double"), ("bool", "i1"), ("string", "i8*")] {
            self.output.push_str(&format!("declare %OrmQuery* @ube_orm_where_{}(%OrmQuery*, i8*, i8*, {})\n", suffix, ty));
        }
        for &class in crate::builtins::SQL_CLASSES {
            for method in crate::builtins::sql_methods(class) {
                let mut params = Vec::new();
                if !method.is_static { params.push(format!("%{}*", class)); }
                params.extend(method.params.iter().map(|ty| self.type_to_llvm(ty)));
                self.output.push_str(&format!("declare {} @{}({})\n", self.type_to_llvm(&method.result), method.symbol, params.join(", ")));
            }
        }
        for &class in crate::builtins::AUTH_CLASSES {
            for method in crate::builtins::auth_methods(class) {
                let mut params = Vec::new();
                if !method.is_static { params.push(format!("%{}*", class)); }
                params.extend(method.params.iter().map(|ty| self.type_to_llvm(ty)));
                self.output.push_str(&format!("declare {} @{}({})\n", self.type_to_llvm(&method.result), method.symbol, params.join(", ")));
            }
        }
        self.output.push_str("declare i8* @ube_file_open(i8*)\n");
        self.output.push_str("declare i8* @ube_file_read(i8*)\n");
        self.output
            .push_str("declare void @ube_file_write(i8*, i8*)\n");
        self.output
            .push_str("declare void @ube_file_append(i8*, i8*)\n");
        self.output.push_str("declare void @ube_file_delete(i8*)\n");
        self.output.push_str("declare i1 @ube_file_exists(i8*)\n");
        self.output.push_str("declare void @ube_file_close(i8*)\n");
        self.output.push_str("declare i8* @ube_file_info(i8*)\n");
        self.output
            .push_str("declare i64 @ube_fileinfo_size(i8*)\n");
        self.output
            .push_str("declare i8* @ube_fileinfo_created_at(i8*)\n");
        self.output
            .push_str("declare i8* @ube_fileinfo_modified_at(i8*)\n");
        self.output
            .push_str("declare i1 @ube_fileinfo_is_directory(i8*)\n");
        self.output
            .push_str("declare void @ube_directory_create(i8*)\n");
        self.output
            .push_str("declare void @ube_directory_delete(i8*)\n");
        self.output
            .push_str("declare i8* @ube_directory_list(i8*)\n");
        self.output
            .push_str("declare i8* @ube_path_join(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_path_filename(i8*)\n");
        self.output
            .push_str("declare i8* @ube_path_extension(i8*)\n");
        self.output
            .push_str("declare %Server* @ube_server_create()\n");
        self.output
            .push_str("declare void @ube_server_start(%Server*, i32)\n");
        self.output
            .push_str("declare void @ube_server_stop(%Server*)\n");
        self.output
            .push_str("declare i32 @ube_server_port(%Server*)\n");
        self.output
            .push_str("declare void @ube_server_tls(%Server*, i8*, i8*)\n");
        self.output
            .push_str("declare void @ube_server_route(%Server*, i8*, i8*, i8*, i64)\n");
        self.output
            .push_str("declare void @ube_server_use(%Server*, i64)\n");
        self.output
            .push_str("declare i8* @ube_request_path(i8*, i8*)\n");
        self.output
            .push_str("declare i32 @ube_request_path_int(i8*, i8*)\n");
        self.output
            .push_str("declare i64 @ube_request_path_long(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_request_query(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_request_header(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_request_body(i8*)\n");
        self.output
            .push_str("declare i1 @ube_request_has_bearer_token(i8*)\n");
        self.output
            .push_str("declare %Response* @ube_response_create(i32, i8*)\n");
        self.output
            .push_str("declare i8* @ube_cookie_create(i8*, i8*)\n");
        self.output
            .push_str("declare void @ube_websocket_send(i8*, i8*)\n");
        self.output
            .push_str("declare void @ube_websocket_on_message(i8*, i64)\n");
        self.output
            .push_str("declare void @ube_server_enable_http2(%Server*)\n");
        self.output
            .push_str("declare void @ube_server_enable_http3(%Server*, i32)\n");
        self.output
            .push_str("declare i8* @ube_request_protocol(i8*)\n");
        self.output
            .push_str("declare i1 @ube_request_is_http3(i8*)\n");
        self.output.push_str("declare i32 @ube_server_http3_port(%Server*)\n");
        self.output.push_str("declare %Response* @ube_response_redirect(i8*)\n");
        self.output.push_str("declare %Session* @ube_request_session(%Request*)\n");
        self.output.push_str("declare i8* @ube_session_get(%Session*, i8*)\n");
        self.output.push_str("declare void @ube_session_set(%Session*, i8*, i8*)\n");
        self.output.push_str("declare void @ube_session_remove(%Session*, i8*)\n");
        self.output.push_str("declare void @ube_session_regenerate(%Session*)\n");
        self.output.push_str("declare void @ube_session_destroy(%Session*)\n");
        self.output.push_str("declare i8* @ube_bytes_create(i64)\n");
        self.output.push_str("declare void @ube_bytes_free(i8*)\n");
        self.output
            .push_str("declare i8* @ube_file_open_read(i8*)\n");
        self.output
            .push_str("declare i8* @ube_file_open_write(i8*)\n");
        self.output
            .push_str("declare i64 @ube_stream_read(i8*, i8*, i64, i64)\n");
        self.output
            .push_str("declare i64 @ube_stream_write(i8*, i8*, i64, i64)\n");
        self.output
            .push_str("declare i64 @ube_stream_pipe_to(i8*, i8*)\n");
        self.output.push_str("declare i8* @ube_file_sha256(i8*)\n");
        self.output
            .push_str("declare i8* @ube_response_stream(i32, i8*)\n");
        self.output
            .push_str("declare i8* @ube_response_file(i8*)\n");
        self.output.push_str("declare i8* @ube_json_parse(i8*)\n");

        self.output
            .push_str("declare i8* @ube_json_stringify(i8*)\n");
        self.output
            .push_str("declare i8* @ube_jsonobject_get_string(i8*, i8*)\n");
        self.output
            .push_str("declare i32 @ube_jsonobject_get_int(i8*, i8*)\n");
        self.output
            .push_str("declare i64 @ube_jsonobject_get_long(i8*, i8*)\n");
        self.output
            .push_str("declare double @ube_jsonobject_get_decimal(i8*, i8*)\n");
        self.output
            .push_str("declare i1 @ube_jsonobject_get_bool(i8*, i8*)\n");
        self.output.push_str("declare i8* @ube_xml_parse(i8*)\n");
        self.output.push_str("declare i8* @ube_xml_create(i8*)\n");
        self.output
            .push_str("declare i8* @ube_xmldocument_root(i8*)\n");
        self.output
            .push_str("declare i8* @ube_xmldocument_to_string(i8*)\n");
        self.output
            .push_str("declare i8* @ube_xmlement_child(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_xmlement_element(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_xmlement_text(i8*, i8*)\n");
        self.output
            .push_str("declare i8* @ube_xmlement_get_text(i8*)\n\n");

        let mut declared_types: HashSet<String> =
            ir_program.classes.iter().map(|c| c.name.clone()).collect();

        // Object layouts for concrete classes
        for cls in &ir_program.classes {
            let field_types = cls
                .fields
                .iter()
                .map(|(_, ty)| self.type_to_llvm(ty))
                .collect::<Vec<_>>()
                .join(", ");
            self.output
                .push_str(&format!("%{} = type {{ {} }}\n", cls.name, field_types));
        }

        for class in &ir_program.classes {
            let edges = class.fields.iter().enumerate().filter_map(|(index, (_, ty))| {
                let field = self.type_to_llvm(ty);
                field.ends_with('*').then(|| format!("i64 ptrtoint ({}* getelementptr (%{}, %{}* null, i32 0, i32 {}) to i64)", field, class.name, class.name, index))
            }).collect::<Vec<_>>();
            self.output.push_str(&format!("@ube_gc_layout_{} = private constant [{} x i64] [{}]\n", class.name, edges.len(), edges.join(", ")));
        }
        let builtins = vec![
            ("GenericError", "i8*"),
            ("CurrencyMismatchError", "i8*"),
            ("MoneyError", "i8*"),
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
            ("Orm", "i8*"),
            ("OrmModel", "i8*"),
            ("OrmQuery", "i8*"),
            ("OrmRows", "i8*"),
            ("SQL", "i8*"),
            ("SQLParams", "i8*"),
            ("SQLTransaction", "i8*"),
            ("SQLResult", "i8*"),
            ("SQLError", "i8*"),
            ("SQLite", "i8*"),
            ("SQLiteResult", "i8*"),
            ("SQLiteStatement", "i8*"),
            ("SQLiteError", "i8*"),
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
            ("Session", "i8*"),
            ("Jwt", "i8*"),
            ("JwtClaims", "i8*"),
            ("OAuth2", "i8*"),
            ("OAuth2Token", "i8*"),
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
                self.output
                    .push_str(&format!("%{} = type {{ {} }}\n", name, fields));
                declared_types.insert(name.to_string());
            }
        }

        // Object layouts for abstract trait types referenced in parameters/receivers
        for func in &ir_program.functions {
            for (_, ty) in std::iter::once(&func.receiver).chain(func.params.iter()) {
                if let Type::Class { name } = ty {
                    if !declared_types.contains(name) {
                        self.output.push_str(&format!("%{} = type {{ }}\n", name));
                        declared_types.insert(name.clone());
                    }
                }
            }
        }
        self.output.push('\n');

        // A module initializer registers structural signatures and C-ABI method addresses.
        // Identity uses names/signatures, never module-local pointer addresses.
        self.output.push_str("@llvm.global_ctors = appending global [1 x { i32, void ()*, i8* }] [{ i32, void ()*, i8* } { i32 65535, void ()* @ube_register_types, i8* null }]\n");
        self.output.push_str("define private void @ube_register_types() {\nentry:\n");
        for function in &ir_program.functions {
            if !self.class_map.contains_key(&function.class_name) || function.is_async { continue; }
            let name = self.emit_operand(&IrOperand::ConstantString(function.class_name.clone()));
            let params: Vec<_> = function.params.iter().map(|(_,t)| t.clone()).collect();
            let signature = self.emit_operand(&IrOperand::ConstantString(method_signature(&function.name, &params, &function.return_type)));
            let arguments = std::iter::once(self.type_to_llvm(&function.receiver.1)).chain(params.iter().map(|t| self.type_to_llvm(t))).collect::<Vec<_>>().join(", ");
            self.output.push_str(&format!("  call void @ube_type_method(i8* {}, i8* {}, i8* bitcast ({} ({})* @{}_{} to i8*))\n", name, signature, self.type_to_llvm(&function.return_type), arguments, function.class_name, function.name));
        }
        self.output.push_str("  ret void\n}\n\n");

        // Emit functions
        for func in &ir_program.functions {
            let func_str = self.emit_function(func);
            self.output.push_str(&func_str);
            self.output.push('\n');
        }

        for thunk in std::mem::take(&mut self.task_thunks) {
            self.output.push_str(&thunk);
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
            self.output
                .push_str("define i32 @main(i32 %argc, i8** %argv) {\n");
            self.output.push_str("entry:\n");
            self.output.push_str("  call void @ube_runtime_init()\n");
            // Startup shares the IR builder's declaration zero-initialization path.
            self.output.push_str(&format!(
                "  %app = call {}* @__ube.internal_zero(i8* null)\n", class_type
            ));
            self.output.push_str(&format!(
                "  call void @{}({}* %app)\n",
                target_symbol, class_type
            ));
            self.output.push_str("  call void @ube_exception_fail_unhandled()\n");
            self.output.push_str("  call void @ube_runtime_cleanup()\n");
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
                label, bytes_len, escaped
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
        self.slots.clear();
        let mut buf = String::new();
        let ret_ty_str = self.type_to_llvm(&func.return_type);
        let fn_name = format!("{}_{}", func.class_name, func.name);
        let receiver_str = format!(
            "{} %{}",
            self.type_to_llvm(&func.receiver.1),
            func.receiver.0 .0
        );

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

        if func.blocks.is_empty() { return format!("declare {} @{}({})\n", ret_ty_str, fn_name, all_params); }
        let linkage = if ["MoneyError", "CurrencyMismatchError", "SQLError", "SQLiteError", "GenericError", "OutOfBoundsError", "FileError", "FileNotFoundError", "PermissionError", "AlreadyExistsError", "InvalidPathError", "DiskFullError", "LocationPermissionError", "LocationUnavailableError", "JsonError", "JsonParseError", "MissingFieldError", "InvalidTypeError", "XmlError", "XmlParseError"].contains(&func.class_name.as_str()) { "linkonce_odr " } else { "" };
        buf.push_str(&format!("define {}{} @{}({}) {{\n", linkage, ret_ty_str, fn_name, all_params));

        for (index, block) in func.blocks.iter().enumerate() {
            buf.push_str(&format!("{}:\n", block.id));
            if index == 0 {
                // Stack slots have function lifetime. Allocate them once, not
                // on every loop iteration. Named slots preserve numeric SSA order.
                for allocation in func.blocks.iter().flat_map(|block| &block.instructions) {
                    if matches!(allocation, IrInstruction::Alloca { .. }) {
                        buf.push_str(&format!("  {}\n", self.emit_instruction(allocation)));
                    }
                }
                buf.push_str("  call void @ube_gc_frame_enter()\n");
                for (register, ty) in std::iter::once(&func.receiver).chain(func.params.iter()) {
                    let pin = self.gc_pin(&format!("%{}", register.0), ty, register.0 * 2);
                    if !pin.is_empty() { buf.push_str(&format!("  {}\n", pin)); }
                }
            }
            for instruction in &block.instructions {
                if matches!(instruction, IrInstruction::Alloca { .. }) { continue; }
                let escaping = match instruction {
                    IrInstruction::CallRuntime { symbol, args, .. } if !matches!(symbol.as_str(), "ube_object_register" | "ube_object_is" | "ube_object_satisfies" | "ube_object_method") && !symbol.starts_with("ube_gc_") => args.clone(),
                    IrInstruction::SpawnAsyncCall { receiver, args, .. } => std::iter::once(receiver.clone()).chain(args.iter().cloned()).collect(),
                    IrInstruction::SpawnAsyncExpression { captures, .. } => captures.clone(),
                    IrInstruction::ServerRoute { context, .. } => vec![context.clone()],
                    _ => Vec::new(),
                };
                for value in escaping {
                    let escape = self.gc_escape(&value);
                    if !escape.is_empty() { buf.push_str(&format!("  {}\n", escape)); }
                }
                if let IrInstruction::Return { val } = instruction {
                    let returned = val.as_ref().filter(|value| self.type_to_llvm(&value.get_type()).ends_with('*'));
                    let pointer = if let Some(value) = returned { { let operand = self.emit_operand(value); self.gc_pointer(&operand, &value.get_type()) } } else { (Vec::new(), "null".into()) };
                    for cast in pointer.0 { buf.push_str(&format!("  {}\n", cast)); }
                    buf.push_str(&format!("  call void @ube_gc_frame_leave(i8* {})\n", pointer.1));
                }
                buf.push_str(&format!("  {}\n", self.emit_instruction(instruction)));
                let result = match instruction {
                    IrInstruction::Cast { dest, ty, .. } | IrInstruction::Load { dest, ty, .. } => Some((*dest, ty.clone())),
                    IrInstruction::GetField { dest, field_ty, .. } => Some((*dest, field_ty.clone())),
                    IrInstruction::CallMethod { dest: Some(dest), ret_ty, .. } | IrInstruction::CallRuntime { dest: Some(dest), ret_ty, .. } | IrInstruction::CallTrait { dest: Some(dest), ret_ty, .. } => Some((*dest, ret_ty.clone())),
                    IrInstruction::InstantiateClass { dest, .. } => Some((*dest, Type::Bytes)),
                    IrInstruction::AwaitTask { dest, result_ty, .. } => Some((*dest, result_ty.clone())),
                    _ => None,
                };
                if let Some((register, ty)) = result {
                    let pin = self.gc_pin(&format!("%{}", register.0), &ty, register.0 * 2);
                    if !pin.is_empty() { buf.push_str(&format!("  {}\n", pin)); }
                    if matches!(instruction, IrInstruction::InstantiateClass { .. }) {
                        buf.push_str(&format!("  %gc_initial_drop_{} = call i1 @ube_gc_root_remove(i64 %gc_handle_{})\n", register.0, register.0));
                    }
                }
                if let IrInstruction::Store { val, ptr } = instruction {
                    let ty = self.slots.get(ptr).cloned().unwrap_or_else(|| val.get_type());
                    if self.type_to_llvm(&ty).ends_with('*') {
                        let (casts, value) = self.coerce(val, &ty);
                        for cast in casts { buf.push_str(&format!("  {}\n", cast)); }
                        let pin = self.gc_pin(&value, &ty, ptr.0 * 2 + 1);
                        buf.push_str(&format!("  {}\n", pin));
                    }
                }
            }
        }

        buf.push_str("}\n");
        buf
    }

    fn emit_instruction(&mut self, instr: &IrInstruction) -> String {
        match instr {
            IrInstruction::Cast { dest, value, ty } => {
                let (mut instructions, value) = self.coerce(value, ty);
                if let Some(inner) = ty.nullable_scalar_inner() {
                    let llvm = self.type_to_llvm(ty);
                    instructions.push(format!("%cast_payload_{} = extractvalue {} {}, 1", dest.0, llvm, value));
                    instructions.push(format!("%{} = insertvalue {} {}, {} %cast_payload_{}, 1", dest.0, llvm, value, self.type_to_llvm(inner), dest.0));
                } else {
                    instructions.push(format!("%{} = bitcast {} {} to {}", dest.0, self.type_to_llvm(ty), value, self.type_to_llvm(ty)));
                }
                instructions.join("\n  ")
            }
            IrInstruction::Alloca { dest, ty } => {
                self.slots.insert(*dest, ty.clone());
                let mut ty_str = self.type_to_llvm(ty);
                if ty_str == "void" {
                    ty_str = "i8*".to_string();
                }
                format!("%slot_{} = alloca {}, align 1", dest.0, ty_str)
            }
            IrInstruction::Load { dest, ptr, ty } => {
                let ty_str = self.type_to_llvm(ty);
                format!(
                    "%{} = load {}, {}* %slot_{}, align 1",
                    dest.0, ty_str, ty_str, ptr.0
                )
            }
            IrInstruction::Store { val, ptr } => {
                let ty = self.slots.get(ptr).cloned().unwrap_or_else(|| val.get_type());
                let ty_str = self.type_to_llvm(&ty);
                let (mut instructions, value) = self.coerce(val, &ty);
                instructions.push(format!("store {} {}, {}* %slot_{}, align 1", ty_str, value, ty_str, ptr.0));
                instructions.join("\n  ")
            }
            IrInstruction::GetField {
                dest,
                ptr,
                class_name,
                field_index,
                field_ty,
            } => {
                let class_type = format!("%{}", class_name);
                let field_ty_str = self.type_to_llvm(field_ty);
                let gep_reg = format!("gep_{}_{}", ptr.0, self.string_counter);
                self.string_counter += 1;
                if field_ty_str.ends_with('*') {
                    let mut instructions = vec![format!("%{} = getelementptr inbounds {}, {}* %{}, i32 0, i32 {}", gep_reg, class_type, class_type, ptr.0, field_index)];
                    let (casts, object) = self.gc_pointer(&format!("%{}", ptr.0), &Type::Class { name: class_name.clone() });
                    instructions.extend(casts);
                    let number = self.string_counter; self.string_counter += 1;
                    instructions.push(format!("%gc_load_offset_{} = ptrtoint {}* %{} to i64", number, field_ty_str, gep_reg));
                    instructions.push(format!("%gc_load_base_{} = ptrtoint i8* {} to i64", number, object));
                    instructions.push(format!("%gc_load_index_{} = sub i64 %gc_load_offset_{}, %gc_load_base_{}", number, number, number));
                    instructions.push(format!("%gc_loaded_{} = call i8* @ube_gc_load_reference(i8* {}, i64 %gc_load_index_{}, i64 {})", number, object, number, dest.0 * 2));
                    instructions.push(format!("%{} = bitcast i8* %gc_loaded_{} to {}", dest.0, number, field_ty_str));
                    instructions.join("\n  ")
                } else {
                    format!("%{} = getelementptr inbounds {}, {}* %{}, i32 0, i32 {}\n  %{} = load {}, {}* %{}, align 1", gep_reg, class_type, class_type, ptr.0, field_index, dest.0, field_ty_str, field_ty_str, gep_reg)
                }
            }
            IrInstruction::StoreField {
                ptr,
                class_name,
                field_index,
                val,
            } => {
                let class_type = format!("%{}", class_name);
                let val_ty = self.class_map.get(class_name).and_then(|class| class.fields.get(*field_index)).map(|field| field.1.clone()).unwrap_or_else(|| val.get_type());
                let val_ty_str = self.type_to_llvm(&val_ty);
                let (casts, val_str) = self.coerce(val, &val_ty);
                let gep_reg = format!("gep_store_{}_{}", ptr.0, self.string_counter);
                self.string_counter += 1;
                let mut instructions = casts;
                instructions.push(format!("%{} = getelementptr inbounds {}, {}* %{}, i32 0, i32 {}", gep_reg, class_type, class_type, ptr.0, field_index));
                if val_ty_str.ends_with('*') {
                    let (casts, object) = self.gc_pointer(&format!("%{}", ptr.0), &Type::Class { name: class_name.clone() });
                    instructions.extend(casts);
                    let (casts, value) = self.gc_pointer(&val_str, &val_ty);
                    instructions.extend(casts);
                    let number = self.string_counter; self.string_counter += 1;
                    instructions.push(format!("%gc_offset_{} = ptrtoint {}* %{} to i64", number, val_ty_str, gep_reg));
                    instructions.push(format!("%gc_base_{} = ptrtoint i8* {} to i64", number, object));
                    instructions.push(format!("%gc_index_{} = sub i64 %gc_offset_{}, %gc_base_{}", number, number, number));
                    instructions.push(format!("call void @ube_gc_store_reference(i8* {}, i64 %gc_index_{}, i8* {})", object, number, value));
                } else {
                    instructions.push(format!("store {} {}, {}* %{}, align 1", val_ty_str, val_str, val_ty_str, gep_reg));
                }
                instructions.join("\n  ")
            }
            IrInstruction::Binary {
                dest,
                op,
                left,
                right,
                operand_ty,
                result_ty: _,
            } => {
                let comparison_ty = if left.get_type().nullable_scalar_inner().is_some() { left.get_type() }
                    else if right.get_type().nullable_scalar_inner().is_some() { right.get_type() }
                    else { operand_ty.clone() };
                let operand_ty = &comparison_ty;
                if let Some(inner) = operand_ty.nullable_scalar_inner() {
                    if matches!(op, BinaryOp::Equal | BinaryOp::NotEqual) {
                        let (mut instructions, lhs) = self.coerce(left, operand_ty);
                        let (casts, rhs) = self.coerce(right, operand_ty);
                        instructions.extend(casts);
                        let name = format!("option_cmp_{}", self.string_counter);
                        self.string_counter += 1;
                        let llvm = self.type_to_llvm(operand_ty);
                        instructions.push(format!("%{}_lp = extractvalue {} {}, 0", name, llvm, lhs));
                        instructions.push(format!("%{}_rp = extractvalue {} {}, 0", name, llvm, rhs));
                        instructions.push(format!("%{}_lv = extractvalue {} {}, 1", name, llvm, lhs));
                        instructions.push(format!("%{}_rv = extractvalue {} {}, 1", name, llvm, rhs));
                        instructions.push(format!("%{}_same_tag = icmp eq i1 %{}_lp, %{}_rp", name, name, name));
                        let comparison = if *inner == Type::Decimal { "fcmp oeq" } else { "icmp eq" };
                        instructions.push(format!("%{}_same_value = {} {} %{}_lv, %{}_rv", name, comparison, self.type_to_llvm(inner), name, name));
                        instructions.push(format!("%{}_absent = xor i1 %{}_lp, true", name, name));
                        instructions.push(format!("%{}_value_ok = or i1 %{}_absent, %{}_same_value", name, name, name));
                        instructions.push(format!("%{}_eq = and i1 %{}_same_tag, %{}_value_ok", name, name, name));
                        instructions.push(format!("%{} = xor i1 %{}_eq, {}", dest.0, name, if *op == BinaryOp::NotEqual { "true" } else { "false" }));
                        return instructions.join("\n  ");
                    }
                }
                let ty_str = self.type_to_llvm(operand_ty);
                let l_str = self.emit_operand(left);
                let r_str = self.emit_operand(right);
                if matches!(op, BinaryOp::Equal | BinaryOp::NotEqual)
                    && (operand_ty == &Type::String || matches!(operand_ty, Type::Nullable(inner) if inner.as_ref() == &Type::String)) {
                    let equal = format!("string_equal_{}", self.string_counter);
                    self.string_counter += 1;
                    return format!("%{} = call i1 @ube_string_equal(i8* {}, i8* {})\n  %{} = xor i1 %{}, {}", equal, l_str, r_str, dest.0, equal, if *op == BinaryOp::NotEqual { "true" } else { "false" });
                }
                match op {
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
                            BinaryOp::Add => {
                                if operand_ty == &Type::Decimal {
                                    "fadd"
                                } else {
                                    "add"
                                }
                            }
                            BinaryOp::Sub => {
                                if operand_ty == &Type::Decimal {
                                    "fsub"
                                } else {
                                    "sub"
                                }
                            }
                            BinaryOp::Mul => {
                                if operand_ty == &Type::Decimal {
                                    "fmul"
                                } else {
                                    "mul"
                                }
                            }
                            BinaryOp::Div => {
                                if operand_ty == &Type::Decimal {
                                    "fdiv"
                                } else {
                                    "sdiv"
                                }
                            }
                            BinaryOp::Mod => {
                                if operand_ty == &Type::Decimal {
                                    "frem"
                                } else {
                                    "srem"
                                }
                            }
                            BinaryOp::Equal => {
                                if operand_ty == &Type::Decimal {
                                    "fcmp oeq"
                                } else {
                                    "icmp eq"
                                }
                            }
                            BinaryOp::NotEqual => {
                                if operand_ty == &Type::Decimal {
                                    "fcmp one"
                                } else {
                                    "icmp ne"
                                }
                            }
                            BinaryOp::Less => {
                                if operand_ty == &Type::Decimal {
                                    "fcmp olt"
                                } else {
                                    "icmp slt"
                                }
                            }
                            BinaryOp::LessEqual => {
                                if operand_ty == &Type::Decimal {
                                    "fcmp ole"
                                } else {
                                    "icmp sle"
                                }
                            }
                            BinaryOp::Greater => {
                                if operand_ty == &Type::Decimal {
                                    "fcmp ogt"
                                } else {
                                    "icmp sgt"
                                }
                            }
                            BinaryOp::GreaterEqual => {
                                if operand_ty == &Type::Decimal {
                                    "fcmp oge"
                                } else {
                                    "icmp sge"
                                }
                            }
                            BinaryOp::And => "and",
                            BinaryOp::Or => "or",
                            other => panic!("Unsupported binary operator lowering: {:?}", other),
                        };
                        format!("%{} = {} {} {}, {}", dest.0, op_str, ty_str, l_str, r_str)
                    }
                }
            }
            IrInstruction::InstantiateClass {
                dest,
                class_name,
                args,
            } => {
                let class_type = format!("%{}", class_name);
                let bitcast_reg = format!("obj_{}_{}", dest.0, self.string_counter);
                let sz_ptr = format!("sz_ptr_{}", self.string_counter);
                let sz_val = format!("sz_val_{}", self.string_counter);
                self.string_counter += 1;

                let mut instrs = Vec::new();
                instrs.push(format!(
                    "%{} = getelementptr {}, {}* null, i32 1",
                    sz_ptr, class_type, class_type
                ));
                instrs.push(format!(
                    "%{} = ptrtoint {}* %{} to i64",
                    sz_val, class_type, sz_ptr
                ));
                let edge_count = self.class_map.get(class_name).map(|class| class.fields.iter().filter(|(_, ty)| self.type_to_llvm(ty).ends_with('*')).count()).unwrap_or(0);
                instrs.push(format!("%gc_allocation_{} = call {{ i8*, i64 }} @ube_gc_allocate(i64 %{}, i64 8, i64* getelementptr inbounds ([{} x i64], [{} x i64]* @ube_gc_layout_{}, i32 0, i32 0), i64 {})", dest.0, sz_val, edge_count, edge_count, class_name, edge_count));
                instrs.push(format!("%{} = extractvalue {{ i8*, i64 }} %gc_allocation_{}, 0", dest.0, dest.0));
                instrs.push(format!("%gc_handle_{} = extractvalue {{ i8*, i64 }} %gc_allocation_{}, 1", dest.0, dest.0));
                instrs.push(format!(
                    "%{} = bitcast i8* %{} to {}*",
                    bitcast_reg, dest.0, class_type
                ));

                let field_types: Vec<Type> = self
                    .class_map
                    .get(class_name)
                    .map(|c| c.fields.iter().map(|(_, ty)| ty.clone()).collect())
                    .unwrap_or_default();

                for (i, arg) in args.iter().enumerate() {
                    let field_ty = field_types
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| arg.get_type());
                    let field_ty_str = self.type_to_llvm(&field_ty);
                    let gep_reg = format!("gep_inst_{}_{}", dest.0, i);
                    let (casts, val_str) = self.coerce(arg, &field_ty);
                    instrs.extend(casts);

                    instrs.push(format!(
                        "%{} = getelementptr inbounds {}, {}* %{}, i32 0, i32 {}",
                        gep_reg, class_type, class_type, bitcast_reg, i
                    ));

                    if field_ty_str.ends_with('*') {
                        let (casts, pointer) = self.gc_pointer(&val_str, &field_ty);
                        instrs.extend(casts);
                        instrs.push(format!("%gc_field_offset_{}_{} = ptrtoint {}* %{} to i64", dest.0, i, field_ty_str, gep_reg));
                        instrs.push(format!("%gc_field_base_{}_{} = ptrtoint i8* %{} to i64", dest.0, i, dest.0));
                        instrs.push(format!("%gc_field_index_{}_{} = sub i64 %gc_field_offset_{}_{}, %gc_field_base_{}_{}", dest.0, i, dest.0, i, dest.0, i));
                        instrs.push(format!("call void @ube_gc_store_reference(i8* %{}, i64 %gc_field_index_{}_{}, i8* {})", dest.0, dest.0, i, pointer));
                    } else { instrs.push(format!("store {} {}, {}* %{}, align 1", field_ty_str, val_str, field_ty_str, gep_reg)); }
                }
                let type_name = self.emit_operand(&IrOperand::ConstantString(class_name.clone()));
                instrs.push(format!("call void @ube_object_register(i8* %{}, i8* {})", dest.0, type_name));
                instrs.join("\n  ")
            }
            IrInstruction::CallTrait { dest, signature, receiver, args, ret_ty } => {
                let index = self.string_counter;
                self.string_counter += 1;
                let key = self.emit_operand(&IrOperand::ConstantString(signature.clone()));
                let object = self.emit_operand(receiver);
                let mut instructions = vec![format!("%trait_method_{} = call i8* @ube_object_method(i8* {}, i8* {})", index, object, key)];
                let params = std::iter::once("i8*".to_string()).chain(args.iter().map(|a| self.type_to_llvm(&a.get_type()))).collect::<Vec<_>>().join(", ");
                instructions.push(format!("%trait_fn_{} = bitcast i8* %trait_method_{} to {} ({})*", index, index, self.type_to_llvm(ret_ty), params));
                let values = std::iter::once(format!("i8* {}", object)).chain(args.iter().map(|a| format!("{} {}", self.type_to_llvm(&a.get_type()), self.emit_operand(a)))).collect::<Vec<_>>().join(", ");
                let prefix = dest.map_or(String::new(), |r| format!("%{} = ", r.0));
                instructions.push(format!("{}call {} %trait_fn_{}({})", prefix, self.type_to_llvm(ret_ty), index, values));
                instructions.join("\n  ")
            }
            IrInstruction::CallMethod {
                dest,
                class_name,
                method_name,
                receiver,
                args,
                ret_ty,
            } => {
                let fn_symbol = format!("{}_{}", class_name, method_name);
                let ret_ty_str = self.type_to_llvm(ret_ty);
                let rx_str = self.emit_operand(receiver);
                let rx_ty_str = self.type_to_llvm(&receiver.get_type());

                let params = self.signatures.get(&(class_name.clone(), method_name.clone())).cloned().unwrap_or_default();
                let mut instructions = Vec::new();
                let mut formatted_args = Vec::new();
                for (i, arg) in args.iter().enumerate() {
                    let ty = params.get(i).cloned().unwrap_or_else(|| arg.get_type());
                    let (casts, value) = self.coerce(arg, &ty);
                    instructions.extend(casts);
                    formatted_args.push(format!("{} {}", self.type_to_llvm(&ty), value));
                }
                let all_args = if formatted_args.is_empty() { format!("{} {}", rx_ty_str, rx_str) }
                    else { format!("{} {}, {}", rx_ty_str, rx_str, formatted_args.join(", ")) };
                let call = if ret_ty == &Type::Void { format!("call void @{}({})", fn_symbol, all_args) }
                    else if let Some(r) = dest { format!("%{} = call {} @{}({})", r.0, ret_ty_str, fn_symbol, all_args) }
                    else { format!("call {} @{}({})", ret_ty_str, fn_symbol, all_args) };
                instructions.push(call);
                instructions.join("\n  ")
            }
            IrInstruction::CallRuntime {
                dest,
                symbol,
                args,
                ret_ty,
            } => {
                let ret_ty_str = self.type_to_llvm(ret_ty);
                let emitted_symbol = if symbol == "ube_channel_send_async" { "ube_channel_send_value_async" } else { symbol.as_str() };
                let mut instrs = Vec::new();
                let mut formatted_args = Vec::new();
                if symbol == "ube_format_value" {
                    let value = &args[0];
                    let ty = value.get_type();
                    let inner = ty.nullable_scalar_inner().unwrap_or(&ty);
                    let tag = match inner { Type::Int | Type::Long | Type::Byte => 0, Type::Decimal => 1, Type::Bool => 2, _ => 3 };
                    let name = format!("format_value_{}", self.string_counter);
                    self.string_counter += 1;
                    let operand = self.emit_operand(value);
                    let encoded = self.encode_value(&ty, &operand, &name);
                    instrs.extend(encoded.0);
                    if let Some(dest) = dest {
                        instrs.push(format!("%{} = call i8* @ube_format_value(i32 {}, i64 {}, i64 {})", dest.0, tag, encoded.1, encoded.2));
                    }
                    return instrs.join("\n  ");
                }
                if symbol.starts_with("ube_output_") {
                    if let Some(value) = args.get(1) {
                        let mode = self.emit_operand(&args[0]);
                        if let Some(inner) = value.get_type().nullable_scalar_inner() {
                            let llvm = self.type_to_llvm(&value.get_type());
                            let operand = self.emit_operand(value);
                            let name = format!("output_option_{}", self.string_counter);
                            self.string_counter += 1;
                            let null_text = self.emit_operand(&IrOperand::ConstantString("null".into()));
                            let (output_symbol, output_ty, output_value, conversion) = match inner {
                                Type::Int | Type::Byte => ("ube_output_i64", "i64", format!("%{name}_wide"), format!("%{name}_wide = {} {} %{name}_value to i64\n  ", if *inner == Type::Int { "sext" } else { "zext" }, self.type_to_llvm(inner))),
                                Type::Long => ("ube_output_i64", "i64", format!("%{name}_value"), String::new()),
                                Type::Bool => ("ube_output_bool", "i1", format!("%{name}_value"), String::new()),
                                _ => ("ube_output_f64", "double", format!("%{name}_value"), String::new()),
                            };
                            return format!("%{name}_present = extractvalue {llvm} {operand}, 0\n  br i1 %{name}_present, label %{name}_some, label %{name}_null\n{name}_some:\n  %{name}_value = extractvalue {llvm} {operand}, 1\n  {conversion}call void @{output_symbol}(i32 {mode}, {output_ty} {output_value})\n  br label %{name}_end\n{name}_null:\n  call void @ube_output_str(i32 {mode}, i8* {null_text})\n  br label %{name}_end\n{name}_end:");
                        }
                    }
                }
                if symbol == "ube_channel_recv" {
                    let channel = args
                        .first()
                        .map(|arg| self.emit_operand(arg))
                        .unwrap_or_else(|| "null".into());
                    let raw = format!("channel_value_{}", self.string_counter);
                    self.string_counter += 1;
                    instrs.push(format!(
                        "%{} = call i64 @ube_channel_recv(i8* {})",
                        raw, channel
                    ));
                    if let Some(register) = dest {
                        let decoded =
                            self.decode_i64(ret_ty, &format!("%{}", raw), &register.0.to_string());
                        instrs.extend(decoded.0);
                    }
                    return instrs.join("\n  ");
                }
                for (index, a) in args.iter().enumerate() {
                    let arg_ty = a.get_type();
                    if symbol == "ube_channel_send_async" && index == 1 {
                        let value = self.emit_operand(a);
                        let encoded = self.encode_value(&arg_ty, &value, &format!("channel_value_{}", self.string_counter));
                        self.string_counter += 1;
                        instrs.extend(encoded.0);
                        formatted_args.push(format!("i64 {}", encoded.1));
                        formatted_args.push(format!("i64 {}", encoded.2));
                        continue;
                    }

                    if symbol == "ube_task_create"
                        || (symbol == "ube_list_add" && index == 1)
                        || (matches!(
                            symbol.as_str(),
                            "ube_channel_send" | "ube_channel_send_async"
                        ) && index == 1)
                    {
                        let value = self.emit_operand(a);
                        let encoded = self.encode_i64(
                            &arg_ty,
                            &value,
                            &format!("task_value_{}", self.string_counter),
                        );
                        self.string_counter += 1;
                        instrs.extend(encoded.0);
                        formatted_args.push(format!("i64 {}", encoded.1));
                        continue;
                    }
                    if (symbol == "ube_channel_create_capacity"
                        || (symbol == "ube_task_timeout" && index == 1))
                        && arg_ty == Type::Int
                    {
                        let ext_reg = format!("channel_capacity_{}", self.string_counter);
                        self.string_counter += 1;
                        instrs.push(format!(
                            "%{} = sext i32 {} to i64",
                            ext_reg,
                            self.emit_operand(a)
                        ));
                        formatted_args.push(format!("i64 %{}", ext_reg));
                        continue;
                    }
                    if symbol == "ube_output_i64" && index == 1 && matches!(arg_ty, Type::Int | Type::Byte) {
                        let ext_reg = format!("ext_{}", self.string_counter);
                        self.string_counter += 1;
                        let val_str = self.emit_operand(a);
                        instrs.push(format!("%{} = {} {} {} to i64", ext_reg, if arg_ty == Type::Byte { "zext" } else { "sext" }, self.type_to_llvm(&arg_ty), val_str));
                        formatted_args.push(format!("i64 %{}", ext_reg));
                    } else {
                        formatted_args.push(format!(
                            "{} {}",
                            self.type_to_llvm(&arg_ty),
                            self.emit_operand(a)
                        ));
                    }
                }
                let args_str = formatted_args.join(", ");
                let call_str = if ret_ty == &Type::Void || ret_ty_str == "void" {
                    format!("call void @{}({})", symbol, args_str)
                } else if let Some(r) = dest {
                    format!("%{} = call {} @{}({})", r.0, ret_ty_str, emitted_symbol, args_str)
                } else {
                    format!("call {} @{}({})", ret_ty_str, emitted_symbol, args_str)
                };
                if instrs.is_empty() {
                    call_str
                } else {
                    instrs.push(call_str);
                    instrs.join("\n  ")
                }
            }
            IrInstruction::ServerRoute {
                server,
                path,
                method,
                context,
                handler_symbol,
            } => {
                let context_ty = self.type_to_llvm(&context.get_type());
                let route_index = self.string_counter;
                self.string_counter += 1;
                let context_value = self.emit_operand(context);
                let server_ty = self.type_to_llvm(&server.get_type());
                let server_value = self.emit_operand(server);
                let path_value = self.emit_operand(path);
                let method = self.emit_operand(&IrOperand::ConstantString(method.clone()));
                let handler_type = format!(
                    "{} ({}*, %Request*)*",
                    self.type_to_llvm(&Type::Class {
                        name: "Response".into()
                    }),
                    context_ty.trim_end_matches('*')
                );
                let function_pointer = format!("route_handler_{}", route_index);
                let context_name = if context_ty == "i8*" {
                    context_value.clone()
                } else {
                    format!("%route_context_{}", route_index)
                };
                let mut instructions = Vec::new();
                if context_ty != "i8*" {
                    instructions.push(format!(
                        "%route_context_{} = bitcast {} {} to i8*",
                        route_index, context_ty, context_value
                    ));
                }
                instructions.push(format!(
                    "%{} = ptrtoint {} @{} to i64",
                    function_pointer, handler_type, handler_symbol
                ));
                instructions.push(format!(
                    "call void @ube_server_route({} {}, i8* {}, i8* {}, i8* {}, i64 %{})",
                    server_ty, server_value, path_value, method, context_name, function_pointer
                ));
                instructions.join("\n  ")
            }
            IrInstruction::SpawnAsyncCall {
                dest,
                callback,
                class_name,
                method_name,
                receiver,
                args,
                result_ty,
            } => self.emit_async_call(
                *dest,
                callback,
                class_name,
                method_name,
                receiver,
                args,
                result_ty,
            ),
            IrInstruction::SpawnAsyncExpression {
                dest,
                callback,
                class_name,
                method_name,
                captures,
                result_ty,
            } => self.emit_async_expression(
                *dest,
                callback,
                class_name,
                method_name,
                captures,
                result_ty,
            ),
            IrInstruction::AwaitTask {
                dest,
                error_dest,
                task,
                result_ty,
            } => {
                let task_value = self.emit_operand(task);
                let raw = format!("task_raw_{}", self.string_counter);
                self.string_counter += 1;
                let mut instructions = Vec::new();
                instructions.push(format!(
                    "%{} = call {{ i64, i64 }} @ube_task_await_value(i8* {})",
                    raw, task_value
                ));
                let decoded = self.decode_value(result_ty, &format!("%{}", raw), &dest.0.to_string());
                instructions.extend(decoded.0);
                instructions.push(format!(
                    "%{} = call i8* @ube_exception_take()",
                    error_dest.0
                ));
                instructions.join("\n  ")
            }
            IrInstruction::Branch {
                cond,
                then_block,
                else_block,
            } => {
                let cond_str = self.emit_operand(cond);
                format!(
                    "br i1 {}, label %{}, label %{}",
                    cond_str, then_block, else_block
                )
            }
            IrInstruction::Jump { target } => {
                format!("br label %{}", target)
            }
            IrInstruction::Return { val } => {
                if let Some(v) = val {
                    let v_ty = v.get_type();
                    let ty_str = if matches!(v, IrOperand::Null | IrOperand::NullOf(_))
                        || matches!(v_ty, Type::Nullable(_))
                    {
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

    fn gc_pointer(&mut self, value: &str, ty: &Type) -> (Vec<String>, String) {
        let llvm = self.type_to_llvm(ty);
        if llvm == "i8*" { return (Vec::new(), value.into()); }
        let name = format!("gc_pointer_{}", self.string_counter); self.string_counter += 1;
        (vec![format!("%{} = bitcast {} {} to i8*", name, llvm, value)], format!("%{}", name))
    }

    fn gc_pin(&mut self, value: &str, ty: &Type, key: usize) -> String {
        if !self.type_to_llvm(ty).ends_with('*') { return String::new(); }
        let (mut instructions, value) = self.gc_pointer(value, ty);
        instructions.push(format!("call void @ube_gc_frame_pin(i64 {}, i8* {})", key, value));
        instructions.join("\n  ")
    }

    fn gc_escape(&mut self, value: &IrOperand) -> String {
        if !self.type_to_llvm(&value.get_type()).ends_with('*') { return String::new(); }
        let operand = self.emit_operand(value);
        let (mut instructions, value) = self.gc_pointer(&operand, &value.get_type());
        instructions.push(format!("call void @ube_gc_escape(i8* {})", value));
        instructions.join("\n  ")
    }

    fn coerce(&mut self, value: &IrOperand, target: &Type) -> (Vec<String>, String) {
        let from = self.type_to_llvm(&value.get_type());
        let to = self.type_to_llvm(target);
        let operand = self.emit_operand(value);
        if matches!(value, IrOperand::Null | IrOperand::NullOf(_)) {
            return (vec![], if target.nullable_scalar_inner().is_some() { "zeroinitializer".into() } else { operand });
        }
        if from == to { return (vec![], operand); }
        if let (Some(source), Some(inner)) = (value.get_type().nullable_scalar_inner(), target.nullable_scalar_inner()) {
            let name = format!("nullable_convert_{}", self.string_counter);
            self.string_counter += 1;
            let source_llvm = self.type_to_llvm(source);
            let inner_llvm = self.type_to_llvm(inner);
            let mut lines = vec![format!("%{name}_present = extractvalue {from} {operand}, 0"), format!("%{name}_payload = extractvalue {from} {operand}, 1")];
            let op = if source_llvm == "i32" && inner_llvm == "i64" { "sext" }
                else if source_llvm == "i64" && inner_llvm == "i32" { "trunc" }
                else if inner_llvm == "double" { "sitofp" } else { "bitcast" };
            lines.push(format!("%{name}_converted = {op} {source_llvm} %{name}_payload to {inner_llvm}"));
            lines.push(format!("%{name}_tagged = insertvalue {to} zeroinitializer, i1 %{name}_present, 0"));
            lines.push(format!("%{name} = insertvalue {to} %{name}_tagged, {inner_llvm} %{name}_converted, 1"));
            return (lines, format!("%{name}"));
        }
        if let Some(inner) = target.nullable_scalar_inner() {
            let (mut instructions, payload) = self.coerce(value, inner);
            let name = format!("nullable_{}", self.string_counter);
            self.string_counter += 1;
            instructions.push(format!("%{}_tag = insertvalue {} zeroinitializer, i1 true, 0", name, to));
            instructions.push(format!("%{} = insertvalue {} %{}_tag, {} {}, 1", name, to, name, self.type_to_llvm(inner), payload));
            return (instructions, format!("%{}", name));
        }
        if value.get_type().nullable_scalar_inner().is_some() {
            let name = format!("unwrapped_{}", self.string_counter);
            self.string_counter += 1;
            return (vec![format!("%{} = extractvalue {} {}, 1", name, from, operand)], format!("%{}", name));
        }
        let name = format!("coerce_{}", self.string_counter);
        self.string_counter += 1;
        let op = if from.ends_with('*') && to.ends_with('*') { "bitcast" }
            else if from == "i32" && to == "i64" { "sext" }
            else if from.starts_with('i') && to.starts_with('i') {
                let source_bits = from[1..].parse::<u32>().unwrap_or(0);
                let target_bits = to[1..].parse::<u32>().unwrap_or(0);
                if source_bits > target_bits { "trunc" } else if from == "i32" || from == "i64" { "sext" } else { "zext" }
            }
            else if (from == "i32" || from == "i64") && to == "double" { "sitofp" }
            else { "bitcast" };
        (vec![format!("%{} = {} {} {} to {}", name, op, from, operand, to)], format!("%{}", name))
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
                format!(
                    "getelementptr inbounds ([{} x i8], [{} x i8]* @{}, i32 0, i32 0)",
                    v.as_bytes().len() + 1,
                    v.as_bytes().len() + 1,
                    label
                )
            }
            IrOperand::ConstantBool(v) => (if *v { "true" } else { "false" }).to_string(),
            IrOperand::NullOf(ty) if ty.nullable_scalar_inner().is_some() => "zeroinitializer".into(),
            IrOperand::Null | IrOperand::NullOf(_) => "null".to_string(),
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
            Type::Bytes | Type::InputStream | Type::OutputStream | Type::FileStream => {
                "i8*".to_string()
            }
            Type::Void => "void".to_string(),
            Type::Task(_) => "i8*".to_string(),
            Type::Channel(_) => "i8*".to_string(),
            Type::Class { name }
                if matches!(name.as_str(), "Task" | "Channel" | "List" | "Map") =>
            {
                "i8*".to_string()
            }
            Type::Class { name } => format!("%{}*", name),
            Type::Orm { kind, .. } => format!("%{}*", if *kind == crate::orm::Kind::Rows { "SQLResult" } else { kind.class() }),
            Type::Nullable(inner) => {
                if ty.nullable_scalar_inner().is_some() {
                    return format!("{{ i1, {} }}", self.type_to_llvm(inner));
                }
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

    fn encode_i64(&self, ty: &Type, value: &str, name: &str) -> (Vec<String>, String) {
        match ty {
            Type::Int => (
                vec![format!("%{} = sext i32 {} to i64", name, value)],
                format!("%{}", name),
            ),
            Type::Byte => (
                vec![format!("%{} = zext i8 {} to i64", name, value)],
                format!("%{}", name),
            ),
            Type::Bool => (
                vec![format!("%{} = zext i1 {} to i64", name, value)],
                format!("%{}", name),
            ),
            Type::Decimal => (
                vec![format!("%{} = bitcast double {} to i64", name, value)],
                format!("%{}", name),
            ),
            Type::Void => (Vec::new(), "0".to_string()),
            Type::Long => (Vec::new(), value.to_string()),
            Type::Channel(_) => (
                vec![format!("%{} = ptrtoint i8* {} to i64", name, value)],
                format!("%{}", name),
            ),
            _ => (
                vec![format!(
                    "%{} = ptrtoint {} {} to i64",
                    name,
                    self.type_to_llvm(ty),
                    value
                )],
                format!("%{}", name),
            ),
        }
    }

    fn decode_i64(&self, ty: &Type, value: &str, name: &str) -> (Vec<String>, String) {
        match ty {
            Type::Int => (
                vec![format!("%{} = trunc i64 {} to i32", name, value)],
                format!("%{}", name),
            ),
            Type::Byte => (
                vec![format!("%{} = trunc i64 {} to i8", name, value)],
                format!("%{}", name),
            ),
            Type::Bool => (
                vec![format!("%{} = trunc i64 {} to i1", name, value)],
                format!("%{}", name),
            ),
            Type::Decimal => (
                vec![format!("%{} = bitcast i64 {} to double", name, value)],
                format!("%{}", name),
            ),
            Type::Void => (Vec::new(), "0".to_string()),
            Type::Long => (vec![format!("%{} = add i64 {}, 0", name, value)], format!("%{}", name)),
            Type::Channel(_) => (
                vec![format!("%{} = inttoptr i64 {} to i8*", name, value)],
                format!("%{}", name),
            ),
            _ => (
                vec![format!(
                    "%{} = inttoptr i64 {} to {}",
                    name,
                    value,
                    self.type_to_llvm(ty)
                )],
                format!("%{}", name),
            ),
        }
    }

    fn encode_value(&self, ty: &Type, value: &str, name: &str) -> (Vec<String>, String, String) {
        if let Some(inner) = ty.nullable_scalar_inner() {
            let llvm = self.type_to_llvm(ty);
            let mut lines = vec![format!("%{name}_present = extractvalue {llvm} {value}, 0"), format!("%{name}_value = extractvalue {llvm} {value}, 1"), format!("%{name}_meta = zext i1 %{name}_present to i64")];
            let encoded = self.encode_i64(inner, &format!("%{name}_value"), name);
            lines.extend(encoded.0);
            (lines, encoded.1, format!("%{name}_meta"))
        } else {
            let encoded = self.encode_i64(ty, value, name);
            (encoded.0, encoded.1, "1".into())
        }
    }

    fn decode_value(&self, ty: &Type, value: &str, name: &str) -> (Vec<String>, String) {
        let temp = format!("transport_{name}");
        let mut lines = vec![format!("%{temp}_payload = extractvalue {{ i64, i64 }} {value}, 0")];
        if let Some(inner) = ty.nullable_scalar_inner() {
            lines.push(format!("%{temp}_meta = extractvalue {{ i64, i64 }} {value}, 1"));
            lines.push(format!("%{temp}_present = icmp ne i64 %{temp}_meta, 0"));
            let decoded = self.decode_i64(inner, &format!("%{temp}_payload"), &format!("{temp}_value"));
            lines.extend(decoded.0);
            let llvm = self.type_to_llvm(ty);
            lines.push(format!("%{temp}_tagged = insertvalue {llvm} zeroinitializer, i1 %{temp}_present, 0"));
            lines.push(format!("%{name} = insertvalue {llvm} %{temp}_tagged, {} {}, 1", self.type_to_llvm(inner), decoded.1));
            (lines, format!("%{name}"))
        } else {
            let decoded = self.decode_i64(ty, &format!("%{temp}_payload"), name);
            lines.extend(decoded.0);
            (lines, decoded.1)
        }
    }

    fn emit_async_call(
        &mut self,
        dest: Register,
        callback: &str,
        class_name: &str,
        method_name: &str,
        receiver: &IrOperand,
        args: &[IrOperand],
        result_ty: &Type,
    ) -> String {
        let mut instructions = vec![format!(
            "%task_ctx_{} = call i8* @ube_task_context_create(i64 {})",
            dest.0,
            args.len() + 1
        )];
        let mut captures = vec![(receiver.get_type(), self.emit_operand(receiver))];
        captures.extend(
            args.iter()
                .map(|arg| (arg.get_type(), self.emit_operand(arg))),
        );
        for (index, (ty, value)) in captures.iter().enumerate() {
            let encoded = self.encode_value(ty, value, &format!("task_capture_{}_{}", dest.0, index));
            instructions.extend(encoded.0);
            instructions.push(format!(
                "call void @ube_task_context_set_value(i8* %task_ctx_{}, i64 {}, i64 {}, i64 {})",
                dest.0, index, encoded.1, encoded.2
            ));
        }
        instructions.push(format!(
            "%task_fn_{} = ptrtoint {{ i64, i64 }} (i8*)* @{} to i64",
            dest.0, callback
        ));
        instructions.push(format!(
            "%{} = call i8* @ube_task_spawn_value(i64 %task_fn_{}, i8* %task_ctx_{})",
            dest.0, dest.0, dest.0
        ));
        self.task_thunks.push(self.emit_async_thunk(
            callback,
            class_name,
            method_name,
            &captures,
            result_ty,
        ));
        instructions.join("\n  ")
    }

    fn emit_async_expression(
        &mut self,
        dest: Register,
        callback: &str,
        class_name: &str,
        method_name: &str,
        captures: &[IrOperand],
        result_ty: &Type,
    ) -> String {
        let mut instructions = vec![format!(
            "%task_ctx_{} = call i8* @ube_task_context_create(i64 {})",
            dest.0,
            captures.len() + 1
        )];
        let mut encoded_captures = vec![(Type::Int, "0".to_owned())];
        encoded_captures.extend(
            captures
                .iter()
                .map(|capture| (capture.get_type(), self.emit_operand(capture))),
        );
        for (index, (ty, value)) in encoded_captures.iter().enumerate() {
            let encoded = self.encode_value(ty, value, &format!("task_capture_{}_{}", dest.0, index));
            instructions.extend(encoded.0);
            instructions.push(format!(
                "call void @ube_task_context_set_value(i8* %task_ctx_{}, i64 {}, i64 {}, i64 {})",
                dest.0, index, encoded.1, encoded.2
            ));
        }
        instructions.push(format!(
            "%task_fn_{} = ptrtoint {{ i64, i64 }} (i8*)* @{} to i64",
            dest.0, callback
        ));
        instructions.push(format!(
            "%{} = call i8* @ube_task_spawn_value(i64 %task_fn_{}, i8* %task_ctx_{})",
            dest.0, dest.0, dest.0
        ));
        self.task_thunks.push(self.emit_async_thunk(
            callback,
            class_name,
            method_name,
            &encoded_captures,
            result_ty,
        ));
        instructions.join("\n  ")
    }

    fn emit_async_thunk(
        &self,
        callback: &str,
        class_name: &str,
        method_name: &str,
        captures: &[(Type, String)],
        result_ty: &Type,
    ) -> String {
        let mut body = vec!["entry:".to_string()];
        let mut arguments = Vec::new();
        for (index, (ty, _)) in captures.iter().enumerate() {
            let raw = format!("%{}_raw_{}", callback, index);
            body.push(format!(
                "  {} = call {{ i64, i64 }} @ube_task_context_get_value(i8* %ctx, i64 {})",
                raw, index
            ));
            let decoded = self.decode_value(ty, &raw, &format!("{}_capture_{}", callback, index));
            body.extend(decoded.0.into_iter().map(|line| format!("  {}", line)));
            arguments.push(format!("{} {}", self.type_to_llvm(ty), decoded.1));
        }
        let call = format!(
            "call {} @{}_{}({})",
            self.type_to_llvm(result_ty),
            class_name,
            method_name,
            arguments.join(", ")
        );
        if *result_ty == Type::Void {
            body.push(format!("  {}", call));
            body.push("  ret { i64, i64 } zeroinitializer".to_string());
        } else {
            body.push(format!("  %{}_result = {}", callback, call));
            let encoded = self.encode_value(
                result_ty,
                &format!("%{}_result", callback),
                &format!("{}_encoded", callback),
            );
            body.extend(encoded.0.into_iter().map(|line| format!("  {}", line)));
            body.push(format!("  %{}_payload = insertvalue {{ i64, i64 }} zeroinitializer, i64 {}, 0", callback, encoded.1));
            body.push(format!("  %{}_transport = insertvalue {{ i64, i64 }} %{}_payload, i64 {}, 1", callback, callback, encoded.2));
            body.push(format!("  ret {{ i64, i64 }} %{}_transport", callback));
        }
        format!(
            "define internal {{ i64, i64 }} @{}(i8* %ctx) {{\n{}\n}}\n",
            callback,
            body.join("\n")
        )
    }
}
