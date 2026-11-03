//! Compiler-managed exceptions. No Rust or LLVM unwinding crosses generated code.
//! Records and object identity metadata live until runtime shutdown, like task handles.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::{Mutex, OnceLock};

#[derive(Debug)]
pub struct ExceptionRecord {
    pub object: usize,
    pub type_name: String,
    pub message: CString,
    pub location: String,
    pub suppressed: Mutex<Vec<usize>>,
}

fn records() -> &'static Mutex<Vec<Box<ExceptionRecord>>> {
    static RECORDS: OnceLock<Mutex<Vec<Box<ExceptionRecord>>>> = OnceLock::new();
    RECORDS.get_or_init(|| Mutex::new(Vec::new()))
}
fn objects() -> &'static Mutex<HashMap<usize, String>> {
    static OBJECTS: OnceLock<Mutex<HashMap<usize, String>>> = OnceLock::new();
    OBJECTS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn types() -> &'static Mutex<HashMap<String, HashMap<String, usize>>> {
    static TYPES: OnceLock<Mutex<HashMap<String, HashMap<String, usize>>>> = OnceLock::new();
    TYPES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[no_mangle]
pub extern "C" fn ube_type_method(name: *const c_char, signature: *const c_char, method: *mut c_void) {
    types().lock().unwrap().entry(string(name)).or_default().insert(string(signature), method as usize);
}
#[no_mangle]
pub extern "C" fn ube_object_satisfies(object: *mut c_void, contract: *const c_char) -> bool {
    let objects = objects().lock().unwrap();
    let Some(name) = objects.get(&(object as usize)) else { return false; };
    let types = types().lock().unwrap();
    types.get(name).is_some_and(|methods| string(contract).lines().all(|required| methods.contains_key(required)))
}
#[no_mangle]
pub extern "C" fn ube_object_method(object: *mut c_void, signature: *const c_char) -> *mut c_void {
    let objects = objects().lock().unwrap();
    let method = objects.get(&(object as usize)).and_then(|name| types().lock().unwrap().get(name).and_then(|methods| methods.get(&string(signature)).copied()));
    method.unwrap_or(0) as *mut c_void
}

thread_local! {
    static PENDING: Cell<usize> = const { Cell::new(0) };
    static CANCELLATION_REPORTED: Cell<bool> = const { Cell::new(false) };
    static DIAGNOSTIC: RefCell<Option<CString>> = const { RefCell::new(None) };
}
fn string(ptr: *const c_char) -> String {
    if ptr.is_null() { String::new() } else { unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned() }
}
pub fn create(object: usize, type_name: String, message: String, location: String) -> usize {
    let mut arena = records().lock().unwrap();
    if let Some(record) = arena.iter_mut().find(|record| record.object == object && object != 0) {
        return (&mut **record as *mut ExceptionRecord) as usize;
    }
    let mut record = Box::new(ExceptionRecord { object, type_name,
        message: CString::new(message.replace('\0', "\\0")).unwrap(), location,
        suppressed: Mutex::new(Vec::new()) });
    let ptr = (&mut *record as *mut ExceptionRecord) as usize;
    arena.push(record);
    ptr
}
pub fn generic(message: String) -> usize {
    named("GenericError", message)
}
pub fn named(type_name: &str, message: String) -> usize {
    // Built-in error objects share the generated one-pointer message layout.
    let message = CString::new(message.replace('\0', "\\0")).unwrap();
    let object = crate::memory::ube_alloc(std::mem::size_of::<usize>(), 8);
    unsafe { *(object as *mut *const c_char) = message.as_ptr(); }
    let record = create(object as usize, type_name.into(), message.to_string_lossy().into_owned(), "runtime".into());
    // Store the stable record-owned CString address in the object.
    unsafe { *(object as *mut *const c_char) = (*(record as *const ExceptionRecord)).message.as_ptr(); }
    objects().lock().unwrap().insert(object as usize, type_name.into());
    record
}
pub fn set_pending(record: usize) { PENDING.with(|p| p.set(record)); }
pub fn take_pending() -> usize { PENDING.with(|p| p.replace(0)) }
pub fn mark_cancellation_reported() { CANCELLATION_REPORTED.with(|p| p.set(true)); }
pub fn reset_cancellation() { CANCELLATION_REPORTED.with(|p| p.set(false)); }
pub fn message(record: usize) -> String {
    if record == 0 { return String::new(); }
    unsafe { (*(record as *const ExceptionRecord)).message.to_string_lossy().into_owned() }
}

#[no_mangle]
pub extern "C" fn ube_object_register(object: *mut c_void, name: *const c_char) {
    objects().lock().unwrap().insert(object as usize, string(name));
}
#[no_mangle]
pub extern "C" fn ube_object_is(object: *mut c_void, name: *const c_char) -> bool {
    objects().lock().unwrap().get(&(object as usize)).is_some_and(|actual| actual == &string(name))
}
#[no_mangle]
pub extern "C" fn ube_exception_create(object: *mut c_void, name: *const c_char, message: *const c_char, location: *const c_char) -> *mut c_void {
    let name = if name.is_null() { objects().lock().unwrap().get(&(object as usize)).cloned().unwrap_or_else(|| "GenericError".into()) } else { string(name) };
    create(object as usize, name, string(message), string(location)) as *mut c_void
}
#[no_mangle]
pub extern "C" fn ube_exception_object(record: *mut c_void) -> *mut c_void {
    if record.is_null() { return std::ptr::null_mut(); }
    unsafe { (*(record as *const ExceptionRecord)).object as *mut c_void }
}
#[no_mangle]
pub extern "C" fn ube_exception_take() -> *mut c_void { take_pending() as *mut c_void }
#[no_mangle]
pub extern "C" fn ube_exception_set(record: *mut c_void) { set_pending(record as usize); }
#[no_mangle]
pub extern "C" fn ube_exception_suppress(active: *mut c_void, original: *mut c_void) {
    if !active.is_null() && !original.is_null() && active != original {
        unsafe { &*(active as *const ExceptionRecord) }.suppressed.lock().unwrap().push(original as usize);
    }
}
#[no_mangle]
pub extern "C" fn ube_exception_poll_cancel() -> *mut c_void {
    if crate::concurrency::ube_task_is_cancelled() && CANCELLATION_REPORTED.with(|p| !p.replace(true)) {
        generic("task was cancelled".into()) as *mut c_void
    } else { std::ptr::null_mut() }
}
#[no_mangle]
pub extern "C" fn ube_exception_fail_unhandled() {
    let record = take_pending();
    if record != 0 {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let error = unsafe { &*(record as *const ExceptionRecord) };
        eprintln!("Ube error: unhandled exception {}: {} at {}", error.type_name, error.message.to_string_lossy(), error.location);
        for &suppressed in error.suppressed.lock().unwrap().iter() {
            eprintln!("  suppressed: {}", message(suppressed));
        }
        crate::ube_runtime_cleanup();
        std::process::exit(1);
    }
}
pub fn cleanup() {
    set_pending(0);
    records().lock().unwrap().clear();
    objects().lock().unwrap().clear();
}
