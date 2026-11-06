use std::collections::HashMap;
use std::sync::Mutex;

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
    pub items: Mutex<Vec<i64>>,
}

#[no_mangle]
pub extern "C" fn ube_list_create() -> *mut UbeList {
    let list = Box::new(UbeList { items: Mutex::new(Vec::new()) });
    Box::into_raw(list)
}

#[no_mangle]
pub extern "C" fn ube_list_add(ptr: *mut UbeList, val: i64) {
    if !ptr.is_null() {
        unsafe {
            (*ptr).items.lock().unwrap().push(val);
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_list_get(ptr: *mut UbeList, idx: i64) -> i64 {
    if !ptr.is_null() {
        unsafe {
            let items = (*ptr).items.lock().unwrap();
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
            return (*ptr).items.lock().unwrap().len() as i64;
        }
    }
    0
}

#[repr(C)]
pub struct UbeMap {
    pub entries: Mutex<HashMap<i64, i64>>,
}

#[no_mangle]
pub extern "C" fn ube_map_create() -> *mut UbeMap {
    let map = Box::new(UbeMap {
        entries: Mutex::new(HashMap::new()),
    });
    Box::into_raw(map)
}

#[no_mangle]
pub extern "C" fn ube_map_put(ptr: *mut UbeMap, key: i64, val: i64) {
    if !ptr.is_null() {
        unsafe {
            (*ptr).entries.lock().unwrap().insert(key, val);
        }
    }
}

// Presence is separate from the payload: zero is a valid stored value.
#[no_mangle]
pub extern "C" fn ube_map_contains_key(ptr: *mut UbeMap, key: i64) -> bool {
    !ptr.is_null() && unsafe { &*ptr }.entries.lock().unwrap().contains_key(&key)
}

#[no_mangle]
pub extern "C" fn ube_map_get(ptr: *mut UbeMap, key: i64) -> i64 {
    if !ptr.is_null() {
        unsafe {
            if let Some(val) = (*ptr).entries.lock().unwrap().get(&key) {
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
            return (*ptr).entries.lock().unwrap().len() as i64;
        }
    }
    0
}

/// Clone one consistent view before calling code that may block or spawn tasks.
pub(crate) fn snapshot(ptr: *mut UbeList) -> Option<Vec<i64>> {
    if ptr.is_null() { None } else { Some(unsafe { &*ptr }.items.lock().unwrap().clone()) }
}

#[cfg(test)]
mod allocation_tests {
    use super::*;

    #[test]
    fn concurrent_map_writers_keep_every_entry() {
        let map = ube_map_create() as usize;
        let workers: Vec<_> = (0..4).map(|worker| std::thread::spawn(move || {
            for index in 0..10000 {
                let key = worker * 10000 + index;
                ube_map_put(map as *mut UbeMap, key, key);
                assert_eq!(ube_map_get(map as *mut UbeMap, key), key);
            }
        })).collect();
        for worker in workers { worker.join().unwrap(); }
        assert_eq!(ube_map_len(map as *mut UbeMap), 40000);
        unsafe { drop(Box::from_raw(map as *mut UbeMap)); }
    }

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
