use skilldock_lib::{
    bounded_task_log, TaskKind, TaskOutcome, TaskQueue, TaskStatus, TASK_LOG_MAX_BYTES,
    TASK_RECORD_MAX_RETAINED,
};

#[test]
fn queued_task_can_be_cancelled_before_it_runs() {
    let queue = TaskQueue::default();

    let task = queue.enqueue(TaskKind::ImportProject, "import project", |_| {
        TaskOutcome::succeeded("should not run")
    });

    let cancelled = queue.cancel_task(&task.id).unwrap();

    assert_eq!(cancelled.status, TaskStatus::Cancelled);
    let final_record = queue.run_next().unwrap();
    assert_eq!(final_record.status, TaskStatus::Cancelled);
}

#[test]
fn task_queue_runs_tasks_serially_and_captures_logs() {
    let queue = TaskQueue::default();

    let first = queue.enqueue(TaskKind::FetchProject, "first", |context| {
        context.stdout("first stdout");
        TaskOutcome::succeeded("first done")
    });
    let second = queue.enqueue(TaskKind::PullProject, "second", |context| {
        context.stderr("second stderr");
        TaskOutcome::failed("second failed", "pull failed")
    });

    let first_done = queue.run_next().unwrap();
    let second_done = queue.run_next().unwrap();

    assert_eq!(first_done.id, first.id);
    assert_eq!(first_done.status, TaskStatus::Succeeded);
    assert!(first_done.stdout.contains("first stdout"));
    assert_eq!(second_done.id, second.id);
    assert_eq!(second_done.status, TaskStatus::Failed);
    assert_eq!(second_done.error.as_deref(), Some("pull failed"));
    assert!(second_done.stderr.contains("second stderr"));
}

#[test]
fn task_queue_recent_records_include_queued_tasks_before_they_run() {
    let queue = TaskQueue::default();

    let queued = queue.enqueue(TaskKind::PullProject, "pull project", |_| {
        TaskOutcome::succeeded("pull done")
    });

    let recent = queue.recent_records(10);

    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].id, queued.id);
    assert_eq!(recent[0].status, TaskStatus::Queued);
}

#[test]
fn recent_records_can_be_filtered_by_workspace_root() {
    let queue = TaskQueue::default();

    let first = queue.enqueue_for_workspace(
        TaskKind::FetchProject,
        "/workspace/one",
        "first workspace",
        |_| TaskOutcome::succeeded("first done"),
    );
    let second = queue.enqueue_for_workspace(
        TaskKind::PullProject,
        "/workspace/two",
        "second workspace",
        |_| TaskOutcome::succeeded("second done"),
    );

    let first_recent = queue.recent_records_for_workspace(Some("/workspace/one"), 10);
    let second_recent = queue.recent_records_for_workspace(Some("/workspace/two"), 10);

    assert_eq!(first_recent.len(), 1);
    assert_eq!(first_recent[0].id, first.id);
    assert_eq!(
        first_recent[0].workspace_root.as_deref(),
        Some("/workspace/one")
    );
    assert_eq!(second_recent.len(), 1);
    assert_eq!(second_recent[0].id, second.id);
}

#[test]
fn task_logs_are_bounded_when_stored_on_records() {
    let queue = TaskQueue::default();
    let oversized = "x".repeat(TASK_LOG_MAX_BYTES + 1024);

    let queued = queue.enqueue(TaskKind::FetchProject, "fetch project", move |context| {
        context.stdout(&oversized);
        TaskOutcome::failed("fetch failed", "too much output").with_stderr(oversized)
    });
    queue.run_until_complete(&queued.id).unwrap();
    let finished = queue.get_task_logs(&queued.id).unwrap();

    assert!(finished.stdout.len() <= TASK_LOG_MAX_BYTES);
    assert!(finished.stderr.len() <= TASK_LOG_MAX_BYTES);
    assert!(finished.stdout.starts_with("[task log truncated;"));
    assert!(finished.stderr.starts_with("[task log truncated;"));
    assert_eq!(bounded_task_log(&finished.stdout), finished.stdout);
}

#[test]
fn status_and_recent_records_strip_logs_until_logs_are_requested() {
    let queue = TaskQueue::default();

    let queued = queue.enqueue(TaskKind::FetchProject, "fetch project", |context| {
        context.stdout("status should not carry stdout");
        context.stderr("status should not carry stderr");
        TaskOutcome::succeeded("fetch done")
    });
    queue.run_until_complete(&queued.id).unwrap();

    let status = queue.get_task_status(&queued.id).unwrap();
    let recent = queue.recent_records(10);
    let logs = queue.get_task_logs(&queued.id).unwrap();

    assert!(status.stdout.is_empty());
    assert!(status.stderr.is_empty());
    assert!(recent[0].stdout.is_empty());
    assert!(recent[0].stderr.is_empty());
    assert!(logs.stdout.contains("status should not carry stdout"));
    assert!(logs.stderr.contains("status should not carry stderr"));
}

#[test]
fn completed_task_records_are_pruned_without_pruning_queued_tasks() {
    let queue = TaskQueue::default();

    for index in 0..(TASK_RECORD_MAX_RETAINED + 5) {
        let queued = queue.enqueue(TaskKind::FetchProject, format!("finished {index}"), |_| {
            TaskOutcome::succeeded("done")
        });
        queue.run_until_complete(&queued.id).unwrap();
    }
    let queued = queue.enqueue(TaskKind::PullProject, "still queued", |_| {
        TaskOutcome::succeeded("queued done")
    });

    let records = queue.records();
    let completed = records
        .iter()
        .filter(|record| !matches!(record.status, TaskStatus::Queued | TaskStatus::Running))
        .count();

    assert_eq!(completed, TASK_RECORD_MAX_RETAINED);
    assert!(records.iter().any(|record| record.id == queued.id));
    assert!(queue.get_task_status("task-1").is_none());
}
