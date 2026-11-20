//! Trusted desktop coordination only. No externally callable agent tools.
use crate::{
    backup_restore::{
        prepare_restore_with_cancel, BackupProgress, PreparedRestore, RestoreGuard, RestoreSummary,
    },
    Service,
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, TryLockError,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
type Result<T> = std::result::Result<T, String>;
const TTL: Duration = Duration::from_secs(600);
const CANCEL: &str = "backup_job_cancelled";
#[derive(Clone, Serialize)]
pub struct BackupResult {
    pub output_path: Option<PathBuf>,
    pub recovery_path: Option<PathBuf>,
    pub summary: Option<RestoreSummary>,
}
#[derive(Clone, Serialize)]
pub struct BackupJob {
    pub id: String,
    pub kind: String,
    pub source_name: Option<String>,
    pub status: String,
    pub progress: BackupProgress,
    pub cancel_requested: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub expires_at: Option<u64>,
    pub preview: Option<RestoreSummary>,
    pub confirmation_token: Option<String>,
    pub result: Option<BackupResult>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}
struct Ready {
    data: PreparedRestore,
    guard: RestoreGuard,
    deadline: Instant,
}
enum Work {
    Export(PathBuf),
    Prepare(PathBuf),
    Commit(Ready,bool),
}
struct State {
    job: Option<BackupJob>,
    cancel: Arc<AtomicBool>,
    ready: Option<Ready>,
    work: Option<Work>,
    stop: bool,
}
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}
pub struct BackupJobManager {
    shared: Arc<Shared>,
    handle: Mutex<Option<JoinHandle<()>>>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, State> {
    shared.state.lock().unwrap_or_else(|p| p.into_inner())
}
fn pending(j: &BackupJob) -> bool {
    matches!(j.status.as_str(), "running" | "prepared" | "committing")
}
fn expire(s: &mut State) {
    if s.ready
        .as_ref()
        .is_some_and(|r| Instant::now() >= r.deadline)
    {
        s.ready = None;
        if let Some(j) = &mut s.job {
            j.status = "expired".into();
            j.confirmation_token = None;
            j.updated_at = now();
        }
    }
}
fn classify_error(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if message.starts_with("restore_conflict") {
        "restore_conflict"
    } else if lower.contains("sqlite_full")
        || lower.contains("disk is full")
        || lower.contains("no space left")
        || lower.contains("enospc")
    {
        "disk_full"
    } else if message.contains("超限")
        || message.contains("限512MiB")
        || message.contains("上限512MiB")
        || message.contains("限64MiB")
    {
        "limit_exceeded"
    } else if message.contains("非空WAL") {
        "not_standalone"
    } else if lower.contains("schema")
        || lower.contains("file is not a database")
        || lower.contains("database disk image is malformed")
        || message.contains("虚表")
        || message.contains("备份格式")
    {
        "unsupported_backup"
    } else {
        "operation_failed"
    }
}
fn finish(
    shared: &Shared,
    id: &str,
    status: &str,
    result: Option<BackupResult>,
    code: Option<&str>,
) {
    let mut s = lock(shared);
    if let Some(j) = s.job.as_mut().filter(|j| j.id == id) {
        j.status = status.into();
        j.result = result;
        j.confirmation_token = None;
        j.updated_at = now();
        j.error_code = code.map(str::to_owned);
        j.error_message = code.map(|c| {
            match c {
                "restore_conflict" => "书库已变化，请重新预览并确认",
                "limit_exceeded" => "备份超过限制：SQLite文件或正文总量512MiB，JSON文件64MiB，元数据32MiB；单条内容与记录数量也有限制",
                "not_standalone" => "备份含未合并的WAL，请使用应用导出的独立SQLite备份文件",
                "unsupported_backup" => "备份格式或结构不兼容，请选择本应用支持的备份",
                "disk_full" => "磁盘空间不足，请释放空间或选择其他保存位置",
                _ => "备份操作未完成，请检查文件及磁盘空间后重试",
            }.into()
        });
    }
}
fn service<'a>(
    store: &'a Mutex<Service>,
    cancel: &AtomicBool,
) -> Result<std::sync::MutexGuard<'a, Service>> {
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(CANCEL.into());
        }
        match store.try_lock() {
            Ok(s) => return Ok(s),
            Err(TryLockError::Poisoned(_)) => return Err("service_lock_poisoned".into()),
            Err(TryLockError::WouldBlock) => thread::sleep(Duration::from_millis(10)),
        }
    }
}
fn progress(shared: &Shared, id: &str, cancel: &AtomicBool, p: BackupProgress) -> Result<()> {
    if cancel.load(Ordering::SeqCst) {
        return Err(CANCEL.into());
    }
    let mut s = lock(shared);
    if let Some(j) = s.job.as_mut().filter(|j| j.id == id) {
        if p.phase == "commit" || p.phase == "committing" {
            j.status = "committing".into();
        }
        j.progress = p;
        j.updated_at = now();
    }
    Ok(())
}
fn worker(store: Arc<Mutex<Service>>, shared: &Shared, recovery: PathBuf) {
    loop {
        let (id, work, cancel) = {
            let mut s = lock(shared);
            loop {
                expire(&mut s);
                if s.stop {
                    return;
                }
                if let Some(work) = s.work.take() {
                    break (s.job.as_ref().unwrap().id.clone(), work, s.cancel.clone());
                }
                if let Some(r) = &s.ready {
                    let duration = r.deadline.saturating_duration_since(Instant::now());
                    s = shared
                        .wake
                        .wait_timeout(s, duration)
                        .unwrap_or_else(|p| p.into_inner())
                        .0;
                } else {
                    s = shared.wake.wait(s).unwrap_or_else(|p| p.into_inner());
                }
            }
        };
        let result: Result<Option<BackupResult>> = (|| match work {
            Work::Export(path) => {
                let s = service(&store, &cancel)?;
                let output = s.export_sqlite_backup_with_cancel(&path, cancel.clone(), |p| {
                    progress(shared, &id, &cancel, p)
                })?;
                Ok(Some(BackupResult {
                    output_path: Some(output),
                    recovery_path: None,
                    summary: None,
                }))
            }
            Work::Prepare(path) => {
                let prepared = prepare_restore_with_cancel(&path, cancel.clone(), |p| {
                    progress(shared, &id, &cancel, p)
                })?;
                let guard = service(&store, &cancel)?.restore_guard()?;
                let mut state = lock(shared);
                if cancel.load(Ordering::SeqCst) || state.stop {
                    return Err(CANCEL.into());
                }
                let summary = prepared.summary().clone();
                state.ready = Some(Ready {
                    data: prepared,
                    guard,
                    deadline: Instant::now() + TTL,
                });
                if let Some(j) = state.job.as_mut() {
                    j.status = "prepared".into();
                    j.progress = BackupProgress {
                        phase: "awaiting_confirmation".into(),
                        completed: 0,
                        total: None,
                    };
                    j.preview = Some(summary);
                    j.confirmation_token = Some(uuid::Uuid::new_v4().to_string());
                    j.expires_at = Some(now() + TTL.as_secs());
                    j.updated_at = now();
                }
                Ok(None)
            }
            Work::Commit(ready,writing_ack) => {
                let mut s = service(&store, &cancel)?;
                if s.restore_guard()? != ready.guard {
                    return Err("restore_conflict".into());
                }
                std::fs::create_dir_all(&recovery).map_err(|_| "recovery_directory_error")?;
                let path = recovery.join(format!("before-restore-{}.sqlite", uuid::Uuid::new_v4()));
                let outcome = s.commit_prepared_restore_with_writing_ack(
                    ready.data,
                    &ready.guard,
                    &path,
                    cancel.clone(),
                    writing_ack,
                    |p| progress(shared, &id, &cancel, p),
                )?;
                Ok(Some(BackupResult {
                    output_path: None,
                    recovery_path: Some(outcome.recovery_path),
                    summary: Some(outcome.summary),
                }))
            }
        })();
        match result {
            Ok(Some(out)) => finish(shared, &id, "done", Some(out), None),
            Ok(None) => {}
            Err(e) if e == CANCEL => finish(shared, &id, "cancelled", None, None),
            Err(e) => finish(shared, &id, "failed", None, Some(classify_error(&e))),
        }
    }
}
impl BackupJobManager {
    pub fn new(store: Arc<Mutex<Service>>, recovery_dir: PathBuf) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                job: None,
                cancel: Arc::new(AtomicBool::new(false)),
                ready: None,
                work: None,
                stop: false,
            }),
            wake: Condvar::new(),
        });
        let other = shared.clone();
        let handle = thread::Builder::new()
            .name("mojuan-backup".into())
            .spawn(move || {
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker(store, &other, recovery_dir)
                }))
                .is_err()
                {
                    let mut s = lock(&other);
                    s.stop = true;
                    s.ready = None;
                    s.work = None;
                    if let Some(j) = &mut s.job {
                        j.status = "failed".into();
                        j.confirmation_token = None;
                        j.error_code = Some("worker_panicked".into());
                        j.error_message = Some("后台备份已停止，请重开应用后重试".into());
                    }
                }
            });
        let handle = match handle {
            Ok(h) => Some(h),
            Err(_) => {
                lock(&shared).stop = true;
                None
            }
        };
        Self {
            shared,
            handle: Mutex::new(handle),
        }
    }
    fn start(&self, kind: &str, work: Work) -> Result<BackupJob> {
        let mut s = lock(&self.shared);
        expire(&mut s);
        if s.stop {
            return Err("后台备份已停止".into());
        }
        if s.job.as_ref().is_some_and(pending) {
            return Err("已有备份或待确认恢复，请先完成或取消".into());
        }
        let source_name = match &work {
            Work::Prepare(p) => p.file_name().map(|s| s.to_string_lossy().into_owned()),
            _ => None,
        };
        let j = BackupJob {
            source_name,
            id: uuid::Uuid::new_v4().to_string(),
            kind: kind.into(),
            status: "running".into(),
            progress: BackupProgress {
                phase: "waiting".into(),
                completed: 0,
                total: None,
            },
            cancel_requested: false,
            created_at: now(),
            updated_at: now(),
            expires_at: None,
            preview: None,
            confirmation_token: None,
            result: None,
            error_code: None,
            error_message: None,
        };
        s.cancel = Arc::new(AtomicBool::new(false));
        s.job = Some(j.clone());
        s.work = Some(work);
        self.shared.wake.notify_all();
        Ok(j)
    }
    pub fn start_export(&self, path: PathBuf) -> Result<BackupJob> {
        self.start("export_sqlite", Work::Export(path))
    }
    pub fn start_prepare_restore(&self, path: PathBuf) -> Result<BackupJob> {
        self.start("restore", Work::Prepare(path))
    }
    pub fn current(&self) -> Result<Option<BackupJob>> {
        let mut state = lock(&self.shared);
        expire(&mut state);
        Ok(state.job.clone())
    }
    pub fn status(&self, id: &str) -> Result<BackupJob> {
        let mut s = lock(&self.shared);
        expire(&mut s);
        s.job
            .as_ref()
            .filter(|j| j.id == id)
            .cloned()
            .ok_or_else(|| "备份任务不存在".into())
    }
    pub fn cancel(&self, id: &str) -> Result<BackupJob> {
        let mut s = lock(&self.shared);
        expire(&mut s);
        let j = s
            .job
            .as_ref()
            .filter(|j| j.id == id)
            .ok_or("备份任务不存在")?;
        if !pending(j) {
            return Ok(j.clone());
        }
        s.cancel.store(true, Ordering::SeqCst);
        s.ready = None;
        let j = s.job.as_mut().unwrap();
        j.cancel_requested = true;
        j.updated_at = now();
        if j.status == "prepared" {
            j.status = "cancelled".into();
            j.confirmation_token = None;
        }
        let out = j.clone();
        self.shared.wake.notify_all();
        Ok(out)
    }
    pub fn confirm_restore(&self,id:&str,token:&str)->Result<BackupJob>{self.confirm_restore_with_writing_ack(id,token,false)}
    pub fn confirm_restore_with_writing_ack(&self, id: &str, token: &str,writing_ack:bool) -> Result<BackupJob> {
        let mut s = lock(&self.shared);
        expire(&mut s);
        if s.stop {
            return Err("后台备份已停止".into());
        }
        let j = s
            .job
            .as_ref()
            .filter(|j| j.id == id)
            .ok_or("备份任务不存在")?;
        if j.status != "prepared" || j.confirmation_token.as_deref() != Some(token) {
            return Err("恢复确认已失效，请重新预览".into());
        }
        let ready = s.ready.take().ok_or("恢复预览已失效")?;
        s.work = Some(Work::Commit(ready,writing_ack));
        let j = s.job.as_mut().unwrap();
        j.status = "running".into();
        j.progress = BackupProgress {
            phase: "waiting".into(),
            completed: 0,
            total: None,
        };
        j.confirmation_token = None;
        j.updated_at = now();
        let out = j.clone();
        self.shared.wake.notify_all();
        Ok(out)
    }
    pub fn has_pending(&self) -> bool {
        let mut s = lock(&self.shared);
        expire(&mut s);
        s.job.as_ref().is_some_and(pending)
    }
    pub fn shutdown(&self) {
        {
            let mut s = lock(&self.shared);
            s.stop = true;
            s.cancel.store(true, Ordering::SeqCst);
            s.ready = None;
            if s.work.take().is_some() || s.job.as_ref().is_some_and(|j| j.status == "prepared") {
                if let Some(j) = &mut s.job {
                    j.status = "cancelled".into();
                    j.cancel_requested = true;
                    j.confirmation_token = None;
                }
            }
            self.shared.wake.notify_all();
        }
        if let Some(h) = self.handle.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = h.join();
        }
    }
}
impl Drop for BackupJobManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_backup_errors_have_safe_actionable_categories() {
        for message in [
            "SQLite备份限512MiB，或输入不是普通文件",
            "JSON备份限64MiB",
            "备份canonical文本512MiB或metadata32MiB超限",
        ] {
            assert_eq!(classify_error(message), "limit_exceeded");
        }
        assert_eq!(
            classify_error("仅支持独立SQLite文件：发现非空WAL"),
            "not_standalone"
        );
        assert_eq!(
            classify_error("备份表列不匹配当前schema"),
            "unsupported_backup"
        );
        assert_eq!(classify_error("database or disk is full"), "disk_full");
        assert_eq!(
            classify_error("No space left on device (os error 28)"),
            "disk_full"
        );
        assert_eq!(
            classify_error("restore_conflict: changed"),
            "restore_conflict"
        );
        assert_eq!(
            classify_error("private fixture text /private/path"),
            "operation_failed"
        );
    }
    #[test]
    fn prepared_expiry_drops_private_candidate_and_invalidates_confirmation() {
        let dir = std::env::temp_dir().join(format!("manager-expiry-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let s = Service::open(":memory:").unwrap();
        s.export_sqlite_backup(&dir.join("source.sqlite"), |_| Ok(()))
            .unwrap();
        let manager = BackupJobManager::new(Arc::new(Mutex::new(s)), dir.join("recovery"));
        let job = manager
            .start_prepare_restore(dir.join("source.sqlite"))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if manager.status(&job.id).unwrap().status == "prepared" {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
        let token = manager.status(&job.id).unwrap().confirmation_token.unwrap();
        // Exercise production expiry logic without a ten-minute wall-clock test.
        {
            let mut state = lock(&manager.shared);
            state.ready.as_mut().unwrap().deadline = Instant::now();
        }
        let expired = manager.status(&job.id).unwrap();
        assert_eq!(expired.status, "expired");
        assert!(expired.confirmation_token.is_none());
        assert!(lock(&manager.shared).ready.is_none());
        assert!(manager.confirm_restore(&job.id, &token).is_err());
        assert!(!manager.has_pending());
        manager.shutdown();
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn success_publication_wins_over_late_cancel_request() {
        let shared = Shared {
            state: Mutex::new(State {
                job: Some(BackupJob {
                    id: "fixture".into(),
                    kind: "restore".into(),
                    source_name: None,
                    status: "committing".into(),
                    progress: BackupProgress {
                        phase: "commit".into(),
                        completed: 0,
                        total: Some(1),
                    },
                    cancel_requested: true,
                    created_at: 0,
                    updated_at: 0,
                    expires_at: None,
                    preview: None,
                    confirmation_token: None,
                    result: None,
                    error_code: None,
                    error_message: None,
                }),
                cancel: Arc::new(AtomicBool::new(true)),
                ready: None,
                work: None,
                stop: false,
            }),
            wake: Condvar::new(),
        };
        finish(
            &shared,
            "fixture",
            "done",
            Some(BackupResult {
                output_path: None,
                recovery_path: Some("fixture.sqlite".into()),
                summary: None,
            }),
            None,
        );
        let state = lock(&shared);
        let job = state.job.as_ref().unwrap();
        assert_eq!(job.status, "done");
        assert!(job.cancel_requested);
        assert!(job.result.is_some());
        assert!(!pending(job));
    }
}
