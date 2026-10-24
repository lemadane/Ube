use std::ffi::CStr;
use std::os::raw::c_char;

#[repr(C)]
pub struct UbeString {
    pub ptr: *const u8,
    pub len: usize,
}

#[no_mangle]
pub extern "C" fn ube_string_create(c_str_ptr: *const c_char) -> UbeString {
    if c_str_ptr.is_null() {
        return UbeString {
            ptr: std::ptr::null(),
            len: 0,
        };
    }
    let cstr = unsafe { CStr::from_ptr(c_str_ptr) };
    let bytes = cstr.to_bytes();
    let len = bytes.len();

    let layout = match std::alloc::Layout::from_size_align(len + 1, 1) {
        Ok(l) => l,
        Err(_) => {
            return UbeString {
                ptr: c_str_ptr as *const u8,
                len,
            }
        }
    };

    let heap_ptr = unsafe { std::alloc::alloc(layout) };
    if !heap_ptr.is_null() {
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), heap_ptr, len);
            *heap_ptr.add(len) = 0;
        }
        UbeString { ptr: heap_ptr, len }
    } else {
        UbeString {
            ptr: c_str_ptr as *const u8,
            len,
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_string_concat(a: *const c_char, b: *const c_char) -> *const c_char {
    let s_a = if a.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(a).to_str().unwrap_or("") }
    };
    let s_b = if b.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(b).to_str().unwrap_or("") }
    };
    let res = format!("{}{}\0", s_a, s_b);
    let bytes = res.as_bytes();
    let layout = match std::alloc::Layout::from_size_align(bytes.len(), 1) {
        Ok(l) => l,
        Err(_) => return std::ptr::null(),
    };
    let ptr = unsafe { std::alloc::alloc(layout) };
    if !ptr.is_null() {
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        }
        ptr as *const c_char
    } else {
        std::ptr::null()
    }
}
