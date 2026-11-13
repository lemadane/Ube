//! Sequential, single-use collection streams. Registry ownership is moved out
//! before callbacks run; captured GC references have explicit, releasable roots.
use crate::{collections, concurrency::AbiValue, exceptions, gc};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    ffi::{c_char, c_void, CStr},
    sync::{Mutex, OnceLock},
};
type Handle = *mut c_void;
type Result<T> = std::result::Result<T, usize>;
type Invoke = extern "C" fn(Handle, i64, i64) -> i64;
const MAX_VALUES: usize = 1_000_000;
const MAX_STAGES: usize = 128;

fn error(message: &str) -> usize {
    exceptions::generic(format!("Stream: {message}"))
}
fn run<T: Default>(f: impl FnOnce() -> Result<T>) -> T {
    match f() {
        Ok(v) => v,
        Err(e) => {
            exceptions::set_pending(e);
            T::default()
        }
    }
}
fn text(pointer: *const c_char) -> String {
    if pointer.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }
}
struct Root(u64);
impl Root {
    fn new(pointer: i64) -> Self {
        Self(if pointer == 0 {
            0
        } else {
            gc::ube_gc_root_add(pointer as usize as Handle)
        })
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        if self.0 != 0 {
            gc::ube_gc_root_remove(self.0);
        }
    }
}
struct Value {
    payload: i64,
    _root: Root,
}
impl Value {
    fn checked(payload: i64, kind: i64) -> Result<Self> {
        if (kind == 0 || kind >= 6) && payload == 0 {
            return Err(error("stream elements must be non-null"));
        }
        Ok(Self::new(payload, kind))
    }
    fn new(payload: i64, kind: i64) -> Self {
        Self {
            payload,
            _root: if kind == 0 || kind >= 6 {
                Root::new(payload)
            } else {
                Root(0)
            },
        }
    }
}
struct Callback {
    context: usize,
    invoke: Invoke,
    _root: Root,
}
impl Callback {
    fn new(context: Handle, invoke: Option<Invoke>) -> Result<Self> {
        if context.is_null() {
            return Err(error("callback must be non-null"));
        }
        Ok(Self {
            context: context as usize,
            invoke: invoke.ok_or_else(|| error("missing callback"))?,
            _root: Root::new(context as i64),
        })
    }
    fn call(&self, a: i64, b: i64) -> Result<i64> {
        let value = (self.invoke)(self.context as Handle, a, b);
        let error = exceptions::take_pending();
        if error == 0 {
            Ok(value)
        } else {
            Err(error)
        }
    }
}
#[derive(Hash, Eq, PartialEq)]
enum Key {
    Bits(i64),
    Text(Vec<u8>),
}
fn key(value: i64, kind: i64) -> Key {
    if kind == 6 && value != 0 {
        Key::Text(
            unsafe { CStr::from_ptr(value as *const c_char) }
                .to_bytes()
                .to_vec(),
        )
    } else if kind == 5 {
        let n = f64::from_bits(value as u64);
        Key::Bits(if n.is_nan() {
            f64::NAN.to_bits() as i64
        } else {
            value
        })
    } else {
        Key::Bits(value)
    }
}
fn compare(a: i64, b: i64, kind: i64) -> std::cmp::Ordering {
    if kind == 6 {
        unsafe {
            CStr::from_ptr(a as *const c_char)
                .to_bytes()
                .cmp(CStr::from_ptr(b as *const c_char).to_bytes())
        }
    } else if kind == 5 {
        f64::from_bits(a as u64).total_cmp(&f64::from_bits(b as u64))
    } else {
        a.cmp(&b)
    }
}
enum Cursor {
    Values(VecDeque<Value>),
    Range {
        next: i64,
        end: i64,
    },
    Stage {
        upstream: Box<Cursor>,
        op: i32,
        callback: Option<Callback>,
        remaining: i64,
        kind: i64,
        seen: HashSet<Key>,
        retained: Vec<Root>,
        sorted: Option<VecDeque<Value>>,
    },
}
impl Cursor {
    fn next(&mut self) -> Result<Option<Value>> {
        match self {
            Self::Values(values) => Ok(values.pop_front()),
            Self::Range { next, end } => {
                if *next >= *end {
                    return Ok(None);
                }
                let value = *next;
                *next += 1;
                Ok(Some(Value::new(value, 1)))
            }
            Self::Stage {
                upstream,
                op,
                callback,
                remaining,
                kind,
                seen,
                retained,
                sorted,
            } => {
                if *op == 3 && *remaining == 0 {
                    return Ok(None);
                }
                if *op == 6 {
                    if sorted.is_none() {
                        let mut values = Vec::new();
                        while let Some(value) = upstream.next()? {
                            if values.len() == MAX_VALUES {
                                return Err(error("sorted exceeds one million buffered values"));
                            }
                            values.push(value);
                        }
                        values.sort_by(|a, b| compare(a.payload, b.payload, *kind));
                        *sorted = Some(values.into());
                    }
                    return Ok(sorted.as_mut().unwrap().pop_front());
                }
                loop {
                    let Some(value) = upstream.next()? else {
                        return Ok(None);
                    };
                    match *op {
                        0 => {
                            if callback.as_ref().unwrap().call(value.payload, 0)? == 0 {
                                continue;
                            }
                        }
                        1 => {
                            return Ok(Some(Value::checked(
                                callback.as_ref().unwrap().call(value.payload, 0)?,
                                *kind,
                            )?))
                        }
                        2 => {
                            callback.as_ref().unwrap().call(value.payload, 0)?;
                        }
                        3 => {
                            *remaining -= 1;
                        }
                        4 => {
                            if *remaining > 0 {
                                *remaining -= 1;
                                continue;
                            }
                        }
                        5 => {
                            let k = key(value.payload, *kind);
                            if seen.contains(&k) {
                                continue;
                            }
                            if seen.len() == MAX_VALUES {
                                return Err(error("distinct exceeds one million values"));
                            }
                            seen.insert(k);
                            if *kind == 0 || *kind >= 7 {
                                retained.push(Root::new(value.payload));
                            }
                        }
                        _ => return Err(error("invalid pipeline stage")),
                    }
                    return Ok(Some(value));
                }
            }
        }
    }
}
struct Pipeline {
    cursor: Cursor,
    kind: i64,
    name: String,
    depth: usize,
}
#[derive(Default)]
struct Registry {
    next: usize,
    pipelines: HashMap<usize, Pipeline>,
}
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}
fn store(pipeline: Pipeline) -> Result<Handle> {
    let mut r = registry().lock().unwrap();
    r.next = r
        .next
        .checked_add(1)
        .ok_or_else(|| error("handle limit reached"))?;
    let id = r.next;
    r.pipelines.insert(id, pipeline);
    Ok(id as Handle)
}
fn take(handle: Handle) -> Result<Pipeline> {
    registry()
        .lock()
        .unwrap()
        .pipelines
        .remove(&(handle as usize))
        .ok_or_else(|| error("stream was already linked, consumed, or closed"))
}
#[no_mangle]
pub extern "C" fn ube_sequence_source(
    list: *mut collections::UbeList,
    kind: i64,
    name: *const c_char,
) -> Handle {
    run(|| {
        if list.is_null() {
            return Err(error("source list is null"));
        }
        let name = text(name);
        let snapshot = collections::json_snapshot(list, MAX_VALUES)
            .map_err(|_| error("source exceeds one million values"))?;
        let mut values = VecDeque::new();
        for (payload, meta, actual, actual_name) in snapshot {
            if meta & 1 == 0
                || !(actual == kind && (!matches!(kind, 0 | 7) || actual_name == name)
                    || kind == 1 && actual == 3)
            {
                return Err(error(
                    "source element type mismatch; use Stream.of for typed values",
                ));
            }
            values.push_back(Value::checked(payload, kind)?);
        }
        store(Pipeline {
            cursor: Cursor::Values(values),
            kind,
            name,
            depth: 0,
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sequence_range(start: i32, end: i32) -> Handle {
    run(|| {
        store(Pipeline {
            cursor: Cursor::Range {
                next: start as i64,
                end: end as i64,
            },
            kind: 1,
            name: "int".into(),
            depth: 0,
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sequence_stage(
    handle: Handle,
    op: i32,
    context: Handle,
    invoke: Option<Invoke>,
    amount: i64,
    kind: i64,
    name: *const c_char,
) -> Handle {
    run(|| {
        if !(0..=6).contains(&op) || matches!(op, 3 | 4) && amount < 0 {
            return Err(error("limit and skip require nonnegative counts"));
        }
        let callback = if op <= 2 {
            Some(Callback::new(context, invoke)?)
        } else {
            None
        };
        let pipeline = take(handle)?;
        if pipeline.depth == MAX_STAGES {
            return Err(error("pipeline exceeds 128 stages"));
        }
        store(Pipeline {
            cursor: Cursor::Stage {
                upstream: Box::new(pipeline.cursor),
                op,
                callback,
                remaining: amount,
                kind,
                seen: HashSet::new(),
                retained: Vec::new(),
                sorted: None,
            },
            kind,
            name: text(name),
            depth: pipeline.depth + 1,
        })
    })
}
#[no_mangle]
pub extern "C" fn ube_sequence_close(handle: Handle) {
    let _ = registry()
        .lock()
        .unwrap()
        .pipelines
        .remove(&(handle as usize));
}
#[no_mangle]
pub extern "C" fn ube_sequence_get(list: *mut collections::UbeList, index: i32) -> i64 {
    run(|| {
        if list.is_null() || index < 0 || index as i64 >= collections::ube_list_len(list) {
            return Err(error("list index out of bounds"));
        }
        Ok(collections::ube_list_get(list, index as i64))
    })
}
#[no_mangle]
pub extern "C" fn ube_sequence_terminal(
    handle: Handle,
    op: i32,
    context: Handle,
    invoke: Option<Invoke>,
    identity: i64,
) -> AbiValue {
    run(|| {
        let mut pipeline = take(handle)?;
        let callback = if matches!(op, 2..=5 | 8 | 9) {
            Some(Callback::new(context, invoke)?)
        } else {
            None
        };
        let mut count = 0i64;
        let mut accumulated = if op == 8 {
            Value::checked(identity, pipeline.kind)?
        } else {
            Value::new(identity, pipeline.kind)
        };
        // Collect into a Rust buffer first: callback errors release all temporary values.
        let mut collected = Vec::new();
        while let Some(value) = pipeline.cursor.next()? {
            match op {
                0 => {
                    count = count
                        .checked_add(1)
                        .ok_or_else(|| error("count overflow"))?;
                }
                1 => {
                    if collected.len() == MAX_VALUES {
                        return Err(error("collection exceeds one million values"));
                    }
                    collected.push(value);
                }
                2 => {
                    callback.as_ref().unwrap().call(value.payload, 0)?;
                }
                3..=5 => {
                    let yes = callback.as_ref().unwrap().call(value.payload, 0)? != 0;
                    if op == 3 && yes || op == 4 && !yes || op == 5 && yes {
                        return Ok(AbiValue {
                            payload: i64::from(op == 3),
                            meta: 1,
                        });
                    }
                }
                6 | 9 => {
                    if op == 9 && callback.as_ref().unwrap().call(value.payload, 0)? == 0 {
                        continue;
                    }
                    if pipeline.kind == 0 || pipeline.kind >= 6 {
                        gc::ube_gc_frame_pin(usize::MAX, value.payload as usize as Handle);
                    }
                    return Ok(AbiValue {
                        payload: value.payload,
                        meta: 1,
                    });
                }
                7 => {
                    let payload = if pipeline.kind == 5 {
                        (f64::from_bits(accumulated.payload as u64)
                            + f64::from_bits(value.payload as u64))
                        .to_bits() as i64
                    } else {
                        accumulated
                            .payload
                            .checked_add(value.payload)
                            .ok_or_else(|| error("integer sum overflow"))?
                    };
                    accumulated = Value::new(payload, pipeline.kind);
                }
                8 => {
                    accumulated = Value::checked(
                        callback
                            .as_ref()
                            .unwrap()
                            .call(accumulated.payload, value.payload)?,
                        pipeline.kind,
                    )?;
                }
                _ => return Err(error("invalid terminal operation")),
            }
        }
        let payload = match op {
            0 => count,
            1 => {
                let list = collections::ube_list_create();
                for value in collected {
                    collections::add_typed(list, value.payload, 1, pipeline.kind, &pipeline.name);
                }
                list as usize as i64
            }
            4 | 5 => 1,
            7 | 8 => {
                if pipeline.kind == 0 || pipeline.kind >= 6 {
                    gc::ube_gc_frame_pin(usize::MAX, accumulated.payload as usize as Handle);
                }
                accumulated.payload
            }
            _ => 0,
        };
        Ok(AbiValue {
            payload,
            meta: u64::from(!matches!(op, 6 | 9)),
        })
    })
}
pub(crate) fn cleanup() {
    registry().lock().unwrap().pipelines.clear();
}

// Factory temporaries borrow already-pinned compiler values, then transfer them
// into rooted stream storage and release the temporary list itself.
#[no_mangle]
pub extern "C" fn ube_sequence_values_add(
    list: *mut collections::UbeList,
    payload: i64,
    kind: i64,
    name: *const c_char,
) {
    collections::add_with_retention(list, payload, 1, kind, &text(name), false);
}
#[no_mangle]
pub extern "C" fn ube_sequence_owned_source(
    list: *mut collections::UbeList,
    kind: i64,
    name: *const c_char,
) -> Handle {
    let result = ube_sequence_source(list, kind, name);
    if !list.is_null() {
        unsafe {
            drop(Box::from_raw(list));
        }
    }
    result
}

#[no_mangle]
pub extern "C" fn ube_sequence_list_add(
    list: *mut collections::UbeList,
    payload: i64,
    kind: i64,
    name: *const c_char,
) {
    collections::add_typed(list, payload, 1, kind, &text(name));
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn echo(context: Handle, value: i64, _: i64) -> i64 {
        assert_eq!(unsafe { *context.cast::<i64>() }, 42);
        value
    }
    extern "C" fn fail(context: Handle, _: i64, _: i64) -> i64 {
        assert_eq!(unsafe { *context.cast::<i64>() }, 42);
        exceptions::set_pending(error("callback failed"));
        0
    }
    extern "C" fn null_object(_: Handle, _: i64, _: i64) -> i64 {
        0
    }
    #[test]
    fn capture_roots_release_after_consumption_error_and_close() {
        for mode in 0..4 {
            let allocation = gc::ube_gc_allocate(8, 8, std::ptr::null(), 0);
            unsafe {
                *allocation.object.cast::<i64>() = 42;
            }
            let source = ube_sequence_range(0, 10);
            let stage = ube_sequence_stage(
                source,
                1,
                allocation.object,
                Some(if mode == 1 {
                    fail
                } else if mode == 3 {
                    null_object
                } else {
                    echo
                }),
                0,
                if mode == 3 { 7 } else { 1 },
                c"int".as_ptr(),
            );
            gc::ube_gc_root_remove(allocation.root);
            gc::ube_gc_collect();
            assert!(gc::is_managed(allocation.object));
            if mode == 2 {
                ube_sequence_close(stage);
            } else {
                let result = ube_sequence_terminal(stage, 0, std::ptr::null_mut(), None, 0);
                let pending = exceptions::take_pending();
                if mode == 1 || mode == 3 {
                    assert_ne!(pending, 0);
                } else {
                    assert_eq!(pending, 0);
                    assert_eq!(result.payload, 10);
                }
            }
            gc::ube_gc_collect();
            assert!(!gc::is_managed(allocation.object));
        }
    }
    #[test]
    fn pipeline_depth_is_bounded_and_old_handles_cannot_be_reused() {
        let source = ube_sequence_range(0, 10);
        let mut current = source;
        for _ in 0..MAX_STAGES {
            current = ube_sequence_stage(
                current,
                3,
                std::ptr::null_mut(),
                None,
                10,
                1,
                c"int".as_ptr(),
            );
            assert!(!current.is_null());
        }
        let rejected = ube_sequence_stage(
            current,
            3,
            std::ptr::null_mut(),
            None,
            10,
            1,
            c"int".as_ptr(),
        );
        assert!(rejected.is_null());
        assert_ne!(exceptions::take_pending(), 0);
        assert!(take(source).is_err());
        assert!(take(current).is_err());
    }
}

#[no_mangle]
pub extern "C" fn ube_sequence_invalid_callback() -> i64 {
    exceptions::set_pending(error("callback does not implement its declared signature"));
    0
}
