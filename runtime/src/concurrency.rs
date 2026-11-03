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

fn abi_task_handles() -> &'static StdMutex<Vec<usize>> {
    static HANDLES: OnceLock<StdMutex<Vec<usize>>> = OnceLock::new();
    HANDLES.get_or_init(|| StdMutex::new(Vec::new()))
}

fn abi_channel_handles() -> &'static StdMutex<Vec<usize>> {
    static HANDLES: OnceLock<StdMutex<Vec<usize>>> = OnceLock::new();
    HANDLES.get_or_init(|| StdMutex::new(Vec::new()))
}

fn track_abi_task(task: Task<i64>) -> *mut c_void {
    let ptr = Box::into_raw(Box::new(task));
    abi_task_handles()
        .lock()
        .expect("Ube task handle registry was poisoned")
        .push(ptr as usize);
    ptr as *mut c_void
}

fn legacy_task_value(result: Result<i64, TaskError>) -> i64 {
    match result {
        Ok(value) => {
            crate::exceptions::set_pending(0);
            LAST_TASK_ERROR.with(|last_error| *last_error.borrow_mut() = None);
            value
        }
        Err(error) => {
            set_last_task_error(error.to_string());
            let record = match error {
                TaskError::Exception(record) => record,
                other => crate::exceptions::generic(other.to_string()),
            };
            crate::exceptions::set_pending(record);
            0
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

#[derive(Clone)]
pub struct CancellationToken {
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
}

impl<T> Clone for Task<T> {
    fn clone(&self) -> Self {
        Self {
            result: self.result.clone(),
            cancellation: self.cancellation.clone(),
            cancellation_requested: self.cancellation_requested.clone(),
            cancelled_flag: self.cancelled_flag.clone(),
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
    let token = CancellationToken {
        cancelled: cancellation_rx.clone(),
        cancelled_flag: cancelled_flag.clone(),
    };
    let cancellation_requested_by_worker = cancellation_requested.clone();
    let cancelled_flag_by_worker = cancelled_flag.clone();

    runtime().spawn(async move {
        let work = async move { work(token).await };
        let outcome = if cooperative { AssertUnwindSafe(work).catch_unwind().await.unwrap_or_else(|p| Err(TaskError::Panicked(panic_message(p)))) } else { tokio::select! {
            biased;
            _ = wait_for_cancellation(cancellation_rx) => Err(TaskError::Cancelled),
            result = AssertUnwindSafe(work).catch_unwind() => {
                result.unwrap_or_else(|payload| Err(TaskError::Panicked(panic_message(payload))))
            }
        }
        };
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
    }
}

async fn wait_for_cancellation(mut cancellation: watch::Receiver<bool>) {
    while !*cancellation.borrow() && cancellation.changed().await.is_ok() {}
}

impl<T> Task<T>
where
    T: Clone + Send + Sync + 'static,
{
    async fn wait_result(&self) -> Result<T, TaskError> {
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
        self.cancellation.send(true).is_ok()
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
    channel: Channel<i64>,
}

#[repr(C)]
struct UbeTaskContext {
    values: Vec<i64>,
}

#[no_mangle]
pub extern "C" fn ube_task_context_create(size: i64) -> *mut c_void {
    let capacity = usize::try_from(size).unwrap_or(0);
    Box::into_raw(Box::new(UbeTaskContext {
        values: vec![0; capacity],
    })) as *mut c_void
}

#[no_mangle]
pub extern "C" fn ube_task_context_set(ptr: *mut c_void, index: i64, value: i64) {
    if ptr.is_null() || index < 0 {
        return;
    }
    let context = unsafe { &mut *(ptr as *mut UbeTaskContext) };
    if let Some(slot) = context.values.get_mut(index as usize) {
        *slot = value;
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
            .unwrap_or(0)
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
    let context = if context.is_null() {
        Box::new(UbeTaskContext { values: Vec::new() })
    } else {
        unsafe { Box::from_raw(context as *mut UbeTaskContext) }
    };
    let task = spawn_task_impl(move |token| async move {
        let flag = token.flag();
        let value = tokio::task::spawn_blocking(move || {
            CURRENT_TASK_ERROR.with(|error| *error.borrow_mut() = None);
            crate::exceptions::set_pending(0);
            crate::exceptions::reset_cancellation();
            CURRENT_CANCELLATION.with(|current| *current.borrow_mut() = Some(flag));
            let value = callback((&*context as *const UbeTaskContext) as *mut c_void);
            let error = CURRENT_TASK_ERROR.with(|error| error.borrow_mut().take());
            CURRENT_CANCELLATION.with(|current| *current.borrow_mut() = None);
            let pending = crate::exceptions::take_pending();
            if pending != 0 { Err(TaskError::Exception(pending)) }
            else { error.map_or(Ok(value), |message| Err(TaskError::Failed(message))) }
        })
        .await
        .map_err(|error| TaskError::Panicked(error.to_string()))?;
        value
    }, true);
    track_abi_task(task)
}

#[no_mangle]
pub extern "C" fn ube_task_create(val: i64) -> *mut c_void {
    track_abi_task(spawn_task(move |_| async move { Ok(val) }))
}

#[no_mangle]
pub extern "C" fn ube_task_await(ptr: *mut c_void) -> i64 {
    if ptr.is_null() {
        set_last_task_error("task handle is null".into());
        return 0;
    }
    let task = unsafe { &*(ptr as *const Task<i64>) };
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
                    Err(TaskError::Cancelled)
                }
            }
        })
    } else { task.await_result() };
    legacy_task_value(result)
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
    unsafe { (&*(ptr as *const Task<i64>)).cancel() }
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
    let inner = unsafe { (&*(ptr as *const Task<i64>)).clone() };
    let duration = Duration::from_millis(timeout_ms as u64);
    track_abi_task(spawn_task(move |_| async move {
        struct CancelOnDrop(Task<i64>);
        impl Drop for CancelOnDrop {
            fn drop(&mut self) {
                self.0.cancel();
            }
        }
        let guard = CancelOnDrop(inner);
        match tokio::time::timeout(duration, guard.0.wait_result()).await {
            Ok(result) => result,
            Err(_) => Err(TaskError::Timeout),
        }
    }))
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
    block_on(channel.send(val)).is_ok()
}

#[no_mangle]
pub extern "C" fn ube_channel_send_async(ptr: *mut UbeChannel, val: i64) -> *mut c_void {
    if ptr.is_null() {
        set_last_task_error("channel handle is null".into());
        return std::ptr::null_mut();
    }
    let channel = unsafe { (*ptr).channel.clone() };
    track_abi_task(spawn_task(move |_| async move {
        match channel.send(val).await {
            Ok(()) => Ok(1),
            Err(TaskError::ChannelClosed) => Ok(0),
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
            value
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

#[no_mangle]
pub extern "C" fn ube_task_all(list_ptr: *mut i8) -> *mut i8 {
    let task_list = list_ptr as *mut crate::collections::UbeList;
    let result_list = crate::collections::ube_list_create();
    if task_list.is_null() {
        return result_list as *mut i8;
    }
    let len = crate::collections::ube_list_len(task_list);
    for index in 0..len {
        let task_ptr = crate::collections::ube_list_get(task_list, index) as *mut Task<i64>;
        let value = if task_ptr.is_null() {
            0
        } else {
            legacy_task_value(unsafe { (&*task_ptr).await_result() })
        };
        crate::collections::ube_list_add(result_list, value);
    }
    result_list as *mut i8
}

#[no_mangle]
pub extern "C" fn ube_task_any(list_ptr: *mut i8) -> i64 {
    run_legacy_task_combinator(list_ptr, Task::<i64>::any)
}

#[no_mangle]
pub extern "C" fn ube_task_race(list_ptr: *mut i8) -> i64 {
    run_legacy_task_combinator(list_ptr, Task::<i64>::race)
}

fn run_legacy_task_combinator<F, Fut>(list_ptr: *mut i8, combinator: F) -> i64
where
    F: FnOnce(Vec<Task<i64>>) -> Fut,
    Fut: Future<Output = Result<i64, TaskError>>,
{
    let task_list = list_ptr as *mut crate::collections::UbeList;
    if task_list.is_null() {
        return 0;
    }
    let len = crate::collections::ube_list_len(task_list);
    let tasks = (0..len)
        .filter_map(|index| {
            let ptr = crate::collections::ube_list_get(task_list, index) as *mut Task<i64>;
            if ptr.is_null() {
                None
            } else {
                Some(unsafe { (&*ptr).clone() })
            }
        })
        .collect();
    legacy_task_value(block_on(combinator(tasks)))
}

fn clone_task_list(list_ptr: *mut i8) -> Vec<Task<i64>> {
    let task_list = list_ptr as *mut crate::collections::UbeList;
    if task_list.is_null() {
        return Vec::new();
    }
    let len = crate::collections::ube_list_len(task_list);
    (0..len)
        .filter_map(|index| {
            let ptr = crate::collections::ube_list_get(task_list, index) as *mut Task<i64>;
            if ptr.is_null() {
                None
            } else {
                Some(unsafe { (&*ptr).clone() })
            }
        })
        .collect()
}

#[no_mangle]
pub extern "C" fn ube_task_all_async(list_ptr: *mut i8) -> *mut c_void {
    let tasks = clone_task_list(list_ptr);
    let task = spawn_task(move |_| async move {
        let values = Task::all(tasks).await?;
        let list = crate::collections::ube_list_create();
        for value in values {
            crate::collections::ube_list_add(list, value);
        }
        Ok(list as usize as i64)
    });
    track_abi_task(task)
}

#[no_mangle]
pub extern "C" fn ube_task_any_async(list_ptr: *mut i8) -> *mut c_void {
    let tasks = clone_task_list(list_ptr);
    let task = spawn_task(move |_| async move { Task::any(tasks).await });
    track_abi_task(task)
}

#[no_mangle]
pub extern "C" fn ube_task_race_async(list_ptr: *mut i8) -> *mut c_void {
    let tasks = clone_task_list(list_ptr);
    let task = spawn_task(move |_| async move { Task::race(tasks).await });
    track_abi_task(task)
}

#[no_mangle]
pub extern "C" fn ube_task_all_settled_async(list_ptr: *mut i8) -> *mut c_void {
    let tasks = clone_task_list(list_ptr);
    let task = spawn_task(move |_| async move {
        let outcomes = Task::all_settled(tasks).await;
        let list = crate::collections::ube_list_create();
        for outcome in outcomes {
            crate::collections::ube_list_add(list, legacy_task_value(outcome));
        }
        Ok(list as usize as i64)
    });
    track_abi_task(task)
}

#[no_mangle]
pub extern "C" fn ube_task_all_settled(list_ptr: *mut i8) -> *mut i8 {
    let task_list = list_ptr as *mut crate::collections::UbeList;
    let result_list = crate::collections::ube_list_create();
    if task_list.is_null() {
        return result_list as *mut i8;
    }
    let len = crate::collections::ube_list_len(task_list);
    for index in 0..len {
        let task_ptr = crate::collections::ube_list_get(task_list, index) as *mut Task<i64>;
        let outcome = if task_ptr.is_null() {
            0
        } else {
            legacy_task_value(unsafe { (&*task_ptr).await_result() })
        };
        crate::collections::ube_list_add(result_list, outcome);
    }
    result_list as *mut i8
}

#[no_mangle]
pub extern "C" fn ube_task_dispose(ptr: *mut c_void) {
    if !ptr.is_null() {
        abi_task_handles()
            .lock()
            .expect("Ube task handle registry was poisoned")
            .retain(|handle| *handle != ptr as usize);
        unsafe {
            drop(Box::from_raw(ptr as *mut Task<i64>));
        }
    }
}

pub fn ube_concurrency_cleanup() {
    let handles = std::mem::take(
        &mut *abi_task_handles()
            .lock()
            .expect("Ube task handle registry was poisoned"),
    );
    for handle in &handles {
        unsafe {
            (&*(*handle as *const Task<i64>)).cancel();
        }
    }
    for handle in handles {
        unsafe {
            let task = Box::from_raw(handle as *mut Task<i64>);
            let _ = task.await_result();
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
                Ok(42_i64)
            });
            let handle = (&task as *const Task<i64>) as *mut c_void;
            assert_eq!(ube_task_await(handle), 42);
            assert_eq!(crate::exceptions::take_pending(), 0);

            let failed = spawn_task::<i64, _, _>(|_| async { Err(TaskError::Cancelled) });
            let handle = (&failed as *const Task<i64>) as *mut c_void;
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
        assert_eq!(block_on(Task::<i64>::all(Vec::new())).unwrap(), Vec::new());

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
