pub mod collections;
pub mod concurrency;
pub mod datetime;
pub mod financial;
pub mod gc;
pub mod geo;
pub mod io;
pub mod math;
pub mod memory;
pub mod net;
pub mod strings;


#[no_mangle]
pub extern "C" fn talas_runtime_init() {
    // Initialize runtime heap and garbage collector
    gc::talas_gc_init();
}

#[no_mangle]
pub extern "C" fn talas_runtime_cleanup() {
    // Perform final garbage collection sweep on exit
    gc::talas_gc_collect();
}
