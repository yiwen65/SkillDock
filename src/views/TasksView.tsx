import React, { useEffect, useRef, useState } from "react";
import { cancelTask, getTaskLogs, getTaskStatus } from "../lib/commands";
import { EmptyState, PanelHeader, errorMessage, preserveLogs } from "../lib/shared";
import type { TaskRecord } from "../lib/types";

const TASKS_RECENT_LIMIT = 80;
const LOG_PREVIEW_CHARS = 12000;

type BoundedLog = { text: string; truncated: boolean };

function boundedLog(log: string): BoundedLog {
  if (log.length <= LOG_PREVIEW_CHARS) return { text: log, truncated: false };
  return { text: log.slice(log.length - LOG_PREVIEW_CHARS), truncated: true };
}

function rawTaskLogs(task: TaskRecord) {
  return [
    task.stdout ? `--- stdout ---\n${task.stdout}` : "",
    task.stderr ? `--- stderr ---\n${task.stderr}` : "",
  ].filter(Boolean).join("\n\n");
}

function LogBlock({ label, log }: { label: string; log: BoundedLog }) {
  return (
    <div className="log-block">
      <div className="panel-header">
        <h2>{label}</h2>
        {log.truncated && <span>Showing last {LOG_PREVIEW_CHARS.toLocaleString()} characters</span>}
      </div>
      <pre>{log.text || `No ${label} captured.`}</pre>
    </div>
  );
}

export function TasksView({
  focusedTaskId,
  onTaskChange,
  tasks,
}: {
  focusedTaskId: string | null;
  onTaskChange: (task: TaskRecord) => void;
  tasks: TaskRecord[];
}) {
  const [message, setMessage] = useState<string | null>(null);
  // Rapid-click guard for the batch "Refresh statuses" button. Without it a
  // user double-clicking spawns a fan-out of get_task_status requests per click.
  const refreshAllBusyRef = useRef(false);
  const [refreshAllBusy, setRefreshAllBusy] = useState(false);

  useEffect(() => {
    if (!message) return;
    const timer = window.setTimeout(() => setMessage(null), 5000);
    return () => window.clearTimeout(timer);
  }, [message]);

  const refreshAll = async () => {
    if (refreshAllBusyRef.current) return;
    refreshAllBusyRef.current = true;
    setRefreshAllBusy(true);
    setMessage("Refreshing task statuses...");
    try {
      const targets = tasks.slice(0, TASKS_RECENT_LIMIT);
      const results = await Promise.allSettled(targets.map((task) => getTaskStatus(task.id)));
      let refreshed = 0;
      let failures = 0;
      for (const result of results) {
        if (result.status === "fulfilled") {
          if (result.value) {
            onTaskChange(result.value);
            refreshed += 1;
          }
        } else {
          failures += 1;
        }
      }
      if (failures === 0) {
        setMessage(`Refreshed ${refreshed} of ${targets.length} task statuses.`);
      } else if (refreshed === 0) {
        setMessage(`Failed to refresh task statuses (${failures} errors).`);
      } else {
        setMessage(`Refreshed ${refreshed} of ${targets.length} task statuses (${failures} errors).`);
      }
    } finally {
      refreshAllBusyRef.current = false;
      setRefreshAllBusy(false);
    }
  };

  if (tasks.length === 0) {
    return <EmptyState title="No task logs" body="Run a workspace operation (import, check, pull, install) to create logs." />;
  }

  return (
    <section className="data-panel">
      <PanelHeader title="Logs" detail={`${tasks.length} recent tasks`} />
      <div className="panel-actions">
        <button
          className="secondary-button"
          disabled={refreshAllBusy}
          onClick={refreshAll}
          type="button"
        >
          Refresh statuses
        </button>
      </div>
      {message && <p className="batch-message">{message}</p>}
      <div className="table-list">
        {tasks.map((task) => (
          <TaskLogRow focused={task.id === focusedTaskId} key={task.id} onTaskChange={onTaskChange} task={task} />
        ))}
      </div>
    </section>
  );
}

const TaskLogRow = React.memo(function TaskLogRow({
  focused,
  onTaskChange,
  task,
}: {
  focused: boolean;
  onTaskChange: (task: TaskRecord) => void;
  task: TaskRecord;
}) {
  const [expanded, setExpanded] = useState(focused);
  const [logTask, setLogTask] = useState(task);
  const [message, setMessage] = useState<string | null>(null);
  const articleRef = useRef<HTMLElement | null>(null);
  const autoLoadedRef = useRef(false);
  // Rapid-click guard shared by every async per-row action (Refresh, Expand/
  // Reload logs, Copy raw, Cancel). Only one backend round-trip is allowed at
  // a time per row.
  const rowBusyRef = useRef(false);
  const [rowBusy, setRowBusy] = useState(false);

  const runRowAction = async (action: () => Promise<void>) => {
    if (rowBusyRef.current) return;
    rowBusyRef.current = true;
    setRowBusy(true);
    try {
      await action();
    } finally {
      rowBusyRef.current = false;
      setRowBusy(false);
    }
  };

  useEffect(() => {
    setLogTask((current) => preserveLogs(current, task));
  }, [task]);

  useEffect(() => {
    if (!focused) return;
    setExpanded(true);
    articleRef.current?.scrollIntoView({ behavior: "smooth", block: "center" });
    if (autoLoadedRef.current) return;
    setLogTask((current) => {
      if (current.stdout || current.stderr) {
        autoLoadedRef.current = true;
        return current;
      }
      void (async () => {
        try {
          const nextTask = await getTaskLogs(task.id);
          if (nextTask) {
            autoLoadedRef.current = true;
            setLogTask((latest) => preserveLogs(latest, nextTask));
            onTaskChange(nextTask);
          }
        } catch {
          // Auto-load failures stay silent
        }
      })();
      return current;
    });
  }, [focused, task.id, onTaskChange]);

  const refreshStatus = () =>
    runRowAction(async () => {
      setMessage("Refreshing...");
      try {
        const nextTask = await getTaskStatus(task.id);
        if (nextTask) {
          setLogTask((current) => preserveLogs(current, nextTask));
          onTaskChange(nextTask);
          setMessage("Status refreshed.");
        } else {
          setMessage("Task record is no longer available.");
        }
      } catch (error) {
        setMessage(errorMessage(error));
      }
    });

  const loadFullLogs = () =>
    runRowAction(async () => {
      setExpanded(true);
      setMessage("Loading logs...");
      try {
        const nextTask = await getTaskLogs(task.id);
        if (nextTask) {
          setLogTask(nextTask);
          onTaskChange(nextTask);
          setMessage("Logs loaded.");
        } else {
          setMessage("Task logs are no longer available.");
        }
      } catch (error) {
        setMessage(errorMessage(error));
      }
    });

  const cancelQueuedTask = () =>
    runRowAction(async () => {
      setMessage("Cancelling...");
      try {
        const nextTask = await cancelTask(task.id);
        if (nextTask) {
          setLogTask((current) => preserveLogs(current, nextTask));
          onTaskChange(nextTask);
          setMessage(
            nextTask.status === "cancelled"
              ? "Task cancelled."
              : "Cancellation requested; waiting for task to stop.",
          );
        } else {
          setMessage("Task record is no longer available.");
        }
      } catch (error) {
        setMessage(errorMessage(error));
      }
    });

  const copyLogs = () =>
    runRowAction(async () => {
      const raw = rawTaskLogs(logTask);
      try {
        await navigator.clipboard.writeText(raw);
        setMessage("Logs copied.");
      } catch {
        setMessage("Clipboard is unavailable; select the log text above and copy it manually.");
      }
    });

  const stdout = boundedLog(logTask.stdout);
  const stderr = boundedLog(logTask.stderr);
  const projectErrors = logTask.projectOutcomes.filter((outcome) => outcome.error);

  return (
    <article
      className={focused ? "list-row task-row focused-task" : "list-row task-row"}
      id={`task-${logTask.id}`}
      ref={articleRef}
    >
      <div className="task-summary">
        <h2>{logTask.kind} / {logTask.status}</h2>
        <p>{logTask.summary}</p>
        {logTask.error && <p className="form-error">{logTask.error}</p>}
        {projectErrors.length > 0 && (
          <div className="task-error-list">
            {projectErrors.slice(0, 5).map((outcome) => (
              <p className="form-error" key={outcome.projectId}>
                {outcome.projectId}: {outcome.error || outcome.summary}
              </p>
            ))}
            {projectErrors.length > 5 && (
              <p className="batch-message">{projectErrors.length - 5} more project errors.</p>
            )}
          </div>
        )}
        <div className="panel-actions">
          <button className="secondary-button" disabled={rowBusy} onClick={refreshStatus} type="button">Refresh</button>
          <button className="secondary-button" disabled={rowBusy} onClick={loadFullLogs} type="button">
            {expanded ? "Reload logs" : "Expand logs"}
          </button>
          <button className="secondary-button" disabled={rowBusy} onClick={copyLogs} type="button">Copy raw</button>
          {(logTask.status === "queued" || logTask.status === "running") && (
            <button className="secondary-button" disabled={rowBusy} onClick={cancelQueuedTask} type="button">Cancel</button>
          )}
        </div>
        {message && <p className="batch-message">{message}</p>}
      </div>
      <div className="task-log-stack">
        {!expanded ? (
          <pre>{boundedLog(rawTaskLogs(logTask)).text || "No output captured."}</pre>
        ) : (
          <>
            <LogBlock label="stdout" log={stdout} />
            <LogBlock label="stderr" log={stderr} />
          </>
        )}
      </div>
    </article>
  );
});
