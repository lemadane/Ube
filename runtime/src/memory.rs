use std::alloc::{alloc, dealloc, Layout};
use std::ffi::c_void;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn allocations() -> &'static Mutex<HashMap<usize, Layout>> {
    static ALLOCATIONS: OnceLock<Mutex<HashMap<usize, Layout>>> = OnceLock::new();
    ALLOCATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn cleanup() {
    crate::gc::cleanup();
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
pub extern "C" fn ube_free(ptr: *mut c_void, _size: usize, _align: usize) {
    if ptr.is_null() {
        return;
    }
    // Size and alignment remain in the C ABI for compatibility. Only the
    // registry establishes ownership and supplies a valid allocation layout.
    // Reject foreign, interior, and already-freed pointers without allocating
    // an error object on this allocator path.
    if crate::gc::is_managed(ptr) { return; }
    free_registered(ptr);
}

pub(crate) fn free_registered(ptr: *mut c_void) {
    let registered = allocations().lock().unwrap().remove(&(ptr as usize));
    if let Some(layout) = registered {
        unsafe {
            dealloc(ptr as *mut u8, layout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_uses_owned_layout_and_rejects_repeated_frees() {
        let pointer = ube_alloc(64, 32);
        assert!(!pointer.is_null());
        let layout = allocations().lock().unwrap()[&(pointer as usize)];
        assert_eq!((layout.size(), layout.align()), (64, 32));

        // Caller-provided layout information must never control deallocation.
        ube_free(pointer, usize::MAX, 3);
        assert!(!allocations().lock().unwrap().contains_key(&(pointer as usize)));
        ube_free(pointer, 64, 32);
    }

    #[test]
    fn free_preserves_foreign_allocations() {
        let mut foreign = Box::new(42_u64);
        let pointer = (&mut *foreign as *mut u64).cast();
        ube_free(pointer, std::mem::size_of::<u64>(), std::mem::align_of::<u64>());
        assert_eq!(*foreign, 42);
        *foreign = 99;
        assert_eq!(*foreign, 99);
        // Box retains ownership and releases its allocation exactly once.
    }

    #[test]
    fn free_rejects_interior_invalid_and_null_pointers() {
        let pointer = ube_alloc(64, 8);
        assert!(!pointer.is_null());
        let interior = unsafe { pointer.cast::<u8>().add(8) }.cast();
        ube_free(interior, 56, 8);
        assert!(allocations().lock().unwrap().contains_key(&(pointer as usize)));
        unsafe {
            pointer.cast::<u64>().write(123);
            assert_eq!(pointer.cast::<u64>().read(), 123);
        }
        ube_free(pointer, 64, 8);
        ube_free(std::ptr::dangling_mut::<u64>().cast(), 8, 8);
        ube_free(std::ptr::null_mut(), 0, 0);
    }
}

pub(crate) fn registered_range(pointer: *mut c_void, bytes: usize) -> bool {
    allocations().lock().unwrap().get(&(pointer as usize)).is_some_and(|layout| bytes <= layout.size())
}
