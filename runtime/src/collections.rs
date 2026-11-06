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
    json_types: Mutex<Vec<(u64, i64, String)>>,
}

#[no_mangle]
pub extern "C" fn ube_list_create() -> *mut UbeList {
    let list = Box::new(UbeList { items: Mutex::new(Vec::new()), json_types: Mutex::new(Vec::new()) });
    Box::into_raw(list)
}

#[no_mangle]
pub extern "C" fn ube_list_add(ptr: *mut UbeList, val: i64) {
    // Legacy native callers do not carry a type; never guess from pointer bits.
    add_typed(ptr, val, 1, 0, "unknown");
}

pub(crate) fn add_typed(ptr: *mut UbeList, val: i64, meta: u64, kind: i64, name: &str) {
    if let Some(list) = unsafe { ptr.as_ref() } {
        let mut items = list.items.lock().unwrap();
        let mut types = list.json_types.lock().unwrap();
        if matches!(kind & 255, 6 | 7 | 8 | 10 | 11) && val != 0 {
            // Lists are runtime-owned handles; retain their generated references
            // through runtime cleanup, just as other escaping references are retained.
            crate::gc::ube_gc_escape(val as *mut std::ffi::c_void);
        }
        items.push(val);
        types.push((meta, kind, name.into()));
    }
}

#[no_mangle]
pub extern "C" fn ube_list_add_typed(ptr: *mut UbeList, val: i64, meta: u64, kind: i64, name: *const std::ffi::c_char) {
    let name = if name.is_null() { "unknown".into() } else { unsafe { std::ffi::CStr::from_ptr(name) }.to_string_lossy() };
    add_typed(ptr, val, meta, kind, &name);
}

pub(crate) fn json_snapshot(ptr: *mut UbeList, limit: usize) -> Result<Vec<(i64, u64, i64, String)>, String> {
    let list = unsafe { &*ptr };
    let items = list.items.lock().unwrap();
    let types = list.json_types.lock().unwrap();
    if items.len() > limit { return Err("JSON node limit exceeded".into()); }
    Ok(items.iter().zip(types.iter()).map(|(value, (meta, kind, name))| (*value, *meta, *kind, name.clone())).collect())
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
    json_types: Mutex<HashMap<i64, ((u64, i64, String), (u64, i64, String))>>,
}

#[no_mangle]
pub extern "C" fn ube_map_create() -> *mut UbeMap {
    let map = Box::new(UbeMap {
        entries: Mutex::new(HashMap::new()),
        json_types: Mutex::new(HashMap::new()),
    });
    Box::into_raw(map)
}

#[no_mangle]
pub extern "C" fn ube_map_put(ptr: *mut UbeMap, key: i64, val: i64) {
    map_put_typed(ptr, key, (1, 0, "unknown".into()), val, (1, 0, "unknown".into()));
}

fn map_put_typed(ptr: *mut UbeMap, key: i64, key_type: (u64, i64, String), val: i64, value_type: (u64, i64, String)) {
    if let Some(map) = unsafe { ptr.as_ref() } {
        let mut entries = map.entries.lock().unwrap();
        let mut types = map.json_types.lock().unwrap();
        for (payload, kind) in [(key, key_type.1), (val, value_type.1)] {
            if matches!(kind & 255, 6 | 7 | 8 | 10 | 11) && payload != 0 {
                crate::gc::ube_gc_escape(payload as *mut std::ffi::c_void);
            }
        }
        entries.insert(key, val);
        types.insert(key, (key_type, value_type));
    }
}

#[no_mangle]
pub extern "C" fn ube_map_put_typed(
    ptr: *mut UbeMap,
    key: i64, key_meta: u64, key_kind: i64, key_name: *const std::ffi::c_char,
    val: i64, val_meta: u64, val_kind: i64, val_name: *const std::ffi::c_char,
) {
    let name = |ptr: *const std::ffi::c_char| if ptr.is_null() { "unknown".into() } else { unsafe { std::ffi::CStr::from_ptr(ptr) }.to_string_lossy().into_owned() };
    map_put_typed(ptr, key, (key_meta, key_kind, name(key_name)), val, (val_meta, val_kind, name(val_name)));
}

type JsonMapEntry = ((i64, u64, i64, String), (i64, u64, i64, String));
pub(crate) fn json_map_snapshot(ptr: *mut UbeMap, limit: usize) -> Result<Vec<JsonMapEntry>, String> {
    let map = unsafe { &*ptr };
    let entries = map.entries.lock().unwrap();
    let types = map.json_types.lock().unwrap();
    if entries.len() > limit / 2 { return Err("JSON node limit exceeded".into()); }
    entries.iter().map(|(key, val)| {
        let (kt, vt) = types.get(key).ok_or("Map entry has no JSON type metadata")?;
        Ok(((*key, kt.0, kt.1, kt.2.clone()), (*val, vt.0, vt.1, vt.2.clone())))
    }).collect()
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
    fn map_json_metadata_tracks_replacements_and_snapshot_limits() {
        let map = ube_map_create();
        assert!(json_map_snapshot(map, 0).unwrap().is_empty());
        map_put_typed(map, 7, (1, 2, "long".into()), 1, (1, 4, "bool".into()));
        map_put_typed(map, 7, (1, 2, "long".into()), 42, (1, 1, "int".into()));
        assert_eq!(ube_map_len(map), 1);
        assert_eq!(ube_map_get(map, 7), 42);
        assert!(json_map_snapshot(map, 1).is_err());
        assert_eq!(json_map_snapshot(map, 2).unwrap(), vec![((7, 1, 2, "long".into()), (42, 1, 1, "int".into()))]);
        // A legacy write replaces metadata too, so the serializer rejects it
        // instead of interpreting the payload using a stale type.
        ube_map_put(map, 7, 99);
        assert_eq!(json_map_snapshot(map, 2).unwrap()[0].1.2, 0);
        unsafe { drop(Box::from_raw(map)); }
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
