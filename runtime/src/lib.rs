pub mod rest_in_peace;
pub mod mockery;
pub mod testing;
pub mod collections;
pub mod cache;
pub mod key_value_store;
pub mod json;
pub mod xml;
pub mod concurrency;
pub mod datetime;
pub mod financial;
pub mod gc;
pub mod exceptions;
pub mod geo;
pub mod io;
pub mod math;
pub mod logging;
pub mod arguments;
pub mod observability;
pub mod memory;
pub mod net;
pub mod strings;
pub mod html;
pub mod sqlite;
pub mod sql;
pub mod sql_builder;
pub mod orm;
pub mod sessions;
pub mod auth;
pub mod secrets;
pub mod sequences;


#[no_mangle]
pub extern "C" fn ube_runtime_init() {
    // Initialize runtime heap and garbage collector
    gc::ube_gc_init();
    observability::init();
    concurrency::ube_task_scope_enter();
}

#[no_mangle]
pub extern "C" fn ube_runtime_cleanup() {
    concurrency::ube_task_scope_leave();
    concurrency::ube_concurrency_cleanup();
    key_value_store::cleanup();
    xml::cleanup();
    json::cleanup();
    cache::cleanup();
    observability::cleanup();
    orm::cleanup();
    sql_builder::cleanup();
    sql::cleanup();
    sequences::cleanup();
    sqlite::cleanup();
    logging::ube_log_clear();
    sessions::cleanup();
    rest_in_peace::cleanup();
    auth::cleanup();
    mockery::cleanup();
    exceptions::cleanup();
    arguments::cleanup();
    memory::cleanup();
    // The explicit tracing heap is empty after cleanup.
    gc::ube_gc_collect();
}
