//! Process arguments copied at native entry, excluding argv[0]. Read-only thereafter.
use std::{
    ffi::{c_char, c_void, CStr, CString},
    sync::{Mutex, OnceLock},
};
fn arguments() -> &'static Mutex<Vec<CString>> {
    static ARGUMENTS: OnceLock<Mutex<Vec<CString>>> = OnceLock::new();
    ARGUMENTS.get_or_init(Default::default)
}
fn handle() -> *mut c_void {
    arguments() as *const _ as *mut c_void
}
fn fail(message: &str) {
    crate::exceptions::set_pending(crate::exceptions::generic(message.into()));
}
#[no_mangle]
pub extern "C" fn ube_args_init(argc: i32, argv: *const *const c_char) {
    let mut values = Vec::new();
    if argc > 1 && !argv.is_null() {
        for index in 1..argc as usize {
            let pointer = unsafe { *argv.add(index) };
            if !pointer.is_null() {
                // Ube strings are UTF-8; replace invalid OS bytes deterministically.
                let text = unsafe { CStr::from_ptr(pointer) }.to_string_lossy();
                values.push(CString::new(text.as_bytes()).expect("C argument cannot contain NUL"));
            }
        }
    }
    *arguments().lock().unwrap() = values;
}
#[no_mangle]
pub extern "C" fn ube_console_args() -> *mut c_void {
    handle()
}
#[no_mangle]
pub extern "C" fn ube_args_len(args: *mut c_void) -> i32 {
    if args != handle() {
        fail("Invalid ConsoleArgs handle");
        return 0;
    }
    arguments().lock().unwrap().len() as i32
}
#[no_mangle]
pub extern "C" fn ube_args_get(args: *mut c_void, index: i32) -> *const c_char {
    if args != handle() {
        fail("Invalid ConsoleArgs handle");
        return std::ptr::null();
    }
    let values = arguments().lock().unwrap();
    match usize::try_from(index)
        .ok()
        .and_then(|index| values.get(index))
    {
        Some(value) => value.as_ptr(),
        None => {
            fail("Command-line argument index out of bounds");
            std::ptr::null()
        }
    }
}
pub(crate) fn cleanup() {
    arguments().lock().unwrap().clear();
}
