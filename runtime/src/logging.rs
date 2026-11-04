//! Severity-filtered diagnostics on stderr and unfiltered Console output on stdout.
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::{c_char, CStr},
    io::{self, BufRead, Write},
    sync::atomic::{AtomicU8, Ordering},
};

// DEBUG < INFO < WARN < ERROR < OFF. The default matches common Java logging setups.
static LEVEL: AtomicU8 = AtomicU8::new(1);

fn parse_level(level: &str) -> Option<u8> {
    match level.to_ascii_lowercase().as_str() {
        "debug" => Some(0),
        "info" => Some(1),
        "warn" => Some(2),
        "error" => Some(3),
        "off" => Some(4),
        _ => None,
    }
}
#[no_mangle]
pub extern "C" fn ube_log_set_level(level: *const c_char) {
    let level = if level.is_null() {
        None
    } else {
        unsafe { CStr::from_ptr(level) }
            .to_str()
            .ok()
            .and_then(parse_level)
    };
    match level {
        Some(level) => LEVEL.store(level, Ordering::Relaxed),
        None => crate::exceptions::set_pending(crate::exceptions::generic(
            "Log.setLevel expects debug, info, warn, error, or off".into(),
        )),
    }
}
fn output(mode: i32, message: impl std::fmt::Display) {
    if mode == 4 || mode == 5 {
        let mut out = io::stdout().lock();
        // I/O failures never panic across the generated-code ABI.
        let result = if mode == 4 {
            write!(out, "{message}")
        } else {
            writeln!(out, "{message}")
        };
        if let Err(error) = result.and_then(|_| out.flush()) {
            crate::exceptions::set_pending(crate::exceptions::generic(format!(
                "Console output failed: {error}"
            )));
        }
        return;
    }
    if !(0..=3).contains(&mode) || (mode as u8) < LEVEL.load(Ordering::Relaxed) {
        return;
    }
    let label = ["DEBUG", "INFO", "WARN", "ERROR"][mode as usize];
    let context = context_prefix();
    let mut out = io::stderr().lock();
    if let Err(error) = writeln!(out, "[{label}] {context}{message}") {
        crate::exceptions::set_pending(crate::exceptions::generic(format!(
            "Log output failed: {error}"
        )));
    }
}
#[no_mangle]
pub extern "C" fn ube_output_i64(mode: i32, value: i64) {
    output(mode, value);
}
#[no_mangle]
pub extern "C" fn ube_output_f64(mode: i32, value: f64) {
    output(mode, value);
}
#[no_mangle]
pub extern "C" fn ube_output_bool(mode: i32, value: bool) {
    output(mode, value);
}
#[no_mangle]
pub extern "C" fn ube_output_str(mode: i32, value: *const c_char) {
    if value.is_null() {
        output(mode, "null");
        return;
    }
    match unsafe { CStr::from_ptr(value) }.to_str() {
        Ok(value) => output(mode, value),
        Err(_) => crate::exceptions::set_pending(crate::exceptions::generic(
            "Invalid UTF-8 output value".into(),
        )),
    }
}

/// Convert an already-evaluated scalar to a tracked string for interpolation.
#[no_mangle]
pub extern "C" fn ube_format_value(kind: i32, payload: i64, present: i64) -> *mut c_char {
    let text = if present == 0 {
        "null".into()
    } else {
        match kind {
            0 => payload.to_string(),
            1 => f64::from_bits(payload as u64).to_string(),
            2 => (payload != 0).to_string(),
            _ if payload == 0 => "null".into(),
            _ => unsafe { CStr::from_ptr(payload as *const c_char) }
                .to_string_lossy()
                .into_owned(),
        }
    };
    let ptr = crate::memory::ube_alloc(text.len() + 1, 1) as *mut c_char;
    if !ptr.is_null() {
        unsafe {
            std::ptr::copy_nonoverlapping(text.as_ptr(), ptr as *mut u8, text.len());
            *ptr.add(text.len()) = 0;
        }
    }
    ptr
}

pub(crate) type LogContext = BTreeMap<String, String>;
thread_local! {
    static CONTEXT: RefCell<LogContext> = const { RefCell::new(BTreeMap::new()) };
}

pub(crate) fn snapshot() -> LogContext {
    CONTEXT.with(|context| context.borrow().clone())
}

/// Install an isolated snapshot for one native Ube task, restoring the worker's
/// previous context on completion, error, cancellation, or Rust unwinding.
pub(crate) fn with_context<T>(context: LogContext, work: impl FnOnce() -> T) -> T {
    struct Restore(LogContext);
    impl Drop for Restore {
        fn drop(&mut self) {
            CONTEXT.with(|context| *context.borrow_mut() = std::mem::take(&mut self.0));
        }
    }
    let previous = CONTEXT.with(|current| std::mem::replace(&mut *current.borrow_mut(), context));
    let _restore = Restore(previous);
    work()
}

fn context_arg(ptr: *const c_char) -> Result<String, String> {
    if ptr.is_null() {
        return Err("Log context keys and values must not be null".into());
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map(str::to_owned)
        .map_err(|error| format!("Invalid UTF-8 Log context: {error}"))
}
fn context_failure(error: String) {
    crate::exceptions::set_pending(crate::exceptions::generic(error));
}
#[no_mangle]
pub extern "C" fn ube_log_put(key: *const c_char, value: *const c_char) {
    match context_arg(key).and_then(|key| Ok((key, context_arg(value)?))) {
        Ok((key, value)) => CONTEXT.with(|context| {
            context.borrow_mut().insert(key, value);
        }),
        Err(error) => context_failure(error),
    }
}
#[no_mangle]
pub extern "C" fn ube_log_get(key: *const c_char) -> *mut c_char {
    match context_arg(key) {
        Ok(key) => {
            let value = CONTEXT.with(|context| context.borrow().get(&key).cloned());
            // Return a stable copy: subsequent put/remove/clear operations cannot
            // invalidate a value already returned to Ube code.
            match value {
                Some(value) => {
                    let ptr = crate::memory::ube_alloc(value.len() + 1, 1) as *mut c_char;
                    if !ptr.is_null() {
                        unsafe {
                            std::ptr::copy_nonoverlapping(
                                value.as_ptr(),
                                ptr as *mut u8,
                                value.len(),
                            );
                            *ptr.add(value.len()) = 0;
                        }
                    }
                    ptr
                }
                None => std::ptr::null_mut(),
            }
        }
        Err(error) => {
            context_failure(error);
            std::ptr::null_mut()
        }
    }
}
#[no_mangle]
pub extern "C" fn ube_log_remove(key: *const c_char) {
    match context_arg(key) {
        Ok(key) => CONTEXT.with(|context| {
            context.borrow_mut().remove(&key);
        }),
        Err(error) => context_failure(error),
    }
}
#[no_mangle]
pub extern "C" fn ube_log_clear() {
    CONTEXT.with(|context| context.borrow_mut().clear());
}

fn json_string(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            ch if ch <= '\u{1f}' => {
                let _ = write!(encoded, "\\u{:04x}", ch as u32);
            }
            ch => encoded.push(ch),
        }
    }
    encoded.push('"');
    encoded
}
fn context_prefix() -> String {
    CONTEXT.with(|context| {
        let context = context.borrow();
        if context.is_empty() {
            return String::new();
        }
        let pairs = context
            .iter()
            .map(|(key, value)| format!("{}:{}", json_string(key), json_string(value)))
            .collect::<Vec<_>>();
        format!("{{{}}} ", pairs.join(","))
    })
}

#[cfg(test)]
mod context_tests {
    use super::*;
    use std::ffi::CString;
    fn text(value: &str) -> CString {
        CString::new(value).unwrap()
    }

    #[test]
    fn context_scopes_restore_worker_state_after_success_and_panic() {
        with_context(LogContext::new(), || {
            ube_log_put(text("worker").as_ptr(), text("before").as_ptr());
            let mut inherited = snapshot();
            inherited.insert("request".into(), "child".into());
            let result = std::panic::catch_unwind(|| {
                with_context(inherited, || {
                    ube_log_put(text("worker").as_ptr(), text("changed").as_ptr());
                    panic!("task panic");
                })
            });
            assert!(result.is_err());
            assert_eq!(
                snapshot(),
                BTreeMap::from([("worker".into(), "before".into())])
            );
            with_context(LogContext::new(), || assert!(snapshot().is_empty()));
            assert_eq!(snapshot().len(), 1);
        });
    }

    #[test]
    fn context_format_escapes_keys_values_and_validates_ffi_arguments() {
        with_context(LogContext::new(), || {
            ube_log_put(text("request\n\"").as_ptr(), text("🍠\t\\\r\u{1}").as_ptr());
            assert_eq!(
                context_prefix(),
                "{\"request\\n\\\"\":\"🍠\\t\\\\\\r\\u0001\"} "
            );
            let previous = snapshot();
            ube_log_put(text("invalid").as_ptr(), std::ptr::null());
            assert_ne!(crate::exceptions::take_pending(), 0);
            assert_eq!(snapshot(), previous);
            let invalid_utf8 = [0xffu8, 0];
            assert!(ube_log_get(invalid_utf8.as_ptr() as *const c_char).is_null());
            assert_ne!(crate::exceptions::take_pending(), 0);
            assert_eq!(crate::exceptions::take_pending(), 0);
        });
    }
}

// Both operations use the same stdin buffer so mixed character/line reads
// preserve input already buffered by a previous call.
fn read_console(input: &mut impl BufRead, line: bool) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    if line {
        if input.read_until(b'\n', &mut bytes)? == 0 { return Ok(None); }
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
            if bytes.last() == Some(&b'\r') { bytes.pop(); }
        }
    } else {
        let mut first = [0u8];
        if input.read(&mut first)? == 0 { return Ok(None); }
        let size = match first[0] {
            0..=0x7f => 1, 0xc2..=0xdf => 2, 0xe0..=0xef => 3, 0xf0..=0xf4 => 4,
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid UTF-8 input")),
        };
        bytes.resize(size, 0);
        bytes[0] = first[0];
        input.read_exact(&mut bytes[1..])?;
    }
    if bytes.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Console input contains a null byte"));
    }
    String::from_utf8(bytes).map(Some).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}
fn console_input(line: bool) -> *mut c_char {
    match read_console(&mut io::stdin().lock(), line) {
        Ok(Some(text)) => {
            let ptr = crate::memory::ube_alloc(text.len() + 1, 1) as *mut c_char;
            if !ptr.is_null() {
                unsafe {
                    std::ptr::copy_nonoverlapping(text.as_ptr(), ptr as *mut u8, text.len());
                    *ptr.add(text.len()) = 0;
                }
            }
            ptr
        }
        Ok(None) => std::ptr::null_mut(),
        Err(error) => {
            crate::exceptions::set_pending(crate::exceptions::generic(format!("Console input failed: {error}")));
            std::ptr::null_mut()
        }
    }
}
#[no_mangle]
pub extern "C" fn ube_console_read() -> *mut c_char { console_input(false) }
#[no_mangle]
pub extern "C" fn ube_console_read_line() -> *mut c_char { console_input(true) }

#[cfg(test)]
mod console_input_tests {
    use super::*;
    #[test]
    fn mixed_reads_preserve_unicode_lines_and_eof() {
        let mut input = io::Cursor::new("🍠hello\r\n\nlast".as_bytes());
        assert_eq!(read_console(&mut input, false).unwrap().as_deref(), Some("🍠"));
        for expected in [Some("hello"), Some(""), Some("last"), None] {
            assert_eq!(read_console(&mut input, true).unwrap().as_deref(), expected);
        }
        assert_eq!(read_console(&mut input, false).unwrap(), None);
    }
    #[test]
    fn input_errors_are_reported() {
        for bytes in [vec![0xff], vec![0xf0, 0x9f], vec![0]] {
            assert!(read_console(&mut io::Cursor::new(bytes), false).is_err());
        }
        assert!(read_console(&mut io::Cursor::new(vec![0xff, b'\n']), true).is_err());
        assert!(read_console(&mut io::Cursor::new(vec![0, b'\n']), true).is_err());
    }
}
