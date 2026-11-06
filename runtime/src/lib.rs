pub mod rest_in_peace;
pub mod mockery;
pub mod testing;
pub mod collections;
pub mod concurrency;
pub mod datetime;
pub mod financial;
pub mod gc;
pub mod exceptions;
pub mod geo;
pub mod io;
pub mod math;
pub mod logging;
pub mod memory;
pub mod net;
pub mod strings;
pub mod sqlite;
pub mod sql;
pub mod orm;
pub mod sessions;
pub mod auth;
pub mod secrets;


#[no_mangle]
pub extern "C" fn ube_runtime_init() {
    // Initialize runtime heap and garbage collector
    gc::ube_gc_init();
}

#[no_mangle]
pub extern "C" fn ube_runtime_cleanup() {
    orm::cleanup();
    sql::cleanup();
    sqlite::cleanup();
    concurrency::ube_concurrency_cleanup();
    logging::ube_log_clear();
    sessions::cleanup();
    rest_in_peace::cleanup();
    auth::cleanup();
    mockery::cleanup();
    exceptions::cleanup();
    memory::cleanup();
    // The explicit tracing heap is empty after cleanup.
    gc::ube_gc_collect();
}
