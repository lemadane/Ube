//! Strict, synchronized stub registry. Handles are monotonic and never reused.
use std::{
    collections::HashMap,
    ffi::{c_char, CStr, CString},
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone)]
enum Value {
    Void,
    Int(i32),
    Long(i64),
    Byte(i8),
    Bool(bool),
    Decimal(f64),
    String(Arc<CString>),
    Throw(String),
}
#[derive(Default)]
struct Mock {
    stubs: HashMap<String, Value>,
    calls: HashMap<String, i64>,
}
#[derive(Default)]
struct Registry {
    next: i64,
    mocks: HashMap<i64, Mock>,
    strings: Vec<Arc<CString>>,
}
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}
fn text(value: *const c_char) -> String {
    if value.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned()
    }
}
fn error(kind: &str, message: String) {
    crate::exceptions::set_pending(crate::exceptions::named(kind, message));
}
fn missing() {
    error("MockeryError", "Mock handle is disposed or invalid".into());
}
fn stub(id: i64, method: *const c_char, value: Value) {
    let mut registry = registry().lock().unwrap();
    if let Some(mock) = registry.mocks.get_mut(&id) {
        mock.stubs.insert(text(method), value);
    } else {
        missing();
    }
}
fn invoke(id: i64, method: *const c_char) -> Option<Value> {
    let method = text(method);
    let mut registry = registry().lock().unwrap();
    let Some(mock) = registry.mocks.get_mut(&id) else {
        missing();
        return None;
    };
    let calls = mock.calls.entry(method.clone()).or_default();
    *calls = calls.saturating_add(1);
    match mock.stubs.get(&method).cloned() {
        Some(Value::Throw(message)) => {
            error("MockeryError", message);
            None
        }
        None => {
            error("MockeryError", format!("Unconfigured mock call: {method}"));
            None
        }
        value => value,
    }
}
fn wrong_type() {
    error(
        "MockeryError",
        "Configured mock return type does not match the invoked method".into(),
    );
}

#[no_mangle]
pub extern "C" fn ube_mockery_create() -> i64 {
    let mut registry = registry().lock().unwrap();
    let Some(next) = registry.next.checked_add(1) else {
        error("MockeryError", "Mock handle limit reached".into());
        return 0;
    };
    registry.next = next;
    registry.mocks.insert(next, Mock::default());
    next
}

macro_rules! scalar {
    ($set:ident, $call:ident, $variant:ident, $ty:ty, $zero:expr) => {
        #[no_mangle]
        pub extern "C" fn $set(id: i64, method: *const c_char, value: $ty) {
            stub(id, method, Value::$variant(value));
        }
        #[no_mangle]
        pub extern "C" fn $call(id: i64, method: *const c_char) -> $ty {
            match invoke(id, method) {
                Some(Value::$variant(value)) => value,
                Some(_) => {
                    wrong_type();
                    $zero
                }
                None => $zero,
            }
        }
    };
}
scalar!(ube_mockery_return_int, ube_mockery_call_int, Int, i32, 0);
scalar!(ube_mockery_return_long, ube_mockery_call_long, Long, i64, 0);
scalar!(ube_mockery_return_byte, ube_mockery_call_byte, Byte, i8, 0);
scalar!(
    ube_mockery_return_bool,
    ube_mockery_call_bool,
    Bool,
    bool,
    false
);
scalar!(
    ube_mockery_return_decimal,
    ube_mockery_call_decimal,
    Decimal,
    f64,
    0.0
);

#[no_mangle]
pub extern "C" fn ube_mockery_return_string(id: i64, method: *const c_char, value: *const c_char) {
    let value = Arc::new(CString::new(text(value)).unwrap());
    let mut registry = registry().lock().unwrap();
    if let Some(mock) = registry.mocks.get_mut(&id) {
        mock.stubs
            .insert(text(method), Value::String(Arc::clone(&value)));
        // Keep previously returned pointers valid through reconfiguration/disposal.
        registry.strings.push(value);
    } else {
        missing();
    }
}
#[no_mangle]
pub extern "C" fn ube_mockery_call_string(id: i64, method: *const c_char) -> *const c_char {
    match invoke(id, method) {
        Some(Value::String(value)) => value.as_ptr(),
        Some(_) => {
            wrong_type();
            std::ptr::null()
        }
        None => std::ptr::null(),
    }
}
#[no_mangle]
pub extern "C" fn ube_mockery_call_void(id: i64, method: *const c_char) {
    if let Some(value) = invoke(id, method) {
        if !matches!(value, Value::Void) {
            wrong_type();
        }
    }
}
#[no_mangle]
pub extern "C" fn ube_mockery_allow(id: i64, method: *const c_char) {
    stub(id, method, Value::Void);
}
#[no_mangle]
pub extern "C" fn ube_mockery_throws(id: i64, method: *const c_char, message: *const c_char) {
    stub(id, method, Value::Throw(text(message)));
}
#[no_mangle]
pub extern "C" fn ube_mockery_calls(id: i64, method: *const c_char) -> i64 {
    let registry = registry().lock().unwrap();
    match registry.mocks.get(&id) {
        Some(mock) => mock.calls.get(&text(method)).copied().unwrap_or(0),
        None => {
            missing();
            0
        }
    }
}
#[no_mangle]
pub extern "C" fn ube_mockery_verify(id: i64, method: *const c_char, expected: i32) {
    if expected < 0 {
        error(
            "MockeryError",
            "Expected call count must be non-negative".into(),
        );
        return;
    }
    let name = text(method);
    let registry = registry().lock().unwrap();
    let Some(mock) = registry.mocks.get(&id) else {
        missing();
        return;
    };
    let actual = mock.calls.get(&name).copied().unwrap_or(0);
    if actual != i64::from(expected) {
        error(
            "AssertionError",
            format!(
                "Mock verification failed for {name}: expected {expected} calls, received {actual}"
            ),
        );
    }
}
#[no_mangle]
pub extern "C" fn ube_mockery_verify_no_calls(id: i64) {
    let registry = registry().lock().unwrap();
    let Some(mock) = registry.mocks.get(&id) else {
        missing();
        return;
    };
    if mock.calls.values().any(|count| *count != 0) {
        error("AssertionError", "Expected no mock calls".into());
    }
}
#[no_mangle]
pub extern "C" fn ube_mockery_clearCalls(id: i64) {
    let mut registry = registry().lock().unwrap();
    if let Some(mock) = registry.mocks.get_mut(&id) {
        mock.calls.clear();
    } else {
        missing();
    }
}
#[no_mangle]
pub extern "C" fn ube_mockery_reset(id: i64) {
    let mut registry = registry().lock().unwrap();
    if let Some(mock) = registry.mocks.get_mut(&id) {
        *mock = Mock::default();
    } else {
        missing();
    }
}
#[no_mangle]
pub extern "C" fn ube_mockery_dispose(id: i64) {
    if registry().lock().unwrap().mocks.remove(&id).is_none() {
        missing();
    }
}
pub(crate) fn cleanup() {
    let mut registry = registry().lock().unwrap();
    registry.mocks.clear();
    registry.strings.clear();
}
