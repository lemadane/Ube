//! Heap-backed native continuations. A pending await returns its Tokio worker.
use super::*;
use std::{collections::HashMap, pin::Pin, task::Poll};

struct Frame {
    values: Vec<AbiValue>,
    roots: HashMap<usize, u64>,
    pc: u64,
    budget: usize,
    waiting: Option<Task<AbiValue>>,
    resumed: Option<Result<AbiValue, TaskError>>,
    result: AbiValue,
    cancellation_reported: bool,
    pending: usize,
}
impl Frame {
    fn set(&mut self, slot: usize, value: AbiValue, reference: bool) {
        self.values[slot] = value;
        // Publish the new root before releasing the previous value's root.
        let root = if reference && value.payload != 0 {
            crate::gc::ube_gc_root_add(value.payload as *mut c_void)
        } else {
            0
        };
        if let Some(old) = self.roots.remove(&slot) {
            crate::gc::ube_gc_root_remove(old);
        }
        if root != 0 {
            self.roots.insert(slot, root);
        }
    }
}
impl Drop for Frame {
    fn drop(&mut self) {
        for (_, root) in self.roots.drain() {
            crate::gc::ube_gc_root_remove(root);
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_poll_get(frame: *mut c_void, slot: u64) -> AbiValue {
    unsafe { &*(frame as *const Frame) }.values[slot as usize]
}
#[no_mangle]
pub extern "C" fn ube_poll_set(
    frame: *mut c_void,
    slot: u64,
    payload: i64,
    meta: u64,
    reference: bool,
) {
    unsafe { &mut *(frame as *mut Frame) }.set(
        slot as usize,
        AbiValue { payload, meta },
        reference,
    );
}
#[no_mangle]
pub extern "C" fn ube_poll_pc(frame: *mut c_void) -> u64 {
    unsafe { &*(frame as *const Frame) }.pc
}
#[no_mangle]
pub extern "C" fn ube_poll_step(frame: *mut c_void, pc: u64) -> bool {
    let frame = unsafe { &mut *(frame as *mut Frame) };
    frame.pc = pc;
    if frame.budget == 0 {
        return false;
    }
    frame.budget -= 1;
    true
}
#[no_mangle]
pub extern "C" fn ube_poll_finish(frame: *mut c_void, payload: i64, meta: u64) {
    unsafe { &mut *(frame as *mut Frame) }.result = AbiValue { payload, meta };
}
#[no_mangle]
pub extern "C" fn ube_poll_await(frame: *mut c_void, handle: *mut c_void) -> bool {
    let frame = unsafe { &mut *(frame as *mut Frame) };
    let result = if let Some(result) = frame.resumed.take() {
        result
    } else {
        let task = match lookup_task(handle) {
            Ok(task) => task,
            Err(error) => {
                frame.result = task_value(Err(error));
                return true;
            }
        };
        task.observed.store(true, Ordering::Release);
        let ready = task.result.borrow().clone();
        match ready {
            Some(result) => result,
            None => {
                frame.waiting = Some(task);
                return false;
            }
        }
    };
    if ube_task_is_cancelled() && result.is_err() {
        crate::exceptions::mark_cancellation_reported();
    }
    frame.result = task_value(result);
    true
}
#[no_mangle]
pub extern "C" fn ube_poll_value(frame: *mut c_void) -> AbiValue {
    unsafe { &*(frame as *const Frame) }.result
}

async fn dependency(
    task: Task<AbiValue>,
    mut token: CancellationToken,
    reported: bool,
) -> Result<AbiValue, TaskError> {
    if reported {
        return task.wait_result().await;
    }
    tokio::select! {
        biased;
        _ = token.cancelled() => {
            task.cancel();
            match task.wait_result().await {
                Err(error) if !is_cancel_error(&error) => Err(merge_cleanup_error(TaskError::Cancelled, error)),
                _ => Err(TaskError::Cancelled),
            }
        }
        result = task.wait_result() => result,
    }
}

#[no_mangle]
pub extern "C" fn ube_task_spawn_poll(
    callback: Option<extern "C" fn(*mut c_void) -> bool>,
    context: *mut c_void,
    slots: u64,
) -> *mut c_void {
    let captures = if context.is_null() {
        Box::new(UbeTaskContext { values: vec![] })
    } else {
        unsafe { Box::from_raw(context as *mut UbeTaskContext) }
    };
    let Some(callback) = callback else {
        return std::ptr::null_mut();
    };
    if slots > 1_000_000 || slots < (captures.values.len() * 2) as u64 {
        return track_abi_task(spawn_task(|_| async {
            Err(TaskError::Failed("invalid async frame size".into()))
        }));
    }
    let mut frame = Box::new(Frame {
        values: vec![AbiValue::default(); slots as usize],
        roots: HashMap::new(),
        pc: 0,
        budget: 0,
        waiting: None,
        resumed: None,
        result: AbiValue::default(),
        cancellation_reported: false,
        pending: 0,
    });
    for (index, value) in captures.values.iter().copied().enumerate() {
        frame.set(index * 2, value, value.meta >> 8 == 7);
    }
    let mut log_context = crate::logging::snapshot();
    let task = spawn_task_impl(
        move |token| async move {
            let mut waiting: Option<
                Pin<Box<dyn Future<Output = Result<AbiValue, TaskError>> + Send>>,
            > = None;
            std::future::poll_fn(move |cx| {
                frame.budget = 256;
                loop {
                    if let Some(wait) = waiting.as_mut() {
                        match wait.as_mut().poll(cx) {
                            Poll::Pending => return Poll::Pending,
                            Poll::Ready(result) => {
                                frame.resumed = Some(result);
                                waiting = None;
                            }
                        }
                    }
                    let (complete, pending) =
                        crate::logging::with_context(log_context.clone(), || {
                            let old_children = CURRENT_CHILDREN
                                .with(|current| current.replace(Some(token.children.clone())));
                            let old_cancel = CURRENT_CANCELLATION
                                .with(|current| current.replace(Some(token.flag())));
                            let old_last =
                                LAST_TASK_ERROR.with(|current| current.borrow_mut().take());
                            let old_pending = crate::exceptions::take_pending();
                            crate::exceptions::set_pending(frame.pending);
                            let old_reported = crate::exceptions::cancellation_reported();
                            if frame.cancellation_reported {
                                crate::exceptions::mark_cancellation_reported();
                            } else {
                                crate::exceptions::reset_cancellation();
                            }
                            let complete = callback((&mut *frame as *mut Frame).cast());
                            frame.cancellation_reported =
                                crate::exceptions::cancellation_reported();
                            log_context = crate::logging::snapshot();
                            let pending = crate::exceptions::take_pending();
                            frame.pending = pending;
                            crate::exceptions::set_pending(old_pending);
                            if old_reported {
                                crate::exceptions::mark_cancellation_reported();
                            } else {
                                crate::exceptions::reset_cancellation();
                            }
                            LAST_TASK_ERROR.with(|current| *current.borrow_mut() = old_last);
                            CURRENT_CANCELLATION.with(|current| *current.borrow_mut() = old_cancel);
                            CURRENT_CHILDREN.with(|current| *current.borrow_mut() = old_children);
                            (complete, pending)
                        });
                    if complete && pending != 0 {
                        return Poll::Ready(Err(TaskError::Exception(pending)));
                    }
                    if complete {
                        return Poll::Ready(Ok(frame.result));
                    }
                    if let Some(task) = frame.waiting.take() {
                        waiting = Some(Box::pin(dependency(
                            task,
                            token.clone(),
                            frame.cancellation_reported,
                        )));
                        // Poll now so the dependency registers this task's waker.
                    } else {
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                }
            })
            .await
        },
        true,
    );
    track_abi_task(task)
}

#[cfg(test)]
mod tests {
    use super::*;
    extern "C" fn pending_exception_across_yield(frame: *mut c_void) -> bool {
        if ube_poll_pc(frame) == 0 {
            crate::exceptions::set_pending(crate::exceptions::generic("saved error".into()));
            ube_poll_step(frame, 1);
            false
        } else {
            let record = crate::exceptions::take_pending();
            let value = if crate::exceptions::message(record) == "saved error" {
                99
            } else {
                -1
            };
            ube_poll_finish(frame, value, 1);
            true
        }
    }
    #[test]
    fn scheduler_yield_preserves_pending_exception_until_generated_handler_runs() {
        let task = ube_task_spawn_poll(
            Some(pending_exception_across_yield),
            std::ptr::null_mut(),
            0,
        );
        assert_eq!(ube_task_await(task), 99);
        assert_eq!(crate::exceptions::take_pending(), 0);
        ube_task_dispose(task);
    }
}
