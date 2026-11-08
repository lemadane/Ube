mod yielding;
use futures_util::{stream::FuturesUnordered, FutureExt, StreamExt};
use std::ffi::c_void;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;
use std::{error::Error, fmt};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::{mpsc, watch, Mutex};

const DEFAULT_CHANNEL_CAPACITY: usize = 64;

thread_local! {
    static CURRENT_TASK_ERROR: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    static LAST_TASK_ERROR: std::cell::RefCell<Option<std::ffi::CString>> = const { std::cell::RefCell::new(None) };
    static CURRENT_CHILDREN: std::cell::RefCell<Option<Arc<StdMutex<Vec<Task<AbiValue>>>>>> = const { std::cell::RefCell::new(None) };
    static CURRENT_CANCELLATION: std::cell::RefCell<Option<Arc<AtomicBool>>> = const { std::cell::RefCell::new(None) };
}

fn set_last_task_error(message: String) {
    let message = message.replace('\0', "\\0");
    LAST_TASK_ERROR.with(|last_error| {
        *last_error.borrow_mut() = Some(
            std::ffi::CString::new(message)
                .expect("NUL bytes were removed from task error"),
        )
    });
}

fn abi_task_handles() -> &'static StdMutex<std::collections::HashMap<usize, Task<AbiValue>>> {
    static HANDLES: OnceLock<StdMutex<std::collections::HashMap<usize, Task<AbiValue>>>> = OnceLock::new();
    HANDLES.get_or_init(Default::default)
}

/// Snapshot registered language task handles; completed handles may remain until disposal.
pub(crate) fn observability_counts() -> (usize, usize) {
    let handles = abi_task_handles().lock().unwrap();
    let active = handles.values().filter(|task| task.result.borrow().is_none()).count();
    (handles.len(), active)
}

fn abi_channel_handles() -> &'static StdMutex<Vec<usize>> {
    static HANDLES: OnceLock<StdMutex<Vec<usize>>> = OnceLock::new();
    HANDLES.get_or_init(|| StdMutex::new(Vec::new()))
}

/// Inline transport value. Presence is independent of payload bits and task errors.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AbiValue {
    pub payload: i64,
    pub meta: u64,
}
impl AbiValue {
    fn present(payload: i64) -> Self {
        Self { payload, meta: 1 }
    }
}

fn track_abi_task(task: Task<AbiValue>) -> *mut c_void {
    CURRENT_CHILDREN.with(|children| {
        if let Some(children) = children.borrow().as_ref() {
            children.lock().unwrap().push(task.clone());
        }
    });
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
    let id = NEXT.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| value.checked_add(1))
        .expect("Ube task handle space exhausted");
    abi_task_handles().lock().unwrap().insert(id, task);
    id as *mut c_void
}

fn lookup_task(ptr: *mut c_void) -> Result<Task<AbiValue>, TaskError> {
    abi_task_handles().lock().unwrap().get(&(ptr as usize)).cloned()
        .ok_or_else(|| TaskError::Failed("invalid or disposed task handle".into()))
}

fn task_value(result: Result<AbiValue, TaskError>) -> AbiValue {
    match result {
        Ok(value) => {
            crate::exceptions::set_pending(0);
            LAST_TASK_ERROR.with(|last_error| *last_error.borrow_mut() = None);
            value
        }
        Err(error) => {
            set_last_task_error(error.to_string());
            let record = match error {
                TaskError::Exception(record) => crate::exceptions::observe_exception(record),
                other => crate::exceptions::generic(other.to_string()),
            };
            crate::exceptions::set_pending(record);
            AbiValue::default()
        }
    }
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskError {
    Cancelled,
    Timeout,
    NoTasks,
    Aggregate(Vec<TaskError>),
    ChannelClosed,
    InvalidCapacity,
    Panicked(String),
    WorkerStopped,
    Failed(String),
    Exception(usize),
}

impl fmt::Display for TaskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => write!(formatter, "task was cancelled"),
            Self::Timeout => write!(formatter, "task timed out"),
            Self::NoTasks => write!(formatter, "combinator requires at least one task"),
            Self::Aggregate(errors) => {
                write!(formatter, "all tasks failed: ")?;
                for (index, error) in errors.iter().enumerate() {
                    if index > 0 {
                        write!(formatter, "; ")?;
                    }
                    write!(formatter, "{error}")?;
                }
                Ok(())
            }
            Self::ChannelClosed => write!(formatter, "channel is closed"),
            Self::InvalidCapacity => {
                write!(formatter, "channel capacity must be greater than zero")
            }
            Self::Panicked(message) => write!(formatter, "task panicked: {message}"),
            Self::WorkerStopped => write!(formatter, "task worker stopped without a result"),
            Self::Failed(message) => write!(formatter, "{message}"),
            Self::Exception(record) => write!(formatter, "{}", crate::exceptions::message(*record)),
        }
    }
}

impl Error for TaskError {}

fn is_cancel_error(error: &TaskError) -> bool {
    match error {
        TaskError::Cancelled => true,
        TaskError::Exception(record) => unsafe {
            let record = &*(*record as *const crate::exceptions::ExceptionRecord);
            record.type_name == "GenericError" && record.message.to_bytes() == b"task was cancelled"
        },
        _ => false,
    }
}
fn error_record(error: TaskError) -> usize {
    match error { TaskError::Exception(record) => record, other => crate::exceptions::generic(other.to_string()) }
}
fn same_task_error(left: &TaskError, right: &TaskError) -> bool {
    match (left, right) {
        (TaskError::Exception(left), TaskError::Exception(right)) => crate::exceptions::same_exception(*left, *right),
        _ => left == right,
    }
}
fn merge_cleanup_error(primary: TaskError, cleanup: TaskError) -> TaskError {
    let (primary, suppressed) = if is_cancel_error(&primary) || primary == TaskError::Timeout {
        (error_record(cleanup), error_record(primary))
    } else { (error_record(primary), error_record(cleanup)) };
    if !crate::exceptions::same_exception(primary, suppressed) {
        unsafe { (&*(primary as *const crate::exceptions::ExceptionRecord)).suppressed.lock().unwrap().push(suppressed); }
    }
    TaskError::Exception(primary)
}


#[derive(Clone)]
pub struct CancellationToken {
    children: Arc<StdMutex<Vec<Task<AbiValue>>>>,
    cancelled: watch::Receiver<bool>,
    cancelled_flag: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled_flag.load(Ordering::Acquire)
    }

    pub async fn cancelled(&mut self) {
        while !self.is_cancelled() && self.cancelled.changed().await.is_ok() {}
    }

    pub fn check(&self) -> Result<(), TaskError> {
        if self.is_cancelled() {
            Err(TaskError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn flag(&self) -> Arc<AtomicBool> {
        self.cancelled_flag.clone()
    }
}

pub struct Task<T> {
    result: watch::Receiver<Option<Result<T, TaskError>>>,
    cancellation: watch::Sender<bool>,
    cancellation_requested: Arc<AtomicBool>,
    cancelled_flag: Arc<AtomicBool>,
    children: Arc<StdMutex<Vec<Task<AbiValue>>>>,
    observed: Arc<AtomicBool>,
}

impl<T> Clone for Task<T> {
    fn clone(&self) -> Self {
        Self {
            result: self.result.clone(),
            cancellation: self.cancellation.clone(),
            cancellation_requested: self.cancellation_requested.clone(),
            cancelled_flag: self.cancelled_flag.clone(),
            children: self.children.clone(),
            observed: self.observed.clone(),
        }
    }
}

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .thread_name("ube-worker")
            .build()
            .expect("failed to initialize Ube's Tokio runtime")
    })
}

fn block_on<F: Future>(future: F) -> F::Output {
    let runtime = runtime();
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(|| runtime.block_on(future))
    } else {
        runtime.block_on(future)
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else {
        "task panicked with a non-string payload".to_owned()
    }
}

pub fn spawn_task<T, F, Fut>(work: F) -> Task<T>
where
    T: Clone + Send + Sync + 'static,
    F: FnOnce(CancellationToken) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, TaskError>> + Send + 'static,
{
    spawn_task_impl(work, false)
}

fn spawn_task_impl<T, F, Fut>(work: F, cooperative: bool) -> Task<T>
where T: Clone + Send + Sync + 'static,
F: FnOnce(CancellationToken) -> Fut + Send + 'static,
Fut: Future<Output=Result<T,TaskError>> + Send + 'static {
    let (result_tx, result) = watch::channel(None);
    let (cancellation, cancellation_rx) = watch::channel(false);
    let cancellation_requested = Arc::new(AtomicBool::new(false));
    let cancelled_flag = Arc::new(AtomicBool::new(false));
    let children = Arc::new(StdMutex::new(Vec::new()));
    let worker_children = children.clone();
    let observed = Arc::new(AtomicBool::new(false));
    let token = CancellationToken {
        children: children.clone(),
        cancelled: cancellation_rx.clone(),
        cancelled_flag: cancelled_flag.clone(),
    };
    let cancellation_requested_by_worker = cancellation_requested.clone();
    let cancelled_flag_by_worker = cancelled_flag.clone();

    runtime().spawn(async move {
        let work = async move { work(token).await };
        let mut outcome = if cooperative { AssertUnwindSafe(work).catch_unwind().await.unwrap_or_else(|p| Err(TaskError::Panicked(panic_message(p)))) } else { tokio::select! {
            biased;
            _ = wait_for_cancellation(cancellation_rx) => Err(TaskError::Cancelled),
            result = AssertUnwindSafe(work).catch_unwind() => {
                result.unwrap_or_else(|payload| Err(TaskError::Panicked(panic_message(payload))))
            }
        }
        };
        let child_error = finish_children(worker_children).await;
        if let Some(error) = child_error {
            outcome = Err(match outcome { Ok(_) => error, Err(primary) => merge_cleanup_error(primary, error) });
        }
        if matches!(outcome, Err(TaskError::Cancelled)) {
            cancellation_requested_by_worker.store(true, Ordering::Release);
            cancelled_flag_by_worker.store(true, Ordering::Release);
        }
        let _ = result_tx.send(Some(outcome));
    });

    Task {
        result,
        cancellation,
        cancellation_requested,
        cancelled_flag,
        children,
        observed,
    }
}

fn completed_task<T>(outcome: Result<T, TaskError>) -> Task<T>
where
    T: Clone + Send + Sync + 'static,
{
    let cancelled = matches!(outcome, Err(TaskError::Cancelled));
    let (_result_tx, result) = watch::channel(Some(outcome));
    let (cancellation, _) = watch::channel(cancelled);
    Task {
        result,
        cancellation,
        cancellation_requested: Arc::new(AtomicBool::new(cancelled)),
        cancelled_flag: Arc::new(AtomicBool::new(cancelled)),
        children: Arc::new(StdMutex::new(Vec::new())),
        observed: Arc::new(AtomicBool::new(false)),
    }
}

// Scope exit cancels unfinished children and joins every callback before
// publishing the parent's result. Already-observed errors are not rethrown.
async fn finish_children(children: Arc<StdMutex<Vec<Task<AbiValue>>>>) -> Option<TaskError> {
    let tasks = std::mem::take(&mut *children.lock().unwrap());
    let mut first_error = None;
    for task in &tasks { task.cancel(); }
    for task in tasks {
        let observed = task.observed.load(Ordering::Acquire);
        if let Err(error) = task.wait_result().await {
            if !observed && !is_cancel_error(&error) && first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    first_error
}

#[no_mangle]
pub extern "C" fn ube_task_scope_enter() {
    CURRENT_CHILDREN.with(|children| *children.borrow_mut() = Some(Arc::new(StdMutex::new(Vec::new()))));
}

#[no_mangle]
pub extern "C" fn ube_task_scope_leave() {
    let pending = crate::exceptions::take_pending();
    let children = CURRENT_CHILDREN.with(|children| children.borrow_mut().take());
    let error = children.and_then(|children| block_on(finish_children(children)));
    if pending != 0 {
        let pending = error.map(|error| error_record(merge_cleanup_error(TaskError::Exception(pending), error))).unwrap_or(pending);
        crate::exceptions::set_pending(pending);
    } else if let Some(error) = error { task_value(Err(error)); }
}

async fn wait_for_cancellation(mut cancellation: watch::Receiver<bool>) {
    while !*cancellation.borrow() && cancellation.changed().await.is_ok() {}
}

impl<T> Task<T>
where
    T: Clone + Send + Sync + 'static,
{
    async fn wait_result(&self) -> Result<T, TaskError> {
        self.observed.store(true, Ordering::Release);
        let mut result = self.result.clone();
        loop {
            let completed = result.borrow().clone();
            if let Some(outcome) = completed {
                return outcome;
            }
            if result.changed().await.is_err() {
                return Err(TaskError::WorkerStopped);
            }
        }
    }

    pub async fn join(&self) -> Result<T, TaskError> {
        self.wait_result().await
    }

    pub fn await_result(&self) -> Result<T, TaskError> {
        block_on(self.wait_result())
    }

    pub fn await_timeout(&self, timeout: Duration) -> Result<T, TaskError> {
        let outcome = block_on(async { tokio::time::timeout(timeout, self.wait_result()).await });
        match outcome {
            Ok(result) => result,
            Err(_) => {
                self.cancel();
                Err(TaskError::Timeout)
            }
        }
    }

    pub fn cancel(&self) -> bool {
        if self.result.borrow().is_some() {
            return false;
        }
        if self.cancellation_requested.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.cancelled_flag.store(true, Ordering::Release);
        let children = self.children.lock().unwrap().clone();
        for child in children { child.cancel(); }
        self.cancellation.send(true).is_ok()
    }

    pub fn is_completed(&self) -> bool {
        self.result.borrow().is_some()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled_flag.load(Ordering::Acquire)
    }

    pub async fn all(tasks: Vec<Self>) -> Result<Vec<T>, TaskError> {
        let mut pending = FuturesUnordered::new();
        for (index, task) in tasks.iter().cloned().enumerate() {
            pending.push(async move { (index, task.wait_result().await) });
        }

        let mut results = vec![None; tasks.len()];
        while let Some((index, result)) = pending.next().await {
            match result {
                Ok(value) => results[index] = Some(value),
                Err(error) => {
                    for task in &tasks {
                        task.cancel();
                    }
                    return Err(error);
                }
            }
        }
        Ok(results
            .into_iter()
            .map(|result| result.expect("all task results were collected"))
            .collect())
    }

    pub async fn any(tasks: Vec<Self>) -> Result<T, TaskError> {
        if tasks.is_empty() {
            return Err(TaskError::NoTasks);
        }
        let mut pending = FuturesUnordered::new();
        for task in tasks.iter().cloned() {
            pending.push(async move { task.wait_result().await });
        }

        let mut errors = Vec::new();
        while let Some(result) = pending.next().await {
            match result {
                Ok(value) => {
                    for task in &tasks {
                        task.cancel();
                    }
                    return Ok(value);
                }
                Err(error) => errors.push(error),
            }
        }
        Err(TaskError::Aggregate(errors))
    }

    pub async fn race(tasks: Vec<Self>) -> Result<T, TaskError> {
        if tasks.is_empty() {
            return Err(TaskError::NoTasks);
        }
        let mut pending = FuturesUnordered::new();
        for task in tasks.iter().cloned() {
            pending.push(async move { task.wait_result().await });
        }
        let outcome = pending.next().await.unwrap_or(Err(TaskError::NoTasks));
        for task in &tasks {
            task.cancel();
        }
        outcome
    }

    pub async fn all_settled(tasks: Vec<Self>) -> Vec<Result<T, TaskError>> {
        let mut pending = FuturesUnordered::new();
        for (index, task) in tasks.iter().cloned().enumerate() {
            pending.push(async move { (index, task.wait_result().await) });
        }
        let mut results = vec![None; tasks.len()];
        while let Some((index, result)) = pending.next().await {
            results[index] = Some(result);
        }
        results
            .into_iter()
            .map(|result| result.expect("all settled task results were collected"))
            .collect()
    }
}

pub struct Channel<T> {
    sender: mpsc::Sender<T>,
    receiver: Arc<Mutex<mpsc::Receiver<T>>>,
    closed: Arc<AtomicBool>,
    closed_signal: watch::Sender<bool>,
}

impl<T> Clone for Channel<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            receiver: self.receiver.clone(),
            closed: self.closed.clone(),
            closed_signal: self.closed_signal.clone(),
        }
    }
}

impl<T> Channel<T> {
    pub fn bounded(capacity: usize) -> Result<Self, TaskError> {
        if capacity == 0 {
            return Err(TaskError::InvalidCapacity);
        }
        let (sender, receiver) = mpsc::channel(capacity);
        let (closed_signal, _) = watch::channel(false);
        Ok(Self {
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
            closed: Arc::new(AtomicBool::new(false)),
            closed_signal,
        })
    }

    pub fn new() -> Self {
        Self::bounded(DEFAULT_CHANNEL_CAPACITY).expect("the default channel capacity is non-zero")
    }

    pub async fn send(&self, value: T) -> Result<(), TaskError> {
        let mut closed_signal = self.closed_signal.subscribe();
        if *closed_signal.borrow() {
            return Err(TaskError::ChannelClosed);
        }
        tokio::select! {
            biased;
            _ = closed_signal.changed() => Err(TaskError::ChannelClosed),
            result = self.sender.send(value) => result.map_err(|_| TaskError::ChannelClosed),
        }
    }

    pub async fn receive(&self) -> Result<Option<T>, TaskError> {
        let mut closed_signal = self.closed_signal.subscribe();
        let mut receiver = self.receiver.lock().await;
        loop {
            if *closed_signal.borrow() {
                receiver.close();
            }
            tokio::select! {
                biased;
                value = receiver.recv() => {
                    if value.is_none() {
                        self.closed.store(true, Ordering::Release);
                    }
                    return Ok(value);
                }
                changed = closed_signal.changed(), if !*closed_signal.borrow() => {
                    if changed.is_err() {
                        receiver.close();
                    }
                }
            }
        }
    }

    pub fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            self.closed_signal.send_replace(true);
        }
    }
}

impl<T> Default for Channel<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct UbeChannel {
    channel: Channel<AbiValue>,
}

#[repr(C)]
struct UbeTaskContext {
    values: Vec<AbiValue>,
}

#[no_mangle]
pub extern "C" fn ube_task_context_create(size: i64) -> *mut c_void {
    let capacity = usize::try_from(size).unwrap_or(0);
    Box::into_raw(Box::new(UbeTaskContext {
        values: vec![AbiValue::default(); capacity],
    })) as *mut c_void
}

#[no_mangle]
pub extern "C" fn ube_task_context_set(ptr: *mut c_void, index: i64, value: i64) {
    if ptr.is_null() || index < 0 {
        return;
    }
    let context = unsafe { &mut *(ptr as *mut UbeTaskContext) };
    if let Some(slot) = context.values.get_mut(index as usize) {
        *slot = AbiValue::present(value);
    }
}

#[no_mangle]
pub extern "C" fn ube_task_context_get(ptr: *mut c_void, index: i64) -> i64 {
    if ptr.is_null() || index < 0 {
        return 0;
    }
    unsafe {
        (&(*(ptr as *const UbeTaskContext)).values)
            .get(index as usize)
            .copied()
            .unwrap_or_default().payload
    }
}

#[no_mangle]
pub extern "C" fn ube_task_spawn(
    callback: Option<extern "C" fn(*mut c_void) -> i64>,
    context: *mut c_void,
) -> *mut c_void {
    let Some(callback) = callback else {
        if !context.is_null() {
            unsafe { drop(Box::from_raw(context as *mut UbeTaskContext)) };
        }
        return std::ptr::null_mut();
    };
    spawn_abi_callback(move |context| AbiValue::present(callback(context)), context)
}

#[no_mangle]
pub extern "C" fn ube_task_spawn_value(
    callback: Option<extern "C" fn(*mut c_void) -> AbiValue>,
    context: *mut c_void,
) -> *mut c_void {
    let Some(callback) = callback else {
        if !context.is_null() {
            unsafe { drop(Box::from_raw(context as *mut UbeTaskContext)) };
        }
        return std::ptr::null_mut();
    };
    spawn_abi_callback(move |context| callback(context), context)
}

fn spawn_abi_callback(
    callback: impl FnOnce(*mut c_void) -> AbiValue + Send + 'static,
    context: *mut c_void,
) -> *mut c_void {
    let context = if context.is_null() {
        Box::new(UbeTaskContext { values: Vec::new() })
    } else {
        unsafe { Box::from_raw(context as *mut UbeTaskContext) }
    };
    let log_context = crate::logging::snapshot();
    let mut trace_context = crate::exceptions::submission_context();
    let task = spawn_task_impl(move |token| async move {
        let flag = token.flag();
        let children = token.children.clone();
        let value = tokio::task::spawn_blocking(move || crate::logging::with_context(log_context, || crate::exceptions::with_trace_context(&mut trace_context, || {
            CURRENT_CHILDREN.with(|current| *current.borrow_mut() = Some(children));
            CURRENT_TASK_ERROR.with(|error| *error.borrow_mut() = None);
            crate::exceptions::set_pending(0);
            crate::exceptions::reset_cancellation();
            CURRENT_CANCELLATION.with(|current| *current.borrow_mut() = Some(flag));
            let value = callback((&*context as *const UbeTaskContext) as *mut c_void);
            CURRENT_CHILDREN.with(|current| *current.borrow_mut() = None);
            let error = CURRENT_TASK_ERROR.with(|error| error.borrow_mut().take());
            CURRENT_CANCELLATION.with(|current| *current.borrow_mut() = None);
            let pending = crate::exceptions::take_pending();
            if pending != 0 { Err(TaskError::Exception(pending)) }
            else { error.map_or(Ok(value), |message| Err(TaskError::Failed(message))) }
        })))
        .await
        .map_err(|error| TaskError::Panicked(error.to_string()))?;
        value
    }, true);
    track_abi_task(task)
}

#[no_mangle]
pub extern "C" fn ube_task_create(val: i64) -> *mut c_void {
    track_abi_task(spawn_task(move |_| async move { Ok(AbiValue::present(val)) }))
}

#[no_mangle]
pub extern "C" fn ube_task_completed_value(payload: i64, meta: u64) -> *mut c_void {
    track_abi_task(completed_task(Ok(AbiValue { payload, meta })))
}

#[no_mangle]
pub extern "C" fn ube_task_failed(record: *mut c_void) -> *mut c_void {
    let error = if record.is_null() {
        TaskError::Failed("Async.failed requires an Error".into())
    } else {
        TaskError::Exception(record as usize)
    };
    track_abi_task(completed_task(Err(error)))
}

#[no_mangle]
pub extern "C" fn ube_task_delay(milliseconds: i64) -> *mut c_void {
    if milliseconds < 0 {
        return track_abi_task(completed_task(Err(TaskError::Failed(
            "delay must not be negative".into(),
        ))));
    }
    let duration = Duration::from_millis(milliseconds as u64);
    track_abi_task(spawn_task(move |_| async move {
        tokio::time::sleep(duration).await;
        Ok(AbiValue::default())
    }))
}

#[no_mangle]
pub extern "C" fn ube_task_await(ptr: *mut c_void) -> i64 { ube_task_await_value(ptr).payload }

#[no_mangle]
pub extern "C" fn ube_task_await_value(ptr: *mut c_void) -> AbiValue {
    if ptr.is_null() {
        set_last_task_error("task handle is null".into());
        return task_value(Err(TaskError::Failed("task handle is null".into())));
    }
    let task = match lookup_task(ptr) { Ok(task) => task, Err(error) => return task_value(Err(error)) };
    // Polling and awaiting share one cancellation delivery per callback. Once
    // delivered, cleanup awaits must not re-deliver the parent's cancellation;
    // errors from the awaited task itself still propagate normally.
    let cancellation = CURRENT_CANCELLATION.with(|current| current.borrow().clone())
        .filter(|_| !crate::exceptions::cancellation_reported());
    let result = if let Some(flag) = cancellation {
        block_on(async {
            tokio::select! {
                result = task.wait_result() => result,
                _ = async { while !flag.load(Ordering::Acquire) { tokio::time::sleep(Duration::from_millis(2)).await; } } => {
                    task.cancel();
                    crate::exceptions::mark_cancellation_reported();
                    match task.wait_result().await {
                        Err(error) if !is_cancel_error(&error) => Err(merge_cleanup_error(TaskError::Cancelled, error)),
                        _ => Err(TaskError::Cancelled),
                    }
                }
            }
        })
    } else { task.await_result() };
    // Downward cancellation may complete the awaited child before the flag
    // branch wins. That is still delivery of this callback's cancellation;
    // otherwise a finally await would receive it a second time.
    if ube_task_is_cancelled() && result.is_err() {
        crate::exceptions::mark_cancellation_reported();
    }
    task_value(result)
}

#[no_mangle]
pub extern "C" fn ube_task_last_error() -> *const std::ffi::c_char {
    LAST_TASK_ERROR.with(|last_error| {
        last_error
            .borrow()
            .as_ref()
            .map_or(std::ptr::null(), |error| error.as_ptr())
    })
}

#[no_mangle]
pub extern "C" fn ube_task_set_error(message: *const std::ffi::c_char) {
    if message.is_null() {
        return;
    }
    let message = unsafe { std::ffi::CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned();
    CURRENT_TASK_ERROR.with(|error| *error.borrow_mut() = Some(message));
}

#[no_mangle]
pub extern "C" fn ube_task_is_cancelled() -> bool {
    CURRENT_CANCELLATION.with(|current| {
        current
            .borrow()
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    })
}

#[no_mangle]
pub extern "C" fn ube_task_set_cancelled() {
    CURRENT_TASK_ERROR.with(|error| *error.borrow_mut() = Some("task was cancelled".into()));
}

#[no_mangle]
pub extern "C" fn ube_task_yield() {
    std::thread::yield_now();
}

#[no_mangle]
pub extern "C" fn ube_task_cancel(ptr: *mut c_void) -> bool {
    if ptr.is_null() {
        return false;
    }
    lookup_task(ptr).map(|task| task.cancel()).unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn ube_task_is_completed(ptr: *mut c_void) -> bool {
    !ptr.is_null() && lookup_task(ptr).is_ok_and(|task| task.is_completed())
}

#[no_mangle]
pub extern "C" fn ube_task_was_cancelled(ptr: *mut c_void) -> bool {
    !ptr.is_null() && lookup_task(ptr).is_ok_and(|task| task.is_cancelled())
}

/// Returns a new task that fails with "task timed out" (and cancels `ptr`)
/// if `ptr` does not finish within `timeout_ms` milliseconds.
#[no_mangle]
pub extern "C" fn ube_task_timeout(ptr: *mut c_void, timeout_ms: i64) -> *mut c_void {
    if ptr.is_null() {
        return track_abi_task(spawn_task(|_| async { Err(TaskError::Failed("task handle is null".into())) }));
    }
    if timeout_ms < 0 {
        return track_abi_task(spawn_task(|_| async {
            Err(TaskError::Failed("timeout must not be negative".into()))
        }));
    }
    let inner = match lookup_task(ptr) { Ok(task) => task, Err(error) => return track_abi_task(spawn_task(|_| async move { Err(error) })) };
    let duration = Duration::from_millis(timeout_ms as u64);
    track_abi_task(spawn_task_impl(move |mut token| async move {
        let mut result = tokio::select! {
            biased;
            _ = token.cancelled() => Err(TaskError::Cancelled),
            result = tokio::time::timeout(duration, inner.wait_result()) => match result {
                Ok(result) => result,
                Err(_) => Err(TaskError::Timeout),
            },
        };
        if matches!(result, Err(TaskError::Cancelled | TaskError::Timeout)) {
            inner.cancel();
            if let Err(error) = inner.wait_result().await {
                if !is_cancel_error(&error) { result = Err(merge_cleanup_error(result.unwrap_err(), error)); }
            }
        }
        result
    }, true))
}

/// Called after an unguarded `await` outside an async function: stops the
/// program with a clear message if the awaited task failed.
#[no_mangle]
pub extern "C" fn ube_task_fail_unhandled() {
    let error = LAST_TASK_ERROR.with(|last_error| last_error.borrow_mut().take());
    if let Some(error) = error {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        eprintln!("Ube error: unhandled task failure: {}", error.to_string_lossy());
        std::process::exit(1);
    }
}

/// Called after an unguarded `await` inside an async function: moves the
/// awaited failure into the current task so it propagates to its awaiter.
#[no_mangle]
pub extern "C" fn ube_task_propagate_error() -> bool {
    let error = LAST_TASK_ERROR.with(|last_error| last_error.borrow_mut().take());
    match error {
        Some(error) => {
            let message = error.to_string_lossy().into_owned();
            CURRENT_TASK_ERROR.with(|current| *current.borrow_mut() = Some(message));
            true
        }
        None => false,
    }
}

#[no_mangle]
pub extern "C" fn ube_channel_create() -> *mut UbeChannel {
    ube_channel_create_capacity(DEFAULT_CHANNEL_CAPACITY as i64)
}

#[no_mangle]
pub extern "C" fn ube_channel_create_capacity(capacity: i64) -> *mut UbeChannel {
    let Ok(capacity) = usize::try_from(capacity) else {
        eprintln!("Ube channel capacity must be a positive integer");
        return std::ptr::null_mut();
    };
    if capacity == 0 {
        eprintln!("Ube channel capacity must be a positive integer");
        return std::ptr::null_mut();
    }
    let Ok(channel) = Channel::bounded(capacity) else {
        eprintln!("Ube channel capacity is invalid");
        return std::ptr::null_mut();
    };
    let channel = Box::into_raw(Box::new(UbeChannel { channel }));
    abi_channel_handles()
        .lock()
        .expect("Ube channel handle registry was poisoned")
        .push(channel as usize);
    channel
}

#[no_mangle]
pub extern "C" fn ube_channel_send(ptr: *mut UbeChannel, val: i64) -> bool {
    if ptr.is_null() {
        return false;
    }
    let channel = unsafe { &(*ptr).channel };
    block_on(channel.send(AbiValue::present(val))).is_ok()
}

#[no_mangle]
pub extern "C" fn ube_channel_send_async(ptr: *mut UbeChannel, val: i64) -> *mut c_void { ube_channel_send_value_async(ptr, val, 1) }

#[no_mangle]
pub extern "C" fn ube_channel_send_value_async(ptr: *mut UbeChannel, payload: i64, meta: u64) -> *mut c_void {
    if ptr.is_null() {
        set_last_task_error("channel handle is null".into());
        return std::ptr::null_mut();
    }
    let channel = unsafe { (*ptr).channel.clone() };
    track_abi_task(spawn_task(move |_| async move {
        match channel.send(AbiValue { payload, meta }).await {
            Ok(()) => Ok(AbiValue { payload: 1, meta: 1 | (4 << 8) }),
            Err(TaskError::ChannelClosed) => Ok(AbiValue { payload: 0, meta: 1 | (4 << 8) }),
            Err(error) => Err(error),
        }
    }))
}

#[no_mangle]
pub extern "C" fn ube_channel_recv(ptr: *mut UbeChannel) -> i64 {
    if ptr.is_null() {
        set_last_task_error("channel handle is null".into());
        return 0;
    }
    let channel = unsafe { &(*ptr).channel };
    match block_on(channel.receive()) {
        Ok(Some(value)) => {
            LAST_TASK_ERROR.with(|last_error| *last_error.borrow_mut() = None);
            value.payload
        }
        Ok(None) => {
            set_last_task_error(TaskError::ChannelClosed.to_string());
            0
        }
        Err(error) => {
            set_last_task_error(error.to_string());
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_channel_recv_async(ptr: *mut UbeChannel) -> *mut c_void {
    if ptr.is_null() {
        set_last_task_error("channel handle is null".into());
        return std::ptr::null_mut();
    }
    let channel = unsafe { (*ptr).channel.clone() };
    track_abi_task(spawn_task(move |_| async move {
        match channel.receive().await? {
            Some(value) => Ok(value),
            None => Err(TaskError::ChannelClosed),
        }
    }))
}

#[no_mangle]
pub extern "C" fn ube_channel_close(ptr: *mut UbeChannel) {
    if !ptr.is_null() {
        unsafe { (&(*ptr).channel).close() };
    }
}

#[no_mangle]
pub extern "C" fn ube_channel_dispose(ptr: *mut UbeChannel) {
    if !ptr.is_null() {
        abi_channel_handles()
            .lock()
            .expect("Ube channel handle registry was poisoned")
            .retain(|handle| *handle != ptr as usize);
        unsafe { drop(Box::from_raw(ptr)) };
    }
}

fn clone_task_list(list_ptr: *mut i8) -> Result<Vec<Task<AbiValue>>, TaskError> {
    let values = crate::collections::snapshot(list_ptr.cast())
        .ok_or_else(|| TaskError::Failed("task list is null".into()))?;
    values.into_iter().map(|value| lookup_task(value as *mut c_void)).collect()
}

fn combinator_task<F, Fut>(list_ptr: *mut i8, work: F) -> *mut c_void
where F: FnOnce(Vec<Task<AbiValue>>) -> Fut + Send + 'static,
      Fut: Future<Output = Result<AbiValue, TaskError>> + Send + 'static {
    let tasks = match clone_task_list(list_ptr) {
        Ok(tasks) => tasks,
        Err(error) => return track_abi_task(spawn_task(|_| async move { Err(error) })),
    };
    let children = tasks.clone();
    // Cooperative cancellation keeps the wrapper alive while its child
    // callbacks run finally blocks. It only completes after every child joins.
    let task = spawn_task_impl(move |mut token| async move {
        let mut outcome = tokio::select! {
            biased;
            _ = token.cancelled() => Err(TaskError::Cancelled),
            result = work(tasks) => result,
        };
        for child in &children { child.cancel(); }
        for child in &children {
            if let Err(error) = child.wait_result().await {
                if child.cancelled_flag.load(Ordering::Acquire) && !is_cancel_error(&error) && outcome.as_ref().err().is_none_or(|primary| !same_task_error(primary, &error)) {
                    outcome = Err(match outcome { Ok(_) => error, Err(primary) => merge_cleanup_error(primary, error) });
                }
            }
        }
        outcome
    }, true);
    track_abi_task(task)
}

#[no_mangle]
pub extern "C" fn ube_task_all_async(list_ptr: *mut i8) -> *mut c_void {
    combinator_task(list_ptr, |tasks| async move {
        let values = Task::all(tasks).await?;
        let list = crate::collections::ube_list_create();
        for value in values { crate::collections::ube_list_add(list, value.payload); }
        Ok(AbiValue { payload: list as usize as i64, meta: 1 | (7 << 8) })
    })
}
#[no_mangle]
pub extern "C" fn ube_task_any_async(list_ptr: *mut i8) -> *mut c_void {
    combinator_task(list_ptr, |tasks| async move { Task::any(tasks).await })
}
#[no_mangle]
pub extern "C" fn ube_task_race_async(list_ptr: *mut i8) -> *mut c_void {
    combinator_task(list_ptr, |tasks| async move { Task::race(tasks).await })
}

// Settled results retain the full transport value and original exception.
struct SettledOutcome(Result<AbiValue, TaskError>);
fn settled_handles() -> &'static StdMutex<std::collections::HashMap<usize, Box<SettledOutcome>>> {
    static HANDLES: OnceLock<StdMutex<std::collections::HashMap<usize, Box<SettledOutcome>>>> = OnceLock::new();
    HANDLES.get_or_init(Default::default)
}
fn outcome(handle: *mut c_void) -> Result<AbiValue, TaskError> {
    settled_handles().lock().unwrap().get(&(handle as usize))
        .map(|value| value.0.clone())
        .unwrap_or_else(|| Err(TaskError::Failed("invalid task outcome".into())))
}
#[no_mangle]
pub extern "C" fn ube_task_outcome_succeeded(handle: *mut c_void) -> bool { outcome(handle).is_ok() }
#[no_mangle]
pub extern "C" fn ube_task_outcome_value(handle: *mut c_void) -> i64 { typed_outcome(handle, &[0, 1, 2, 3]).payload }
fn typed_outcome(handle: *mut c_void, kinds: &[u64]) -> AbiValue {
    let result = outcome(handle).and_then(|value| {
        if value.meta != 0 && kinds.contains(&(value.meta >> 8)) { Ok(value) }
        else { Err(TaskError::Failed("task outcome requires a matching value accessor".into())) }
    });
    task_value(result)
}
#[no_mangle]
pub extern "C" fn ube_task_outcome_bool(handle: *mut c_void) -> bool { typed_outcome(handle, &[4]).payload != 0 }
#[no_mangle]
pub extern "C" fn ube_task_outcome_decimal(handle: *mut c_void) -> f64 { f64::from_bits(typed_outcome(handle, &[5]).payload as u64) }
#[no_mangle]
pub extern "C" fn ube_task_outcome_string(handle: *mut c_void) -> *const std::ffi::c_char { typed_outcome(handle, &[6]).payload as *const _ }
#[no_mangle]
pub extern "C" fn ube_task_outcome_error(handle: *mut c_void) -> *mut c_void {
    match outcome(handle) {
        Ok(_) => std::ptr::null_mut(),
        Err(error) => {
            let record = match error { TaskError::Exception(record) => record, other => crate::exceptions::generic(other.to_string()) };
            crate::exceptions::ube_exception_object(record as *mut c_void)
        }
    }
}
#[no_mangle]
pub extern "C" fn ube_task_outcomes_len(list: *mut crate::collections::UbeList) -> i32 {
    crate::collections::ube_list_len(list) as i32
}
#[no_mangle]
pub extern "C" fn ube_task_outcomes_get(list: *mut crate::collections::UbeList, index: i32) -> *mut c_void {
    let handle = crate::collections::ube_list_get(list, index as i64) as *mut c_void;
    if !settled_handles().lock().unwrap().contains_key(&(handle as usize)) {
        task_value(Err(TaskError::Failed("task outcome index out of bounds".into())));
        return std::ptr::null_mut();
    }
    handle
}
#[no_mangle]
pub extern "C" fn ube_task_all_settled_async(list_ptr: *mut i8) -> *mut c_void {
    combinator_task(list_ptr, |tasks| async move {
        let outcomes = Task::all_settled(tasks).await;
        let list = crate::collections::ube_list_create();
        for result in outcomes {
            let mut entry = Box::new(SettledOutcome(result));
            let handle = (&mut *entry as *mut SettledOutcome) as usize;
            settled_handles().lock().unwrap().insert(handle, entry);
            crate::collections::ube_list_add(list, handle as i64);
        }
        Ok(AbiValue { payload: list as usize as i64, meta: 1 | (7 << 8) })
    })
}
#[no_mangle]
pub extern "C" fn ube_task_all(list: *mut i8) -> *mut i8 { ube_task_await(ube_task_all_async(list)) as *mut i8 }
#[no_mangle]
pub extern "C" fn ube_task_any(list: *mut i8) -> i64 { ube_task_await(ube_task_any_async(list)) }
#[no_mangle]
pub extern "C" fn ube_task_race(list: *mut i8) -> i64 { ube_task_await(ube_task_race_async(list)) }
#[no_mangle]
pub extern "C" fn ube_task_all_settled(list: *mut i8) -> *mut i8 { ube_task_await(ube_task_all_settled_async(list)) as *mut i8 }

#[no_mangle]
pub extern "C" fn ube_task_dispose(ptr: *mut c_void) {
    abi_task_handles().lock().unwrap().remove(&(ptr as usize));
}

pub fn ube_concurrency_cleanup() {
    loop {
        let tasks: Vec<_> = {
            let handles = abi_task_handles().lock().unwrap();
            handles.iter().map(|(handle, task)| (*handle, task.clone())).collect()
        };
        if tasks.is_empty() { break; }
        for (_, task) in &tasks { task.cancel(); }
        for (_, task) in &tasks { let _ = task.await_result(); }
        for (handle, _) in tasks { ube_task_dispose(handle as *mut c_void); }
    }
    settled_handles().lock().unwrap().clear();
    let channels = std::mem::take(
        &mut *abi_channel_handles()
            .lock()
            .expect("Ube channel handle registry was poisoned"),
    );
    for handle in channels {
        let channel = unsafe { Box::from_raw(handle as *mut UbeChannel) };
        channel.channel.close();
    }
}


#[no_mangle]
pub extern "C" fn ube_task_context_set_value(ptr: *mut c_void, index: i64, payload: i64, meta: u64) {
    if ptr.is_null() || index < 0 { return; }
    let context = unsafe { &mut *(ptr as *mut UbeTaskContext) };
    if let Some(slot) = context.values.get_mut(index as usize) { *slot = AbiValue { payload, meta }; }
}
#[no_mangle]
pub extern "C" fn ube_task_context_get_value(ptr: *mut c_void, index: i64) -> AbiValue {
    if ptr.is_null() || index < 0 { return AbiValue::default(); }
    unsafe { (&(*(ptr as *const UbeTaskContext)).values).get(index as usize).copied().unwrap_or_default() }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn disposed_handles_are_rejected_and_never_reused() {
        let task = ube_task_create(42);
        assert_eq!(ube_task_await(task), 42);
        ube_task_dispose(task);
        ube_task_dispose(task);
        let another = ube_task_create(7);
        assert_ne!(task, another);
        assert!(!ube_task_cancel(task));
        assert_eq!(ube_task_await(task), 0);
        let error = crate::exceptions::take_pending();
        assert_eq!(crate::exceptions::message(error), "invalid or disposed task handle");
        assert_eq!(ube_task_await(another), 7);
        ube_task_dispose(another);
    }

    #[test]
    fn cancellation_delivered_by_poll_does_not_interrupt_later_awaits() {
        // Isolate the worker-local cancellation state from other runtime tests.
        std::thread::spawn(|| {
            CURRENT_CANCELLATION.with(|current| {
                *current.borrow_mut() = Some(Arc::new(AtomicBool::new(true)));
            });
            crate::exceptions::reset_cancellation();
            assert!(!crate::exceptions::ube_exception_poll_cancel().is_null());
            assert!(crate::exceptions::ube_exception_poll_cancel().is_null());

            let task = spawn_task(|_| async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                Ok(AbiValue::present(42))
            });
            let handle = track_abi_task(task.clone());
            assert_eq!(ube_task_await(handle), 42);
            assert_eq!(crate::exceptions::take_pending(), 0);

            let failed = spawn_task::<AbiValue, _, _>(|_| async { Err(TaskError::Cancelled) });
            let handle = track_abi_task(failed.clone());
            assert_eq!(ube_task_await(handle), 0);
            let record = crate::exceptions::take_pending();
            assert_ne!(record, 0);
            assert_eq!(crate::exceptions::message(record), "task was cancelled");
        }).join().unwrap();
    }

    extern "C" fn failing_ube_callback(_: *mut c_void) -> i64 {
        ube_task_set_error(b"task failed\0".as_ptr().cast());
        0
    }

    #[test]
    fn ffi_task_errors_are_available_to_the_awaiting_language() {
        let task = ube_task_spawn(Some(failing_ube_callback), std::ptr::null_mut());
        assert_eq!(ube_task_await(task), 0);
        let error = unsafe { std::ffi::CStr::from_ptr(ube_task_last_error()) };
        assert_eq!(error.to_str().unwrap(), "task failed");
        ube_task_dispose(task);
    }

    #[test]
    fn ffi_channel_is_typed_at_the_compiler_abi_and_close_is_observable() {
        let channel = ube_channel_create_capacity(1);
        assert!(!channel.is_null());
        assert!(ube_channel_send(channel, 27));
        ube_channel_close(channel);
        assert_eq!(ube_channel_recv(channel), 27);
        assert_eq!(ube_channel_recv(channel), 0);
        let error = unsafe { std::ffi::CStr::from_ptr(ube_task_last_error()) };
        assert_eq!(error.to_str().unwrap(), "channel is closed");
        assert!(!ube_channel_send(channel, 28));
        ube_channel_dispose(channel);
    }

    #[test]
    fn ffi_async_channel_operations_return_before_the_operation_can_finish() {
        let channel = ube_channel_create_capacity(1);
        let receive = ube_channel_recv_async(channel);
        assert!(!receive.is_null());
        assert!(ube_channel_send(channel, 73));
        assert_eq!(ube_task_await(receive), 73);
        ube_task_dispose(receive);

        let send = ube_channel_send_async(channel, 99);
        assert!(!send.is_null());
        assert_eq!(ube_task_await(send), 1);
        assert_eq!(ube_channel_recv(channel), 99);
        ube_task_dispose(send);
        ube_channel_dispose(channel);
    }

    #[test]
    fn tasks_execute_concurrently_and_all_preserves_input_order() {
        let started = Arc::new(AtomicUsize::new(0));
        let tasks = (0..2)
            .map(|value| {
                let started = started.clone();
                spawn_task(move |_| async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    Ok(value)
                })
            })
            .collect::<Vec<_>>();

        assert_eq!(block_on(Task::all(tasks)).unwrap(), vec![0, 1]);
        assert_eq!(started.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn race_uses_completion_order_and_any_ignores_failures() {
        let slow = spawn_task(|_| async {
            tokio::time::sleep(Duration::from_millis(40)).await;
            Ok(1)
        });
        let fast = spawn_task(|_| async {
            tokio::time::sleep(Duration::from_millis(5)).await;
            Ok(2)
        });
        assert_eq!(block_on(Task::race(vec![slow, fast])).unwrap(), 2);

        let failed = spawn_task(|_| async { Err::<i64, _>(TaskError::Failed("expected".into())) });
        let success = spawn_task(|_| async { Ok(3) });
        assert_eq!(block_on(Task::any(vec![failed, success])).unwrap(), 3);
    }

    #[test]
    fn all_fails_fast_and_all_settled_keeps_each_outcome() {
        let failure = spawn_task(|_| async { Err::<i64, _>(TaskError::Failed("boom".into())) });
        let outcome = block_on(Task::all(vec![failure.clone()]));
        assert_eq!(outcome, Err(TaskError::Failed("boom".into())));

        let settled = block_on(Task::all_settled(vec![
            spawn_task(|_| async { Ok(7) }),
            failure,
        ]));
        assert_eq!(settled, vec![Ok(7), Err(TaskError::Failed("boom".into()))]);
    }

    #[test]
    fn empty_combinator_inputs_and_panics_are_explicit() {
        assert_eq!(
            block_on(Task::<i64>::any(Vec::new())),
            Err(TaskError::NoTasks)
        );
        assert_eq!(
            block_on(Task::<i64>::race(Vec::new())),
            Err(TaskError::NoTasks)
        );
        assert_eq!(block_on(Task::<i64>::all(Vec::new())).unwrap(), Vec::<i64>::new());

        let panicked = spawn_task::<i64, _, _>(|_| async { panic!("test panic") });
        assert_eq!(
            panicked.await_result(),
            Err(TaskError::Panicked("test panic".into()))
        );
    }

    #[test]
    fn timeout_and_cancellation_are_observable() {
        let timed = spawn_task(|mut token| async move {
            token.cancelled().await;
            token.check()?;
            Ok(1)
        });
        assert_eq!(
            timed.await_timeout(Duration::from_millis(5)),
            Err(TaskError::Timeout)
        );

        let cancelled = spawn_task(|mut token| async move {
            token.cancelled().await;
            token.check()?;
            Ok(1)
        });
        assert!(cancelled.cancel());
        assert_eq!(cancelled.await_result(), Err(TaskError::Cancelled));
    }

    #[test]
    fn completed_failed_delay_and_status_ffi_are_observable() {
        let completed = ube_task_completed_value(42, 1);
        assert!(ube_task_is_completed(completed));
        assert!(!ube_task_was_cancelled(completed));
        assert_eq!(ube_task_await_value(completed), AbiValue { payload: 42, meta: 1 });

        let failed = ube_task_failed(crate::exceptions::generic("expected".into()) as *mut c_void);
        assert!(ube_task_is_completed(failed));
        assert_eq!(ube_task_await_value(failed), AbiValue::default());
        assert_eq!(crate::exceptions::message(crate::exceptions::take_pending()), "expected");

        let delayed = ube_task_delay(10_000);
        assert!(!ube_task_is_completed(delayed));
        assert!(ube_task_cancel(delayed));
        assert!(ube_task_was_cancelled(delayed));
        assert_eq!(ube_task_await_value(delayed), AbiValue::default());
        assert_eq!(crate::exceptions::message(crate::exceptions::take_pending()), "task was cancelled");

        for task in [completed, failed, delayed] { ube_task_dispose(task); }
    }

    #[test]
    fn channel_is_bounded_and_close_unblocks_receivers() {
        let channel = Channel::bounded(1).unwrap();
        let sender = channel.clone();
        let send_task = spawn_task(move |_| async move {
            sender.send(10).await?;
            sender.send(20).await?;
            Ok(())
        });
        let receive = |channel: &Channel<i32>| {
            block_on(async {
                tokio::time::timeout(Duration::from_secs(2), channel.receive())
                    .await
                    .expect("channel receive should not block indefinitely")
            })
            .unwrap()
        };
        assert_eq!(receive(&channel), Some(10));
        assert_eq!(receive(&channel), Some(20));
        send_task
            .await_timeout(Duration::from_secs(2))
            .expect("sender should complete after both receives");

        channel.close();
        assert_eq!(receive(&channel), None);
        assert_eq!(block_on(channel.send(30)), Err(TaskError::ChannelClosed));
    }

    #[test]
    fn channel_close_unblocks_a_sender_waiting_for_capacity() {
        let channel = Channel::bounded(1).unwrap();
        block_on(channel.send(1)).unwrap();
        let sender = channel.clone();
        let send_task = spawn_task(move |_| async move { sender.send(2).await });
        channel.close();
        assert_eq!(
            send_task.await_timeout(Duration::from_secs(2)),
            Err(TaskError::ChannelClosed)
        );
    }

    #[test]
    fn channel_close_wakes_blocked_senders_and_receivers() {
        let channel = Channel::bounded(1).unwrap();
        block_on(channel.send(1)).unwrap();
        let blocked_sender = channel.clone();
        let send_task = spawn_task(move |_| async move { blocked_sender.send(2).await });
        block_on(async { tokio::time::sleep(Duration::from_millis(5)).await });
        channel.close();
        assert_eq!(send_task.await_result(), Err(TaskError::ChannelClosed));

        let receiver_channel = Channel::<i32>::new();
        let blocked_receiver = receiver_channel.clone();
        let receive_task = spawn_task(move |_| async move { blocked_receiver.receive().await });
        block_on(async { tokio::time::sleep(Duration::from_millis(5)).await });
        receiver_channel.close();
        assert_eq!(receive_task.await_result(), Ok(None));
    }

    extern "C" fn add_one_from_context(context: *mut c_void) -> i64 {
        ube_task_context_get(context, 0) + 1
    }

    #[test]
    fn compiler_ffi_spawns_a_captured_callback() {
        let context = ube_task_context_create(1);
        ube_task_context_set(context, 0, 41);
        let task = ube_task_spawn(Some(add_one_from_context), context);
        assert_eq!(ube_task_await(task), 42);
        ube_task_dispose(task);
    }

    #[test]
    fn ffi_timeout_wraps_any_task() {
        let channel = ube_channel_create_capacity(1);
        let receive = ube_channel_recv_async(channel);
        let timed = ube_task_timeout(receive, 10);
        assert_eq!(ube_task_await(timed), 0);
        assert!(!ube_task_last_error().is_null());
        assert!(ube_task_propagate_error());
        assert!(ube_task_last_error().is_null());

        let quick = ube_task_timeout(ube_task_create(5), 1000);
        assert_eq!(ube_task_await(quick), 5);
        assert!(ube_task_last_error().is_null());
        assert!(!ube_task_propagate_error());
        ube_channel_close(channel);
    }
}

#[cfg(test)]
mod tagged_transport_tests {
    use super::*;

    extern "C" fn echo(context: *mut c_void) -> AbiValue {
        ube_task_context_get_value(context, 0)
    }

    #[test]
    fn tagged_ffi_preserves_presence_and_all_payload_bits() {
        assert_eq!(std::mem::size_of::<AbiValue>(), 16);
        // Includes signed limits, negative zero, infinity, and a NaN payload.
        for payload in [0, 1, -1, i64::MIN, i64::MAX, 0x7ff0000000000000, 0x7ff8000000000042] {
            for meta in [0, 1] {
                let expected = AbiValue { payload, meta };
                let context = ube_task_context_create(1);
                ube_task_context_set_value(context, 0, payload, meta);
                let task = ube_task_spawn_value(Some(echo), context);
                assert_eq!(ube_task_await_value(task), expected);
                assert_eq!(ube_task_await_value(task), expected);
                assert_eq!(crate::exceptions::take_pending(), 0);
                ube_task_dispose(task);
                let channel = ube_channel_create_capacity(1);
                let send = ube_channel_send_value_async(channel, payload, meta);
                assert_eq!(ube_task_await_value(send), AbiValue { payload: 1, meta: 1 | (4 << 8) });
                let receive = ube_channel_recv_async(channel);
                assert_eq!(ube_task_await_value(receive), expected);
                assert_eq!(crate::exceptions::take_pending(), 0);
                ube_task_dispose(send);
                ube_task_dispose(receive);
                ube_channel_dispose(channel);
            }
        }
    }

    #[test]
    fn shared_nullable_results_reach_multiple_waiters() {
        let task = spawn_task(|_| async { Ok(AbiValue::default()) });
        let other = task.clone();
        let waiter = std::thread::spawn(move || other.await_result().unwrap());
        assert_eq!(task.await_result().unwrap(), AbiValue::default());
        assert_eq!(waiter.join().unwrap(), AbiValue::default());
    }
}
