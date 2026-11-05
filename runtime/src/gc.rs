//! Precise tracing heap with explicit roots and pointer-field descriptors.
//! Generated objects publish roots and use serialized pointer-field barriers.
//! Collection never guesses roots from arbitrary stack/heap bytes.
use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    sync::{Mutex, OnceLock},
};

struct Object {
    bytes: usize,
    edges: Vec<usize>,
}
#[derive(Default)]
struct Heap {
    objects: HashMap<usize, Object>,
    roots: HashMap<u64, usize>,
    next_root: u64,
    collections: usize,
    reclaimed: usize,
    frames: HashMap<std::thread::ThreadId, Vec<HashMap<usize, u64>>>,
    external: HashMap<usize, u64>,
    since_collection: usize,
}
fn heap() -> &'static Mutex<Heap> {
    static HEAP: OnceLock<Mutex<Heap>> = OnceLock::new();
    HEAP.get_or_init(|| Mutex::new(Heap::default()))
}

#[repr(C)]
pub struct GcAllocation {
    pub object: *mut c_void,
    pub root: u64,
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GcStats {
    pub objects: usize,
    pub bytes: usize,
    pub roots: usize,
    pub collections: usize,
    pub reclaimed: usize,
}

fn add_root(heap: &mut Heap, pointer: usize) -> u64 {
    let Some(root) = heap.next_root.checked_add(1) else {
        return 0;
    };
    heap.next_root = root;
    heap.roots.insert(root, pointer);
    root
}

#[no_mangle]
pub extern "C" fn ube_gc_init() {
    let _ = heap();
}

/// Allocation and its initial root are published atomically. Callers must keep
/// a root alive while accessing the object. Offsets describe pointer fields only.
#[no_mangle]
pub extern "C" fn ube_gc_allocate(
    bytes: usize,
    align: usize,
    offsets: *const usize,
    count: usize,
) -> GcAllocation {
    let failed = || GcAllocation {
        object: std::ptr::null_mut(),
        root: 0,
    };
    if count > 4096 || (count > 0 && offsets.is_null()) {
        return failed();
    }
    let edges = if count == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(offsets, count) }.to_vec()
    };
    if edges.iter().any(|offset| {
        offset
            .checked_add(std::mem::size_of::<usize>())
            .is_none_or(|end| end > bytes)
    }) {
        return failed();
    }
    let mut state = heap().lock().unwrap();
    if state.since_collection >= 256 && state.frames.contains_key(&std::thread::current().id()) {
        collect(&mut state);
    }
    if state.next_root == u64::MAX {
        return failed();
    }
    let pointer = crate::memory::ube_alloc(bytes, align);
    if pointer.is_null() {
        return failed();
    }
    // Pointer fields and padding are initialized before any tracing is possible.
    unsafe {
        std::ptr::write_bytes(pointer.cast::<u8>(), 0, bytes);
    }
    state
        .objects
        .insert(pointer as usize, Object { bytes, edges });
    state.since_collection += 1;
    let root = add_root(&mut state, pointer as usize);
    GcAllocation {
        object: pointer,
        root,
    }
}

#[no_mangle]
pub extern "C" fn ube_gc_root_add(pointer: *mut c_void) -> u64 {
    let mut state = heap().lock().unwrap();
    if !pointer.is_null() && !state.objects.contains_key(&(pointer as usize)) {
        return 0;
    }
    add_root(&mut state, pointer as usize)
}
#[no_mangle]
pub extern "C" fn ube_gc_root_update(root: u64, pointer: *mut c_void) -> bool {
    let mut state = heap().lock().unwrap();
    if !pointer.is_null() && !state.objects.contains_key(&(pointer as usize)) {
        return false;
    }
    let Some(slot) = state.roots.get_mut(&root) else {
        return false;
    };
    *slot = pointer as usize;
    true
}
#[no_mangle]
pub extern "C" fn ube_gc_root_remove(root: u64) -> bool {
    heap().lock().unwrap().roots.remove(&root).is_some()
}

/// All managed pointer-field writes use this lock, including concurrent callers.
/// Scalar fields can be written directly while the caller retains a root.
#[no_mangle]
pub extern "C" fn ube_gc_store_pointer(
    object: *mut c_void,
    offset: usize,
    target: *mut c_void,
) -> bool {
    let state = heap().lock().unwrap();
    let Some(entry) = state.objects.get(&(object as usize)) else {
        return false;
    };
    if !entry.edges.contains(&offset)
        || (!target.is_null() && !state.objects.contains_key(&(target as usize)))
    {
        return false;
    }
    unsafe {
        object
            .cast::<u8>()
            .add(offset)
            .cast::<usize>()
            .write_unaligned(target as usize);
    }
    true
}

#[no_mangle]
pub extern "C" fn ube_gc_collect() {
    collect(&mut heap().lock().unwrap());
}

fn collect(state: &mut Heap) -> bool {
    // Roots and pointer fields share this mutex. Generated pointer-field loads
    // publish their result root before releasing it, so no safepoint guesswork
    // or concurrent raw pointer scan is involved.
    let mut marked = HashSet::new();
    let mut pending = state.roots.values().copied().collect::<Vec<_>>();
    while let Some(pointer) = pending.pop() {
        let Some(object) = state.objects.get(&pointer) else {
            continue;
        };
        if !marked.insert(pointer) {
            continue;
        }
        for offset in &object.edges {
            let child = unsafe {
                (pointer as *const u8)
                    .add(*offset)
                    .cast::<usize>()
                    .read_unaligned()
            };
            if child != 0 {
                pending.push(child);
            }
        }
    }
    let dead = state
        .objects
        .keys()
        .filter(|pointer| !marked.contains(pointer))
        .copied()
        .collect::<Vec<_>>();
    for pointer in dead {
        state.objects.remove(&pointer).unwrap();
        crate::exceptions::forget_object(pointer);
        crate::memory::free_registered(pointer as *mut c_void);
        state.reclaimed += 1;
    }
    state.collections += 1;
    state.since_collection = 0;
    true
}

#[no_mangle]
pub extern "C" fn ube_gc_stats() -> GcStats {
    let state = heap().lock().unwrap();
    GcStats {
        objects: state.objects.len(),
        bytes: state.objects.values().map(|object| object.bytes).sum(),
        roots: state.roots.len(),
        collections: state.collections,
        reclaimed: state.reclaimed,
    }
}

pub(crate) fn cleanup() {
    let mut state = heap().lock().unwrap();
    for (pointer, _) in state.objects.drain() {
        crate::memory::free_registered(pointer as *mut c_void);
    }
    state.roots.clear();
    state.frames.clear();
    state.external.clear();
}

pub(crate) fn is_managed(pointer: *mut c_void) -> bool {
    heap()
        .lock()
        .unwrap()
        .objects
        .contains_key(&(pointer as usize))
}

#[no_mangle]
pub extern "C" fn ube_gc_frame_enter() {
    heap()
        .lock()
        .unwrap()
        .frames
        .entry(std::thread::current().id())
        .or_default()
        .push(HashMap::new());
}

#[no_mangle]
pub extern "C" fn ube_gc_frame_pin(key: usize, pointer: *mut c_void) {
    frame_pin(&mut heap().lock().unwrap(), key, pointer);
}

fn frame_pin(state: &mut Heap, key: usize, pointer: *mut c_void) {
    let thread = std::thread::current().id();
    let previous = state
        .frames
        .get(&thread)
        .and_then(|frames| frames.last())
        .and_then(|frame| frame.get(&key))
        .copied();
    let managed = state.objects.contains_key(&(pointer as usize));
    if let Some(root) = previous {
        if managed {
            state.roots.insert(root, pointer as usize);
        } else {
            state.roots.remove(&root);
            state
                .frames
                .get_mut(&thread)
                .unwrap()
                .last_mut()
                .unwrap()
                .remove(&key);
        }
    } else if managed
        && state
            .frames
            .get(&thread)
            .is_some_and(|frames| !frames.is_empty())
    {
        let root = add_root(state, pointer as usize);
        assert_ne!(root, 0, "GC root identifier space exhausted");
        state
            .frames
            .get_mut(&thread)
            .unwrap()
            .last_mut()
            .unwrap()
            .insert(key, root);
    }
}

fn escape(state: &mut Heap, pointer: usize) {
    if state.objects.contains_key(&pointer) && !state.external.contains_key(&pointer) {
        let root = add_root(state, pointer);
        assert_ne!(root, 0, "GC root identifier space exhausted");
        state.external.insert(pointer, root);
    }
}

#[no_mangle]
pub extern "C" fn ube_gc_escape(pointer: *mut c_void) {
    escape(&mut heap().lock().unwrap(), pointer as usize);
}

#[no_mangle]
pub extern "C" fn ube_gc_frame_leave(returned: *mut c_void) {
    let mut state = heap().lock().unwrap();
    let thread = std::thread::current().id();
    let Some(frame) = state.frames.get_mut(&thread).and_then(Vec::pop) else {
        return;
    };
    if state.objects.contains_key(&(returned as usize)) {
        if state
            .frames
            .get(&thread)
            .is_some_and(|frames| !frames.is_empty())
        {
            let root = add_root(&mut state, returned as usize);
            assert_ne!(root, 0, "GC root identifier space exhausted");
            if let Some(previous) = state
                .frames
                .get_mut(&thread)
                .unwrap()
                .last_mut()
                .unwrap()
                .insert(usize::MAX, root)
            {
                state.roots.remove(&previous);
            }
        } else {
            escape(&mut state, returned as usize);
        }
    }
    for root in frame.values() {
        state.roots.remove(root);
    }
    if state.frames.get(&thread).is_some_and(Vec::is_empty) {
        state.frames.remove(&thread);
    }
}

// Compiler barrier: non-managed references retain their legacy lifetime and are
// not traced. The destination and field descriptor still must be managed.
#[no_mangle]
pub extern "C" fn ube_gc_store_reference(object: *mut c_void, offset: usize, target: *mut c_void) {
    let mut state = heap().lock().unwrap();
    if let Some(entry) = state.objects.get(&(object as usize)) {
        assert!(
            entry.edges.contains(&offset),
            "compiler GC store requires pointer field descriptor"
        );
    } else {
        assert!(
            offset
                .checked_add(std::mem::size_of::<usize>())
                .is_some_and(|bytes| crate::memory::registered_range(object, bytes)),
            "legacy reference store requires an owned allocation"
        );
        escape(&mut state, target as usize);
    }
    unsafe {
        object
            .cast::<u8>()
            .add(offset)
            .cast::<usize>()
            .write_unaligned(target as usize);
    }
}

#[no_mangle]
pub extern "C" fn ube_gc_try_collect() -> bool {
    collect(&mut heap().lock().unwrap())
}
#[no_mangle]
pub extern "C" fn ube_gc_object_count() -> i64 {
    ube_gc_stats().objects as i64
}
#[no_mangle]
pub extern "C" fn ube_gc_reclaimed_count() -> i64 {
    ube_gc_stats().reclaimed as i64
}

#[no_mangle]
pub extern "C" fn ube_gc_load_reference(
    object: *mut c_void,
    offset: usize,
    key: usize,
) -> *mut c_void {
    let mut state = heap().lock().unwrap();
    if let Some(entry) = state.objects.get(&(object as usize)) {
        assert!(
            entry.edges.contains(&offset),
            "compiler GC load requires pointer field descriptor"
        );
    } else {
        // Runtime-created error objects retain the one-pointer legacy layout.
        assert!(
            offset
                .checked_add(std::mem::size_of::<usize>())
                .is_some_and(|bytes| crate::memory::registered_range(object, bytes)),
            "legacy reference load requires an owned allocation"
        );
    }
    let value = unsafe {
        object
            .cast::<u8>()
            .add(offset)
            .cast::<usize>()
            .read_unaligned()
    } as *mut c_void;
    frame_pin(&mut state, key, value);
    value
}
