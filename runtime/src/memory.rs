use std::alloc::{alloc, dealloc, Layout};
use std::ffi::c_void;

#[no_mangle]
pub extern "C" fn ube_alloc(size: usize, align: usize) -> *mut c_void {
    let size = size.max(1);
    let align = if align == 0 { 8 } else { align };
    match Layout::from_size_align(size, align) {
        Ok(layout) => unsafe { alloc(layout) as *mut c_void },
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn ube_free(ptr: *mut c_void, size: usize, align: usize) {
    if ptr.is_null() {
        return;
    }
    let size = size.max(1);
    let align = if align == 0 { 8 } else { align };
    if let Ok(layout) = Layout::from_size_align(size, align) {
        unsafe {
            dealloc(ptr as *mut u8, layout);
        }
    }
}
