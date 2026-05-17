use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::{ProjectTaskRecord, TaskKind, TaskRecord, TaskStatus};

#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter};

#[cfg(feature = "desktop")]
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

#[cfg(feature = "desktop")]
pub fn set_app_handle(handle: AppHandle) {
    let _ = APP_HANDLE.set(handle);
}

#[cfg(feature = "desktop")]
fn emit_task_update(record: &TaskRecord) {
    if let Some(handle) = APP_HANDLE.get() {
        let _ = handle.emit("task-update", record);
    }
}

fn notify_task_update(record: &TaskRecord) {
    #[cfg(feature = "desktop")]
    emit_task_update(record);
    #[cfg(not(feature = "desktop"))]
    let _ = record;
}

type TaskJob = Box<dyn FnOnce(&mut TaskContext) -> TaskOutcome + Send + 'static>;

pub const TASK_LOG_MAX_BYTES: usize = 64 * 1024;
pub const TASK_RECORD_MAX_RETAINED: usize = 100;
/// Default number of recent records returned when `limit` is not supplied.
/// Kept in sync with `TASKS_RECENT_LIMIT` in `src/App.tsx`.
pub const TASK_RECENT_DEFAULT_LIMIT: usize = 80;
/// Upper bound applied to caller-supplied limits to cap IPC payload size.
pub const TASK_RECENT_MAX_LIMIT: usize = 200;
const TASK_LOG_TRUNCATED_MARKER: &str = "[task log truncated; showing most recent output]\n";

struct QueuedTask {
    id: String,
    cancel_requested: Arc<AtomicBool>,
    job: TaskJob,
}

#[derive(Default)]
struct TaskQueueState {
    next_id: u64,
    records: Vec<TaskRecord>,
    cancel_flags: HashMap<String, Arc<AtomicBool>>,
    queued: VecDeque<QueuedTask>,
}

pub struct TaskQueue {
    state: Mutex<TaskQueueState>,
    runner: Mutex<()>,
    publish_progress: bool,
}

impl Default for TaskQueue {
    fn default() -> Self {
        Self {
            state: Mutex::new(TaskQueueState::default()),
            runner: Mutex::new(()),
            publish_progress: false,
        }
    }
}

pub struct TaskContext {
    task_id: Option<String>,
    cancel_requested: Arc<AtomicBool>,
    stdout: String,
    stderr: String,
}

impl TaskContext {
    pub fn is_cancelled(&self) -> bool {
        self.cancel_requested.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn request_cancel_for_tests(&self) {
        self.cancel_requested.store(true, Ordering::SeqCst);
    }

    pub fn stdout(&mut self, line: impl AsRef<str>) {
        append_bounded_log(&mut self.stdout, line.as_ref());
        if !self.stdout.ends_with('\n') {
            append_bounded_log(&mut self.stdout, "\n");
        }
        if let Some(task_id) = self.task_id.as_deref() {
            task_queue().append_task_stdout(task_id, line.as_ref());
        }
    }

    pub fn stderr(&mut self, line: impl AsRef<str>) {
        append_bounded_log(&mut self.stderr, line.as_ref());
        if !self.stderr.ends_with('\n') {
            append_bounded_log(&mut self.stderr, "\n");
        }
        if let Some(task_id) = self.task_id.as_deref() {
            task_queue().append_task_stderr(task_id, line.as_ref());
        }
    }

    pub fn set_summary(&mut self, summary: impl AsRef<str>) {
        if let Some(task_id) = self.task_id.as_deref() {
            task_queue().update_task_summary(task_id, summary.as_ref());
        }
    }
}

pub struct TaskOutcome {
    status: TaskStatus,
    summary: String,
    error: Option<String>,
    stdout: String,
    stderr: String,
    project_outcomes: Vec<ProjectTaskRecord>,
}

impl TaskOutcome {
    fn new(status: TaskStatus, summary: impl Into<String>, error: Option<String>) -> Self {
        Self {
            status,
            summary: summary.into(),
            error,
            stdout: String::new(),
            stderr: String::new(),
            project_outcomes: Vec::new(),
        }
    }

    pub fn succeeded(summary: impl Into<String>) -> Self {
        Self::new(TaskStatus::Succeeded, summary, None)
    }

    pub fn skipped(summary: impl Into<String>) -> Self {
        Self::new(TaskStatus::Skipped, summary, None)
    }

    pub fn failed(summary: impl Into<String>, error: impl Into<String>) -> Self {
        Self::new(TaskStatus::Failed, summary, Some(error.into()))
    }

    pub fn cancelled(summary: impl Into<String>) -> Self {
        Self::new(TaskStatus::Cancelled, summary, None)
    }

    pub fn with_stdout(mut self, stdout: impl Into<String>) -> Self {
        self.stdout = bounded_task_log(&stdout.into());
        self
    }

    pub fn with_stderr(mut self, stderr: impl Into<String>) -> Self {
        self.stderr = bounded_task_log(&stderr.into());
        self
    }

    pub fn with_project_outcomes(mut self, project_outcomes: Vec<ProjectTaskRecord>) -> Self {
        self.project_outcomes = project_outcomes;
        self
    }

    pub fn status(&self) -> &TaskStatus {
        &self.status
    }
}

impl TaskQueue {
    pub fn enqueue<F>(&self, kind: TaskKind, summary: impl Into<String>, job: F) -> TaskRecord
    where
        F: FnOnce(&mut TaskContext) -> TaskOutcome + Send + 'static,
    {
        self.enqueue_scoped(kind, None, summary, job)
    }

    pub fn enqueue_for_workspace<F>(
        &self,
        kind: TaskKind,
        workspace_root: impl Into<String>,
        summary: impl Into<String>,
        job: F,
    ) -> TaskRecord
    where
        F: FnOnce(&mut TaskContext) -> TaskOutcome + Send + 'static,
    {
        self.enqueue_scoped(kind, Some(workspace_root.into()), summary, job)
    }

    fn enqueue_scoped<F>(
        &self,
        kind: TaskKind,
        workspace_root: Option<String>,
        summary: impl Into<String>,
        job: F,
    ) -> TaskRecord
    where
        F: FnOnce(&mut TaskContext) -> TaskOutcome + Send + 'static,
    {
        let mut state = self.state.lock().expect("task queue state lock poisoned");
        state.next_id += 1;
        let id = format!("task-{}", state.next_id);
        let summary = summary.into();
        let cancel_requested = Arc::new(AtomicBool::new(false));
        let record = TaskRecord {
            id: id.clone(),
            workspace_root,
            kind: kind.clone(),
            status: TaskStatus::Queued,
            summary: summary.clone(),
            error: None,
            stdout: String::new(),
            stderr: String::new(),
            project_outcomes: Vec::new(),
        };
        state.records.push(record.clone());
        state
            .cancel_flags
            .insert(id.clone(), Arc::clone(&cancel_requested));
        state.queued.push_back(QueuedTask {
            id,
            cancel_requested,
            job: Box::new(job),
        });
        prune_completed_records(&mut state);
        record
    }

    pub fn run_next(&self) -> Option<TaskRecord> {
        let _runner = self.runner.lock().expect("task queue runner lock poisoned");
        let (task, running_record) = {
            let mut state = self.state.lock().expect("task queue state lock poisoned");
            let task = state.queued.pop_front()?;
            let record = state
                .records
                .iter_mut()
                .find(|record| record.id == task.id)?;
            if task.cancel_requested.load(Ordering::SeqCst)
                || matches!(record.status, TaskStatus::Cancelled)
            {
                record.status = TaskStatus::Cancelled;
                if !matches!(record.summary.as_str(), "Task cancelled before it started.") {
                    record.summary = "Task cancelled before it started.".to_string();
                }
                let record = record.clone();
                prune_completed_records(&mut state);
                return Some(record);
            }
            record.status = TaskStatus::Running;
            let record = record.clone();
            Some((task, record))
        }?;
        notify_task_update(&running_record);

        let mut context = TaskContext {
            task_id: self.publish_progress.then(|| task.id.clone()),
            cancel_requested: Arc::clone(&task.cancel_requested),
            stdout: String::new(),
            stderr: String::new(),
        };
        let mut outcome = (task.job)(&mut context);
        if !context.stdout.is_empty() {
            outcome.stdout = append_log(context.stdout, outcome.stdout);
        }
        if !context.stderr.is_empty() {
            outcome.stderr = append_log(context.stderr, outcome.stderr);
        }

        Some(self.finish_task(&task.id, outcome))
    }

    pub fn run_until_complete(&self, id: &str) -> Option<TaskRecord> {
        loop {
            let record = self.get_task_status(id)?;
            if !matches!(record.status, TaskStatus::Queued | TaskStatus::Running) {
                return Some(record);
            }
            if self.run_next().is_none() {
                std::thread::yield_now();
            }
        }
    }

    pub fn cancel_task(&self, id: &str) -> Option<TaskRecord> {
        let mut state = self.state.lock().expect("task queue state lock poisoned");
        if let Some(flag) = state.cancel_flags.get(id) {
            flag.store(true, Ordering::SeqCst);
        }

        let record = state.records.iter_mut().find(|record| record.id == id)?;
        if record.status == TaskStatus::Queued {
            record.status = TaskStatus::Cancelled;
            record.summary = "Task cancelled before it started.".to_string();
        }
        Some(record.clone())
    }

    fn append_task_stdout(&self, id: &str, line: &str) {
        self.append_task_output(id, line, true);
    }

    fn append_task_stderr(&self, id: &str, line: &str) {
        self.append_task_output(id, line, false);
    }

    fn append_task_output(&self, id: &str, line: &str, stdout: bool) {
        let record = {
            let mut state = self.state.lock().expect("task queue state lock poisoned");
            let Some(record) = state.records.iter_mut().find(|record| record.id == id) else {
                return;
            };
            let target = if stdout {
                &mut record.stdout
            } else {
                &mut record.stderr
            };
            append_bounded_log(target, line);
            if !target.ends_with('\n') {
                append_bounded_log(target, "\n");
            }
            record.clone()
        };
        notify_task_update(&record);
    }

    fn update_task_summary(&self, id: &str, summary: &str) {
        let record = {
            let mut state = self.state.lock().expect("task queue state lock poisoned");
            let Some(record) = state.records.iter_mut().find(|record| record.id == id) else {
                return;
            };
            record.summary = summary.to_string();
            record.clone()
        };
        notify_task_update(&record);
    }

    pub fn get_task_status(&self, id: &str) -> Option<TaskRecord> {
        self.state
            .lock()
            .expect("task queue state lock poisoned")
            .records
            .iter()
            .find(|record| record.id == id)
            .map(status_only_record)
    }

    pub fn get_task_logs(&self, id: &str) -> Option<TaskRecord> {
        self.state
            .lock()
            .expect("task queue state lock poisoned")
            .records
            .iter()
            .find(|record| record.id == id)
            .cloned()
    }

    pub fn recent_records(&self, limit: usize) -> Vec<TaskRecord> {
        self.recent_records_for_workspace(None, limit)
    }

    pub fn recent_records_for_workspace(
        &self,
        workspace_root: Option<&str>,
        limit: usize,
    ) -> Vec<TaskRecord> {
        let state = self.state.lock().expect("task queue state lock poisoned");
        state
            .records
            .iter()
            .rev()
            .filter(|record| match workspace_root {
                Some(root) => record.workspace_root.as_deref() == Some(root),
                None => true,
            })
            .take(limit)
            .map(status_only_record)
            .collect()
    }

    pub fn records(&self) -> Vec<TaskRecord> {
        self.state
            .lock()
            .expect("task queue state lock poisoned")
            .records
            .clone()
    }

    fn finish_task(&self, id: &str, outcome: TaskOutcome) -> TaskRecord {
        let mut state = self.state.lock().expect("task queue state lock poisoned");
        let record = state
            .records
            .iter_mut()
            .find(|record| record.id == id)
            .expect("finished task record missing");
        record.status = outcome.status;
        record.summary = outcome.summary;
        record.error = outcome.error;
        record.stdout = bounded_task_log(&outcome.stdout);
        record.stderr = bounded_task_log(&outcome.stderr);
        record.project_outcomes = outcome.project_outcomes;
        let record = record.clone();
        prune_completed_records(&mut state);
        notify_task_update(&record);
        record
    }
}

fn status_only_record(record: &TaskRecord) -> TaskRecord {
    let mut status = record.clone();
    status.stdout.clear();
    status.stderr.clear();
    status
}

/// Drop the oldest terminal-status records beyond `TASK_RECORD_MAX_RETAINED`
/// and purge cancel flags for records that are no longer cancellable.
///
/// Invariants:
/// - Records that are still queued or running are never removed here.
/// - `records` is insertion-ordered (new tasks are pushed to the back), so
///   `retain` walks from oldest to newest and removes the oldest terminal
///   records first.
/// - A record that gets pruned cannot be cancelled afterwards, which is fine
///   because only terminal records are eligible for pruning.
fn prune_completed_records(state: &mut TaskQueueState) {
    let queued_ids: std::collections::HashSet<String> =
        state.queued.iter().map(|task| task.id.clone()).collect();
    let completed_count = state
        .records
        .iter()
        .filter(|record| is_prunable_record(record, &queued_ids))
        .count();
    let mut to_remove = completed_count.saturating_sub(TASK_RECORD_MAX_RETAINED);

    if to_remove > 0 {
        state.records.retain(|record| {
            if to_remove > 0 && is_prunable_record(record, &queued_ids) {
                to_remove -= 1;
                false
            } else {
                true
            }
        });
    }

    state.cancel_flags.retain(|task_id, _| {
        queued_ids.contains(task_id)
            || state
                .records
                .iter()
                .any(|record| record.id == *task_id && !is_terminal_status(&record.status))
    });
}

fn is_prunable_record(record: &TaskRecord, queued_ids: &std::collections::HashSet<String>) -> bool {
    is_terminal_status(&record.status) && !queued_ids.contains(&record.id)
}

fn is_terminal_status(status: &TaskStatus) -> bool {
    !matches!(status, TaskStatus::Queued | TaskStatus::Running)
}

fn append_log(first: String, second: String) -> String {
    if second.is_empty() {
        return first;
    }
    if first.is_empty() {
        return second;
    }
    let mut log = String::new();
    append_bounded_log(&mut log, &first);
    append_bounded_log(&mut log, &second);
    log
}

pub fn bounded_task_log(log: &str) -> String {
    let mut bounded = String::new();
    append_bounded_log(&mut bounded, log);
    bounded
}

fn append_bounded_log(log: &mut String, chunk: &str) {
    if chunk.is_empty() {
        return;
    }
    log.push_str(chunk);
    truncate_log(log);
}

fn truncate_log(log: &mut String) {
    if log.len() <= TASK_LOG_MAX_BYTES {
        return;
    }

    let marker_len = TASK_LOG_TRUNCATED_MARKER.len();
    if marker_len >= TASK_LOG_MAX_BYTES {
        log.truncate(TASK_LOG_MAX_BYTES);
        return;
    }

    let keep_bytes = TASK_LOG_MAX_BYTES - marker_len;
    let mut start = log.len().saturating_sub(keep_bytes);
    while start < log.len() && !log.is_char_boundary(start) {
        start += 1;
    }
    let tail = log[start..].to_string();
    log.clear();
    log.push_str(TASK_LOG_TRUNCATED_MARKER);
    log.push_str(&tail);
}

static GLOBAL_TASK_QUEUE: OnceLock<TaskQueue> = OnceLock::new();

pub fn task_queue() -> &'static TaskQueue {
    GLOBAL_TASK_QUEUE.get_or_init(|| TaskQueue {
        publish_progress: true,
        ..TaskQueue::default()
    })
}

pub fn run_task_blocking<F>(kind: TaskKind, summary: impl Into<String>, job: F) -> TaskRecord
where
    F: FnOnce(&mut TaskContext) -> TaskOutcome + Send + 'static,
{
    let queued = task_queue().enqueue(kind, summary, job);
    run_queued_task_blocking(&queued.id)
}

pub fn run_workspace_task_blocking<F>(
    workspace_root: impl Into<String>,
    kind: TaskKind,
    summary: impl Into<String>,
    job: F,
) -> TaskRecord
where
    F: FnOnce(&mut TaskContext) -> TaskOutcome + Send + 'static,
{
    let queued = task_queue().enqueue_for_workspace(kind, workspace_root, summary, job);
    run_queued_task_blocking(&queued.id)
}

pub fn run_workspace_task_background<F>(
    workspace_root: impl Into<String>,
    kind: TaskKind,
    summary: impl Into<String>,
    job: F,
) -> TaskRecord
where
    F: FnOnce(&mut TaskContext) -> TaskOutcome + Send + 'static,
{
    let queued = task_queue().enqueue_for_workspace(kind, workspace_root, summary, job);
    let task_id = queued.id.clone();
    std::thread::Builder::new()
        .name(format!("task-runner-{task_id}"))
        .spawn(move || {
            run_queued_task_blocking(&task_id);
        })
        .expect("failed to spawn background task runner");
    queued
}

fn run_queued_task_blocking(task_id: &str) -> TaskRecord {
    task_queue()
        .run_until_complete(task_id)
        .expect("queued task disappeared");
    task_queue()
        .get_task_logs(task_id)
        .expect("completed task disappeared")
}

#[cfg_attr(feature = "desktop", tauri::command(rename_all = "camelCase"))]
pub fn get_task_status_command(task_id: String) -> Option<TaskRecord> {
    task_queue().get_task_status(&task_id)
}

#[cfg_attr(feature = "desktop", tauri::command(rename_all = "camelCase"))]
pub fn get_task_logs_command(task_id: String) -> Option<TaskRecord> {
    task_queue().get_task_logs(&task_id)
}

#[cfg_attr(feature = "desktop", tauri::command(rename_all = "camelCase"))]
pub fn recent_task_records_command(
    workspace_root: Option<String>,
    limit: Option<usize>,
) -> Vec<TaskRecord> {
    // Limit is clamped to `TASK_RECENT_MAX_LIMIT` so that a malicious or
    // buggy caller cannot pull an unbounded slice of history across the IPC
    // boundary. `workspace_root` filtering assumes both sides use the canonical
    // path produced by `validate_workspace_root` in workspace.rs.
    task_queue().recent_records_for_workspace(
        workspace_root.as_deref(),
        limit
            .unwrap_or(TASK_RECENT_DEFAULT_LIMIT)
            .min(TASK_RECENT_MAX_LIMIT),
    )
}

#[cfg_attr(feature = "desktop", tauri::command(rename_all = "camelCase"))]
pub fn cancel_task_command(task_id: String) -> Option<TaskRecord> {
    task_queue().cancel_task(&task_id)
}
