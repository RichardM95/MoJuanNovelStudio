//! Local-only scheduler. All durable cursors live in Service, never in this worker.
//! One step holds the shared writer mutex; idle and inter-step waits hold no Service lock.
use crate::Service;
use std::sync::{Arc, Condvar, Mutex, TryLockError};
use std::thread::{self, JoinHandle};
use std::time::Duration;
#[derive(Default)]
struct Signal {
    stop: bool,
    notified: bool,
    failure: Option<&'static str>,
}
struct Control {
    state: Mutex<Signal>,
    wake: Condvar,
}
pub struct TaskRunner {
    control: Arc<Control>,
    handle: Mutex<Option<JoinHandle<()>>>,
}
fn signal(control: &Control) -> std::sync::MutexGuard<'_, Signal> {
    // No user callbacks execute under this tiny lock; still recover it for safe shutdown.
    control.state.lock().unwrap_or_else(|p| p.into_inner())
}
fn fail(control: &Control, code: &'static str) {
    let mut s = signal(control);
    s.failure = Some(code);
    s.stop = true;
    control.wake.notify_all();
}
fn pause(control: &Control, millis: u64) -> bool {
    let s = signal(control);
    if s.stop {
        return false;
    }
    let (s, _) = control
        .wake
        .wait_timeout(s, Duration::from_millis(millis))
        .unwrap_or_else(|p| p.into_inner());
    !s.stop
}
fn run(store: Arc<Mutex<Service>>, control: &Control) {
    loop {
        let mut s = signal(control);
        while !s.stop && !s.notified {
            s = control.wake.wait(s).unwrap_or_else(|p| p.into_inner());
        }
        if s.stop {
            return;
        }
        s.notified = false;
        drop(s);
        loop {
            if signal(control).stop {
                return;
            }
            // Never block waiting for Service: shutdown must work even when a caller
            // still holds it. Contention waits on the stop-aware Condvar, not a spin.
            let result = match store.try_lock() {
                Ok(mut service) => service.step_next_task(),
                Err(TryLockError::WouldBlock) => {
                    if !pause(control, 10) {
                        return;
                    }
                    continue;
                }
                Err(TryLockError::Poisoned(_)) => {
                    fail(control, "service_lock_poisoned");
                    return;
                }
            };
            // The Service guard is gone before control/wait operations.
            match result {
                Ok(Some(_)) => {
                    if !pause(control, 1) {
                        return;
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    // Do not log the database error text or retry a broken transaction.
                    // Existing active rows become interrupted on the next Service open.
                    fail(control, "task_step_failed");
                    return;
                }
            }
        }
    }
}
impl TaskRunner {
    pub fn start(store: Arc<Mutex<Service>>) -> Self {
        let control = Arc::new(Control {
            state: Mutex::new(Signal {
                notified: true,
                ..Signal::default()
            }),
            wake: Condvar::new(),
        });
        let worker_control = control.clone();
        let handle = thread::Builder::new()
            .name("mojuan-context-check".into())
            .spawn(move || {
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(store, &worker_control)
                }))
                .is_err()
                {
                    fail(&worker_control, "worker_panicked");
                }
            });
        let handle = match handle {
            Ok(h) => Some(h),
            Err(_) => {
                fail(&control, "worker_start_failed");
                None
            }
        };
        Self {
            control,
            handle: Mutex::new(handle),
        }
    }
    pub fn notify(&self) {
        let mut s = signal(&self.control);
        if !s.stop {
            s.notified = true;
            self.control.wake.notify_one();
        }
    }
    pub fn failure_code(&self) -> Option<String> {
        signal(&self.control).failure.map(str::to_owned)
    }
    pub fn check_health(&self) -> Result<(), String> {
        if self.failure_code().is_some() {
            Err("后台检查已停止，请重开应用恢复".into())
        } else {
            Ok(())
        }
    }
    /// Idempotent; does not acquire Service. An in-flight bounded step may finish.
    pub fn shutdown(&self) {
        {
            let mut s = signal(&self.control);
            s.stop = true;
            self.control.wake.notify_all();
        }
        let mut handle = self.handle.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(h) = handle.take() {
            if h.join().is_err() {
                fail(&self.control, "worker_panicked");
            }
        }
    }
}
impl Drop for TaskRunner {
    fn drop(&mut self) {
        self.shutdown();
    }
}
