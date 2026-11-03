use std::alloc::{alloc, dealloc, Layout};
use std::ffi::c_void;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn allocations() -> &'static Mutex<HashMap<usize, Layout>> {
    static ALLOCATIONS: OnceLock<Mutex<HashMap<usize, Layout>>> = OnceLock::new();
    ALLOCATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn cleanup() {
    let allocations = std::mem::take(&mut *allocations().lock().unwrap());
    for (pointer, layout) in allocations {
        unsafe { dealloc(pointer as *mut u8, layout); }
    }
}


#[no_mangle]
pub extern "C" fn ube_alloc(size: usize, align: usize) -> *mut c_void {
    let size = size.max(1);
    let align = if align == 0 { 8 } else { align };
    match Layout::from_size_align(size, align) {
        Ok(layout) => {
            let pointer = unsafe { alloc(layout) };
            if !pointer.is_null() { allocations().lock().unwrap().insert(pointer as usize, layout); }
            pointer as *mut c_void
        },
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
    let registered = allocations().lock().unwrap().remove(&(ptr as usize));
    if let Some(layout) = registered.or_else(|| Layout::from_size_align(size, align).ok()) {
        unsafe {
            dealloc(ptr as *mut u8, layout);
        }
    }
}
