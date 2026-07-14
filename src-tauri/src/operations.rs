use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

pub const OPERATION_PROGRESS_EVENT: &str = "acumod://operation-progress";

const PROGRESS_EVENT_INTERVAL: Duration = Duration::from_millis(100);
static NEXT_OPERATION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationProgress {
    pub operation_id: String,
    pub kind: String,
    pub title: String,
    pub phase: String,
    pub completed: usize,
    pub total: Option<usize>,
    pub current_item: Option<String>,
    pub elapsed_millis: u128,
    pub terminal: bool,
}

#[derive(Clone, Default)]
pub struct OperationCoordinator {
    active_operation: Arc<Mutex<Option<ActiveOperation>>>,
}

#[derive(Clone)]
struct ActiveOperation {
    id: String,
    title: String,
}

struct OperationLease {
    active_operation: Arc<Mutex<Option<ActiveOperation>>>,
    operation_id: String,
    reporter: OperationReporter,
}

impl OperationCoordinator {
    fn begin(
        &self,
        app: AppHandle,
        kind: impl Into<String>,
        title: impl Into<String>,
    ) -> Result<OperationLease, String> {
        let operation_id = format!(
            "operation-{}",
            NEXT_OPERATION_ID.fetch_add(1, Ordering::Relaxed)
        );
        let title = title.into();
        let mut active_operation = self
            .active_operation
            .lock()
            .map_err(|_| "后台任务状态不可用，请重启 Acumod 后重试。".to_string())?;

        if let Some(active) = active_operation.as_ref() {
            return Err(format!("正在执行“{}”，请等待当前任务完成。", active.title));
        }

        *active_operation = Some(ActiveOperation {
            id: operation_id.clone(),
            title: title.clone(),
        });

        Ok(OperationLease {
            active_operation: Arc::clone(&self.active_operation),
            operation_id: operation_id.clone(),
            reporter: OperationReporter::new(app, operation_id, kind.into(), title),
        })
    }
}

impl Drop for OperationLease {
    fn drop(&mut self) {
        if let Ok(mut active_operation) = self.active_operation.lock() {
            if active_operation
                .as_ref()
                .is_some_and(|active| active.id == self.operation_id)
            {
                *active_operation = None;
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct OperationReporter {
    inner: Option<Arc<OperationReporterInner>>,
}

struct OperationReporterInner {
    app: AppHandle,
    operation_id: String,
    kind: String,
    title: String,
    started_at: Instant,
    last_event: Mutex<LastEvent>,
}

struct LastEvent {
    phase: String,
    emitted_at: Instant,
}

impl OperationReporter {
    fn new(app: AppHandle, operation_id: String, kind: String, title: String) -> Self {
        Self {
            inner: Some(Arc::new(OperationReporterInner {
                app,
                operation_id,
                kind,
                title,
                started_at: Instant::now(),
                last_event: Mutex::new(LastEvent {
                    phase: String::new(),
                    emitted_at: Instant::now() - PROGRESS_EVENT_INTERVAL,
                }),
            })),
        }
    }

    pub fn report(
        &self,
        phase: impl Into<String>,
        completed: usize,
        total: Option<usize>,
        current_item: Option<String>,
    ) {
        let Some(inner) = self.inner.as_ref() else {
            return;
        };
        let phase = phase.into();
        let is_complete = total.is_some_and(|count| completed >= count);
        let should_emit = inner
            .last_event
            .lock()
            .map(|mut last_event| {
                let phase_changed = last_event.phase != phase;
                if !phase_changed
                    && !is_complete
                    && last_event.emitted_at.elapsed() < PROGRESS_EVENT_INTERVAL
                {
                    return false;
                }
                last_event.phase = phase.clone();
                last_event.emitted_at = Instant::now();
                true
            })
            .unwrap_or(false);

        if should_emit {
            self.emit(OperationProgress {
                operation_id: inner.operation_id.clone(),
                kind: inner.kind.clone(),
                title: inner.title.clone(),
                phase,
                completed,
                total,
                current_item,
                elapsed_millis: inner.started_at.elapsed().as_millis(),
                terminal: false,
            });
        }
    }

    fn finish(&self, success: bool, message: String) {
        let Some(inner) = self.inner.as_ref() else {
            return;
        };
        let elapsed_millis = inner.started_at.elapsed().as_millis();
        self.emit(OperationProgress {
            operation_id: inner.operation_id.clone(),
            kind: inner.kind.clone(),
            title: inner.title.clone(),
            phase: if success { "已完成" } else { "失败" }.to_string(),
            completed: 0,
            total: None,
            current_item: Some(message.clone()),
            elapsed_millis,
            terminal: true,
        });
        append_timing_log(&inner.kind, &inner.title, success, elapsed_millis, &message);
    }

    fn emit(&self, progress: OperationProgress) {
        let Some(inner) = self.inner.as_ref() else {
            return;
        };
        let _ = inner.app.emit(OPERATION_PROGRESS_EVENT, progress);
    }
}

pub async fn run_blocking_operation<T, F>(
    app: AppHandle,
    kind: &'static str,
    title: &'static str,
    operation: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(OperationReporter) -> Result<T, String> + Send + 'static,
{
    let coordinator = app.state::<OperationCoordinator>().inner().clone();
    let lease = coordinator.begin(app, kind, title)?;
    let reporter = lease.reporter.clone();
    reporter.report("准备中", 0, None, None);
    let worker_reporter = reporter.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        let result = operation(worker_reporter.clone());
        match &result {
            Ok(_) => worker_reporter.finish(true, "任务完成。".to_string()),
            Err(error) => worker_reporter.finish(false, error.clone()),
        }
        result
    })
    .await
    .map_err(|error| {
        let message = format!("后台任务意外结束：{error}");
        reporter.finish(false, message.clone());
        message
    })?
}

fn append_timing_log(kind: &str, title: &str, success: bool, elapsed_millis: u128, message: &str) {
    let Ok(executable_path) = env::current_exe() else {
        return;
    };
    let Some(executable_directory) = executable_path.parent() else {
        return;
    };
    let log_directory = executable_directory.join("AcumodData").join("logs");
    if fs::create_dir_all(&log_directory).is_err() {
        return;
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let status = if success { "完成" } else { "失败" };
    let sanitized_message = message.replace(['\r', '\n'], " ");
    let line = format!(
        "{timestamp}\t{kind}\t{title}\t{status}\t{elapsed_millis}ms\t{sanitized_message}\n"
    );
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_directory.join("operation-timings.log"))
    {
        let _ = file.write_all(line.as_bytes());
    }
}
