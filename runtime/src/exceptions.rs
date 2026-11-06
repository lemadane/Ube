//! Compiler-managed exceptions. No Rust or LLVM unwinding crosses generated code.
//! Records and object identity metadata live until runtime shutdown, like task handles.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::{Mutex, OnceLock};

#[derive(Debug)]
pub struct ExceptionRecord {
    pub object: usize,
    pub type_name: String,
    pub message: CString,
    pub location: String,
    pub stacktrace: CString,
    async_links: usize,
    pub suppressed: Mutex<Vec<usize>>,
}

fn records() -> &'static Mutex<Vec<Box<ExceptionRecord>>> {
    static RECORDS: OnceLock<Mutex<Vec<Box<ExceptionRecord>>>> = OnceLock::new();
    RECORDS.get_or_init(|| Mutex::new(Vec::new()))
}
fn objects() -> &'static Mutex<HashMap<usize, String>> {
    static OBJECTS: OnceLock<Mutex<HashMap<usize, String>>> = OnceLock::new();
    OBJECTS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn types() -> &'static Mutex<HashMap<String, HashMap<String, usize>>> {
    static TYPES: OnceLock<Mutex<HashMap<String, HashMap<String, usize>>>> = OnceLock::new();
    TYPES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[no_mangle]
pub extern "C" fn ube_type_method(name: *const c_char, signature: *const c_char, method: *mut c_void) {
    types().lock().unwrap().entry(string(name)).or_default().insert(string(signature), method as usize);
}
#[no_mangle]
pub extern "C" fn ube_object_satisfies(object: *mut c_void, contract: *const c_char) -> bool {
    let objects = objects().lock().unwrap();
    let Some(name) = objects.get(&(object as usize)) else { return false; };
    let types = types().lock().unwrap();
    types.get(name).is_some_and(|methods| string(contract).lines().all(|required| methods.contains_key(required)))
}
#[no_mangle]
pub extern "C" fn ube_object_method(object: *mut c_void, signature: *const c_char) -> *mut c_void {
    let objects = objects().lock().unwrap();
    let method = objects.get(&(object as usize)).and_then(|name| types().lock().unwrap().get(name).and_then(|methods| methods.get(&string(signature)).copied()));
    method.unwrap_or(0) as *mut c_void
}

type TraceFrame = (String, String);
#[derive(Default)]
pub(crate) struct TraceContext {
    frames: Vec<TraceFrame>,
    submissions: Vec<Vec<TraceFrame>>,
    observed: HashMap<usize, usize>,
    resumed: Option<TraceFrame>,
}
thread_local! {
    static TRACE: RefCell<TraceContext> = RefCell::new(TraceContext::default());
    static PENDING: Cell<usize> = const { Cell::new(0) };
    static CANCELLATION_REPORTED: Cell<bool> = const { Cell::new(false) };
    static DIAGNOSTIC: RefCell<Option<CString>> = const { RefCell::new(None) };
}

pub(crate) fn submission_context() -> TraceContext {
    TRACE.with(|trace| {
        let trace = trace.borrow();
        let mut submissions = trace.submissions.clone();
        if !trace.frames.is_empty() {
            submissions.push(trace.frames[trace.frames.len().saturating_sub(256)..].to_vec());
        }
        if submissions.len() > 16 { submissions.drain(..submissions.len() - 16); }
        TraceContext { submissions, ..Default::default() }
    })
}

/// Install a task's context only while its callback executes on this worker.
/// The guard also restores the worker's previous context during Rust unwinding.
pub(crate) fn with_trace_context<T>(context: &mut TraceContext, work: impl FnOnce() -> T) -> T {
    struct Guard<'a> { context: &'a mut TraceContext, previous: Option<TraceContext> }
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            *self.context = TRACE.with(|trace| trace.replace(self.previous.take().unwrap()));
        }
    }
    let previous = TRACE.with(|trace| trace.replace(std::mem::take(context)));
    let _guard = Guard { context, previous: Some(previous) };
    work()
}

#[no_mangle]
pub extern "C" fn ube_trace_enter(name: *const c_char) {
    TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        let name = string(name);
        let location = trace.resumed.take().filter(|frame| frame.0 == name).map(|frame| frame.1).unwrap_or_default();
        trace.frames.push((name, location));
    });
}
#[no_mangle]
pub extern "C" fn ube_trace_location(location: *const c_char) {
    TRACE.with(|trace| {
        if let Some(frame) = trace.borrow_mut().frames.last_mut() { frame.1 = string(location); }
    });
}
#[no_mangle]
pub extern "C" fn ube_trace_leave() {
    TRACE.with(|trace| { trace.borrow_mut().frames.pop(); });
}
#[no_mangle]
pub extern "C" fn ube_trace_suspend() {
    TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        trace.resumed = trace.frames.pop();
    });
}
fn format_frames(output: &mut String, frames: &[TraceFrame], location: Option<&str>, budget: &mut usize) {
    for (index, (name, current)) in frames.iter().rev().enumerate() {
        if *budget == 0 { output.push_str("\n  ... additional frames omitted"); break; }
        *budget -= 1;
        let location = if index == 0 { location.unwrap_or(current) } else { current };
        output.push_str(&format!("\n  at {name}"));
        if !location.is_empty() { output.push_str(&format!(" ({location})")); }
    }
}
fn format_context(output: &mut String, trace: &TraceContext, location: Option<&str>) {
    let mut budget = 256;
    format_frames(output, &trace.frames, location, &mut budget);
    for frames in trace.submissions.iter().rev() {
        if budget == 0 { break; }
        output.push_str("\n  --- spawned at ---");
        format_frames(output, frames, None, &mut budget);
    }
}
fn capture_trace(type_name: &str, message: &str, location: &str) -> CString {
    let mut output = format!("{type_name}: {message}");
    TRACE.with(|trace| format_context(&mut output, &trace.borrow(), (location != "runtime" && !location.is_empty()).then_some(location)));
    CString::new(output.replace('\0', "\\0")).unwrap()
}

pub(crate) fn same_exception(left: usize, right: usize) -> bool {
    if left == right { return true; }
    if left == 0 || right == 0 { return false; }
    let (left, right) = unsafe { (&*(left as *const ExceptionRecord), &*(right as *const ExceptionRecord)) };
    left.object != 0 && left.object == right.object
}

/// An await creates an immutable observation record. Shared task failures keep
/// their original record, so separate awaiters never change one another's trace.
pub(crate) fn observe_exception(record: usize) -> usize {
    let mut suffix = String::new();
    TRACE.with(|trace| {
        let trace = trace.borrow();
        if !trace.frames.is_empty() {
            suffix.push_str("\n  --- awaited at ---");
            format_context(&mut suffix, &trace, None);
        }
    });
    if suffix.is_empty() { return record; }
    let original = unsafe { &*(record as *const ExceptionRecord) };
    if original.async_links >= 32 { return record; }
    let trace = format!("{}{}", original.stacktrace.to_string_lossy(), suffix);
    let mut observed = Box::new(ExceptionRecord {
        object: original.object, type_name: original.type_name.clone(), message: original.message.clone(),
        location: original.location.clone(), stacktrace: CString::new(trace.replace('\0', "\\0")).unwrap(),
        async_links: original.async_links + 1,
        suppressed: Mutex::new(original.suppressed.lock().unwrap().clone()),
    });
    let pointer = (&mut *observed as *mut ExceptionRecord) as usize;
    records().lock().unwrap().push(observed);
    pointer
}
#[no_mangle]
pub extern "C" fn ube_exception_stacktrace(object: *mut c_void) -> *const c_char {
    if let Some(record) = TRACE.with(|trace| trace.borrow().observed.get(&(object as usize)).copied()) {
        return unsafe { &*(record as *const ExceptionRecord) }.stacktrace.as_ptr();
    }
    records().lock().unwrap().iter().find(|record| record.object == object as usize)
        .map_or(c"".as_ptr(), |record| record.stacktrace.as_ptr())
}

fn string(ptr: *const c_char) -> String {
    if ptr.is_null() { String::new() } else { unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned() }
}
pub fn create(object: usize, type_name: String, message: String, location: String) -> usize {
    if let Some(record) = TRACE.with(|trace| trace.borrow().observed.get(&object).copied()) { return record; }
    let mut arena = records().lock().unwrap();
    if let Some(record) = arena.iter_mut().find(|record| record.object == object && object != 0) {
        return (&mut **record as *mut ExceptionRecord) as usize;
    }
    let stacktrace = capture_trace(&type_name, &message, &location);
    let mut record = Box::new(ExceptionRecord { object, type_name, stacktrace, async_links: 0,
        message: CString::new(message.replace('\0', "\\0")).unwrap(), location,
        suppressed: Mutex::new(Vec::new()) });
    let ptr = (&mut *record as *mut ExceptionRecord) as usize;
    arena.push(record);
    ptr
}
pub fn generic(message: String) -> usize {
    named("GenericError", message)
}
pub fn named(type_name: &str, message: String) -> usize {
    // Built-in error objects share the generated one-pointer message layout.
    let message = CString::new(message.replace('\0', "\\0")).unwrap();
    let object = crate::memory::ube_alloc(std::mem::size_of::<usize>(), 8);
    unsafe { *(object as *mut *const c_char) = message.as_ptr(); }
    let record = create(object as usize, type_name.into(), message.to_string_lossy().into_owned(), "runtime".into());
    // Store the stable record-owned CString address in the object.
    unsafe { *(object as *mut *const c_char) = (*(record as *const ExceptionRecord)).message.as_ptr(); }
    objects().lock().unwrap().insert(object as usize, type_name.into());
    record
}
pub fn set_pending(record: usize) { PENDING.with(|p| p.set(record)); }
pub fn take_pending() -> usize { PENDING.with(|p| p.replace(0)) }
pub fn mark_cancellation_reported() { CANCELLATION_REPORTED.with(|p| p.set(true)); }
pub(crate) fn cancellation_reported() -> bool { CANCELLATION_REPORTED.with(Cell::get) }
pub fn reset_cancellation() { CANCELLATION_REPORTED.with(|p| p.set(false)); }
pub fn message(record: usize) -> String {
    if record == 0 { return String::new(); }
    unsafe { (*(record as *const ExceptionRecord)).message.to_string_lossy().into_owned() }
}

#[no_mangle]
pub extern "C" fn ube_object_register(object: *mut c_void, name: *const c_char) {
    objects().lock().unwrap().insert(object as usize, string(name));
}
#[no_mangle]
pub extern "C" fn ube_object_is(object: *mut c_void, name: *const c_char) -> bool {
    objects().lock().unwrap().get(&(object as usize)).is_some_and(|actual| actual == &string(name))
}
#[no_mangle]
pub extern "C" fn ube_exception_create(object: *mut c_void, name: *const c_char, message: *const c_char, location: *const c_char) -> *mut c_void {
    let name = if name.is_null() { objects().lock().unwrap().get(&(object as usize)).cloned().unwrap_or_else(|| "GenericError".into()) } else { string(name) };
    create(object as usize, name, string(message), string(location)) as *mut c_void
}
#[no_mangle]
pub extern "C" fn ube_exception_object(record: *mut c_void) -> *mut c_void {
    if record.is_null() { return std::ptr::null_mut(); }
    let object = unsafe { (*(record as *const ExceptionRecord)).object };
    TRACE.with(|trace| { trace.borrow_mut().observed.insert(object, record as usize); });
    object as *mut c_void
}
#[no_mangle]
pub extern "C" fn ube_exception_take() -> *mut c_void { take_pending() as *mut c_void }
#[no_mangle]
pub extern "C" fn ube_exception_set(record: *mut c_void) { set_pending(record as usize); }
#[no_mangle]
pub extern "C" fn ube_exception_suppress(active: *mut c_void, original: *mut c_void) {
    if !active.is_null() && !original.is_null() && !same_exception(active as usize, original as usize) {
        unsafe { &*(active as *const ExceptionRecord) }.suppressed.lock().unwrap().push(original as usize);
    }
}
#[no_mangle]
pub extern "C" fn ube_exception_poll_cancel() -> *mut c_void {
    if crate::concurrency::ube_task_is_cancelled() && CANCELLATION_REPORTED.with(|p| !p.replace(true)) {
        generic("task was cancelled".into()) as *mut c_void
    } else { std::ptr::null_mut() }
}
#[no_mangle]
pub extern "C" fn ube_exception_fail_unhandled() {
    let record = take_pending();
    if record != 0 {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let error = unsafe { &*(record as *const ExceptionRecord) };
        eprintln!("Ube error: unhandled exception {}: {} at {}", error.type_name, error.message.to_string_lossy(), error.location);
        for frame in error.stacktrace.to_string_lossy().lines().skip(1) { eprintln!("{frame}"); }
        for &suppressed in error.suppressed.lock().unwrap().iter() {
            eprintln!("  suppressed: {}", message(suppressed));
        }
        crate::ube_runtime_cleanup();
        std::process::exit(1);
    }
}
pub fn cleanup() {
    set_pending(0);
    TRACE.with(|trace| *trace.borrow_mut() = TraceContext::default());
    records().lock().unwrap().clear();
    objects().lock().unwrap().clear();
}

// Type metadata is weak: reclaiming an ordinary generated object must not leave
// a stale class registration for a subsequently reused address.
pub(crate) fn forget_object(pointer: usize) { objects().lock().unwrap().remove(&pointer); }

#[cfg(test)]
mod trace_tests {
    use super::*;

    #[test]
    fn trace_observers_are_isolated_across_workers_and_original_stays_immutable() {
        let mut source=TraceContext::default();
        let original=with_trace_context(&mut source, || {
            ube_trace_enter(c"Source.fail".as_ptr());
            let record=generic("shared failure".into());
            ube_trace_leave();
            record
        });
        let original_trace=unsafe { &*(original as *const ExceptionRecord) }.stacktrace.clone();
        let workers:Vec<_>=["ObserverA.wait", "ObserverB.wait"].into_iter().map(|name| std::thread::spawn(move || {
            let mut context=TraceContext::default();
            with_trace_context(&mut context, || {
                let name=CString::new(name).unwrap();
                ube_trace_enter(name.as_ptr());
                let record=observe_exception(original);
                assert_ne!(record, original);
                assert!(same_exception(record, original));
                ube_exception_suppress(record as *mut c_void, original as *mut c_void);
                assert!(unsafe { &*(record as *const ExceptionRecord) }.suppressed.lock().unwrap().is_empty());
                let object=ube_exception_object(record as *mut c_void);
                let trace=unsafe { CStr::from_ptr(ube_exception_stacktrace(object)) }.to_string_lossy().into_owned();
                assert_eq!(create(object as usize,"GenericError".into(),"ignored".into(),"runtime".into()),record);
                ube_trace_leave();
                trace
            })
        })).collect();
        let traces:Vec<_>=workers.into_iter().map(|worker|worker.join().unwrap()).collect();
        assert!(traces[0].contains("ObserverA.wait") && !traces[0].contains("ObserverB.wait"));
        assert!(traces[1].contains("ObserverB.wait") && !traces[1].contains("ObserverA.wait"));
        assert_eq!(unsafe { &*(original as *const ExceptionRecord) }.stacktrace,original_trace);
    }

    #[test]
    fn trace_context_restores_worker_and_keeps_locations_across_suspension() {
        let mut context=TraceContext::default();
        with_trace_context(&mut context, || {
            ube_trace_enter(c"Task.wait".as_ptr());
            ube_trace_location(c"12:9".as_ptr());
            ube_trace_suspend();
        });
        assert!(TRACE.with(|trace|trace.borrow().frames.is_empty()));
        with_trace_context(&mut context, || {
            ube_trace_enter(c"Task.wait".as_ptr());
            let text=capture_trace("Error","resumed","runtime");
            assert!(text.to_string_lossy().contains("Task.wait (12:9)"));
            ube_trace_leave();
        });
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_trace_context(&mut context, || { ube_trace_enter(c"Task.panic".as_ptr()); panic!("expected"); });
        }));
        assert!(result.is_err());
        assert!(TRACE.with(|trace|trace.borrow().frames.is_empty()));
    }
}
