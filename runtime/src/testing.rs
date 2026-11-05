//! Assertions use the ordinary, recoverable Ube exception ABI.
use std::ffi::{c_char, CStr};

#[no_mangle]
pub extern "C" fn ube_testicle_check(condition: bool, message: *const c_char) {
    if condition {
        return;
    }
    let message = if message.is_null() {
        "Assertion failed".into()
    } else {
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned()
    };
    crate::exceptions::set_pending(crate::exceptions::named("AssertionError", message));
}
