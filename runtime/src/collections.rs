use std::collections::HashMap;

#[repr(C)]
pub struct UbeArray {
    pub data: *mut u8,
    pub len: usize,
    pub capacity: usize,
    pub element_size: usize,
}

#[no_mangle]
pub extern "C" fn ube_array_create(capacity: usize, element_size: usize) -> UbeArray {
    // Overflow and allocation failure must never produce a usable capacity.
    let ptr = capacity
        .checked_mul(element_size)
        .map(|size| crate::memory::ube_alloc(size, 8))
        .unwrap_or(std::ptr::null_mut());
    UbeArray {
        data: ptr as *mut u8,
        len: 0,
        capacity: if ptr.is_null() { 0 } else { capacity },
        element_size,
    }
}

#[repr(C)]
pub struct UbeList {
    pub items: Vec<i64>,
}

#[no_mangle]
pub extern "C" fn ube_list_create() -> *mut UbeList {
    let list = Box::new(UbeList { items: Vec::new() });
    Box::into_raw(list)
}

#[no_mangle]
pub extern "C" fn ube_list_add(ptr: *mut UbeList, val: i64) {
    if !ptr.is_null() {
        unsafe {
            (*ptr).items.push(val);
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_list_get(ptr: *mut UbeList, idx: i64) -> i64 {
    if !ptr.is_null() {
        unsafe {
            let items = &(*ptr).items;
            if idx >= 0 && (idx as usize) < items.len() {
                return items[idx as usize];
            }
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn ube_list_len(ptr: *mut UbeList) -> i64 {
    if !ptr.is_null() {
        unsafe {
            return (*ptr).items.len() as i64;
        }
    }
    0
}

#[repr(C)]
pub struct UbeMap {
    pub entries: HashMap<i64, i64>,
}

#[no_mangle]
pub extern "C" fn ube_map_create() -> *mut UbeMap {
    let map = Box::new(UbeMap {
        entries: HashMap::new(),
    });
    Box::into_raw(map)
}

#[no_mangle]
pub extern "C" fn ube_map_put(ptr: *mut UbeMap, key: i64, val: i64) {
    if !ptr.is_null() {
        unsafe {
            (*ptr).entries.insert(key, val);
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_map_get(ptr: *mut UbeMap, key: i64) -> i64 {
    if !ptr.is_null() {
        unsafe {
            if let Some(val) = (*ptr).entries.get(&key) {
                return *val;
            }
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn ube_map_len(ptr: *mut UbeMap) -> i64 {
    if !ptr.is_null() {
        unsafe {
            return (*ptr).entries.len() as i64;
        }
    }
    0
}

#[cfg(test)]
mod allocation_tests {
    use super::*;

    #[test]
    fn failed_array_allocation_never_retains_requested_capacity() {
        // Exercise arithmetic overflow and an unrepresentable allocator layout
        // without asking the operating system for an enormous allocation.
        for element_size in [1, 2] {
            let array = ube_array_create(usize::MAX, element_size);
            assert!(array.data.is_null());
            assert_eq!(array.len, 0);
            assert_eq!(array.capacity, 0);
            assert_eq!(array.element_size, element_size);
        }
        let array = ube_array_create(4, 8);
        assert!(!array.data.is_null());
        assert_eq!(array.capacity, 4);
        assert_eq!(array.len, 0);
        unsafe {
            *array.data.add(31) = 42;
        }
        crate::memory::ube_free(array.data.cast(), 32, 8);
    }
}
