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


#[no_mangle]
pub extern "C" fn ube_runtime_init() {
    // Initialize runtime heap and garbage collector
    gc::ube_gc_init();
}

#[no_mangle]
pub extern "C" fn ube_runtime_cleanup() {
    sqlite::cleanup();
    concurrency::ube_concurrency_cleanup();
    logging::ube_log_clear();
    exceptions::cleanup();
    memory::cleanup();
    // Perform final garbage collection sweep on exit
    gc::ube_gc_collect();
}
