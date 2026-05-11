import { useEffect, useMemo, useRef, useState } from "react";
import { openWorkspacePath } from "../lib/commands";
import { projectUpdateDetail, projectUpdateTitle, statusLabel } from "../lib/format";
import { EmptyState, PanelHeader, errorMessage } from "../lib/shared";
import type { GitStatus, Project, TaskRecord } from "../lib/types";

type ProjectErrorLink = {
  task: TaskRecord;
  outcome: { projectId: string; status: string; summary: string; error?: string };
};

// All possible git statuses, ordered so the filter always shows a stable, complete
// set of options. Keeping every status visible ensures changing the filter is a
// meaningful UI update even when the workspace only contains a single status.
const ALL_GIT_STATUSES: readonly GitStatus[] = [
  "up_to_date",
  "behind",
  "ahead",
  "diverged",
  "dirty",
  "no_upstream",
  "detached",
  "fetch_failed",
  "unknown",
];

export function buildLatestProjectErrorIndex(tasks: TaskRecord[], workspaceRoot: string) {
  const errors = new Map<string, ProjectErrorLink>();
  const seenProjectIds = new Set<string>();
  for (const task of tasks) {
    if (task.workspaceRoot !== workspaceRoot) continue;
    for (const outcome of task.projectOutcomes) {
      if (seenProjectIds.has(outcome.projectId)) continue;
      seenProjectIds.add(outcome.projectId);
      if (outcome.status === "failed") {
        errors.set(outcome.projectId, { task, outcome });
      }
    }
  }
  return errors;
}

export function ProjectsView({
  onCheckAll,
  onCheckProject,
  onImport,
  onOpenTaskLog,
  onPullAll,
  onPullProject,
  onSetProjectHidden,
  operationBusy,
  projects,
  taskHistory,
  workspaceRoot,
}: {
  onCheckAll: () => void;
  onCheckProject: (projectId: string) => void;
  onImport: (source: string, directoryName: string, shallow: boolean) => void;
  onOpenTaskLog: (taskId: string) => void;
  onPullAll: (autostash: boolean) => void;
  onPullProject: (projectId: string, autostash: boolean) => void;
  onSetProjectHidden: (projectId: string, hidden: boolean) => void;
  operationBusy: boolean;
  projects: Project[];
  taskHistory: TaskRecord[];
  workspaceRoot: string;
}) {
  const [source, setSource] = useState("");
  const [directoryName, setDirectoryName] = useState("");
  const [shallow, setShallow] = useState(false);
  const [autostash, setAutostash] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [hiddenFilter, setHiddenFilter] = useState<"visible" | "all" | "hidden">("visible");
  const [statusFilter, setStatusFilter] = useState<"all" | Project["gitStatus"]>("all");
  const [query, setQuery] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  // Ref guard so rapid clicks on per-row async buttons (open / hide / log) do
  // not fire overlapping backend calls.
  const rowActionBusyRef = useRef(false);
  const [rowActionBusy, setRowActionBusy] = useState(false);

  useEffect(() => {
    if (!message) return;
    const timer = window.setTimeout(() => setMessage(null), 5000);
    return () => window.clearTimeout(timer);
  }, [message]);

  const runRowAction = async (action: () => Promise<void> | void) => {
    if (rowActionBusyRef.current) return;
    rowActionBusyRef.current = true;
    setRowActionBusy(true);
    try {
      await action();
    } finally {
      rowActionBusyRef.current = false;
      setRowActionBusy(false);
    }
  };

  const openProjectPath = (project: Project) =>
    runRowAction(async () => {
      try {
        await openWorkspacePath(workspaceRoot, project.path);
        setMessage(`Opening ${project.name}.`);
      } catch (error) {
        setMessage(errorMessage(error));
      }
    });

  const toggleProjectHidden = (project: Project) =>
    runRowAction(async () => {
      await onSetProjectHidden(project.id, !project.hidden);
    });

  const openTaskLogOnce = (taskId: string) =>
    runRowAction(() => {
      onOpenTaskLog(taskId);
    });

  const visibleProjects = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    return projects.filter((project) => {
      if (hiddenFilter === "visible" && project.hidden) return false;
      if (hiddenFilter === "hidden" && !project.hidden) return false;
      if (statusFilter !== "all" && project.gitStatus !== statusFilter) return false;
      if (!normalizedQuery) return true;
      return [project.name, project.id, project.remoteUrl, project.branch, project.upstream]
        .filter(Boolean)
        .some((value) => value!.toLowerCase().includes(normalizedQuery));
    });
  }, [hiddenFilter, projects, query, statusFilter]);

  // Count how many projects fall into each git status so the dropdown can show
  // a per-option count and the user sees the filter's discriminating power.
  // The counts intentionally ignore the current `hiddenFilter` / `query` so
  // switching status always reflects the real distribution of the workspace.
  const statusCounts = useMemo(() => {
    const counts = new Map<GitStatus, number>();
    for (const project of projects) {
      counts.set(project.gitStatus, (counts.get(project.gitStatus) ?? 0) + 1);
    }
    return counts;
  }, [projects]);

  // Always expose every git status in the dropdown (sorted by known order),
  // even statuses with zero matches. This guarantees switching between
  // options produces a visible list change, fixing the "nothing happens"
  // feeling when every project currently shares a single status.
  const statusOptions = useMemo<GitStatus[]>(() => {
    const extras = Array.from(statusCounts.keys()).filter(
      (status) => !ALL_GIT_STATUSES.includes(status),
    );
    return [...ALL_GIT_STATUSES, ...extras.sort()];
  }, [statusCounts]);

  // Defensive: if the selected status is neither part of the canonical list
  // nor present in the current workspace, fall back to "all" so the select
  // never shows an invalid value. Zero-count canonical statuses stay valid —
  // that is exactly the scenario that makes the filter give visible feedback.
  useEffect(() => {
    if (statusFilter === "all") return;
    const isCanonical = (ALL_GIT_STATUSES as readonly string[]).includes(statusFilter);
    if (!isCanonical && !statusCounts.has(statusFilter)) {
      setStatusFilter("all");
    }
  }, [statusFilter, statusCounts]);
  const projectErrorById = useMemo(
    () => buildLatestProjectErrorIndex(taskHistory, workspaceRoot),
    [taskHistory, workspaceRoot],
  );

  const submitImport = () => {
    if (!source.trim()) return;
    onImport(source.trim(), directoryName.trim(), shallow);
    setSource("");
    setDirectoryName("");
    setShallow(false);
    setImportOpen(false);
  };

  return (
    <>
      {importOpen && (
        <div className="dialog-backdrop" role="presentation">
          <form
            aria-modal="true"
            className="data-panel compact-form import-dialog"
            onSubmit={(event) => {
              event.preventDefault();
              submitImport();
            }}
            role="dialog"
          >
            <PanelHeader title="Import repository" detail="GitHub shorthand or Git URL" />
            <label>
              <span>Repository</span>
              <input
                autoFocus
                onChange={(event) => setSource(event.target.value)}
                placeholder="owner/repo or https://..."
                value={source}
              />
            </label>
            <details open>
              <summary>Advanced options</summary>
              <div className="advanced-options">
                <label>
                  <span>Directory name</span>
                  <input
                    onChange={(event) => setDirectoryName(event.target.value)}
                    placeholder="Optional"
                    value={directoryName}
                  />
                </label>
                <label className="inline-check">
                  <input checked={shallow} onChange={(event) => setShallow(event.target.checked)} type="checkbox" />
                  <span>Shallow clone</span>
                </label>
              </div>
            </details>
            <div className="panel-actions">
              <button className="primary-button" type="submit">Import</button>
              <button className="secondary-button" onClick={() => setImportOpen(false)} type="button">Cancel</button>
            </div>
          </form>
        </div>
      )}
      <section className="data-panel">
        <PanelHeader title="Projects" detail={`${visibleProjects.length} of ${projects.length} repositories`} />
        <div className="panel-actions">
          <button className="primary-button" disabled={operationBusy} onClick={() => setImportOpen(true)} type="button">Import</button>
          <button className="secondary-button" disabled={operationBusy} onClick={onCheckAll} type="button">Check all</button>
          <button className="secondary-button" disabled={operationBusy} onClick={() => onPullAll(autostash)} type="button">Pull safe</button>
          <label className="inline-check">
            <input checked={autostash} onChange={(event) => setAutostash(event.target.checked)} type="checkbox" />
            <span>Autostash</span>
          </label>
        </div>
        <div className="filter-bar">
          <label>
            <span>Filter</span>
            <input onChange={(event) => setQuery(event.target.value)} placeholder="Name, remote, branch" value={query} />
          </label>
          <label>
            <span>Status</span>
            <select onChange={(event) => setStatusFilter(event.target.value as "all" | Project["gitStatus"])} value={statusFilter}>
              <option value="all">All statuses ({projects.length})</option>
              {statusOptions.map((status) => (
                <option key={status} value={status}>
                  {statusLabel(status)} ({statusCounts.get(status) ?? 0})
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>Visibility</span>
            <select onChange={(event) => setHiddenFilter(event.target.value as "visible" | "all" | "hidden")} value={hiddenFilter}>
              <option value="visible">Visible projects</option>
              <option value="all">All projects</option>
              <option value="hidden">Hidden only</option>
            </select>
          </label>
        </div>
        {message && <p className="batch-message">{message}</p>}
        {projects.length === 0 ? (
          <EmptyState title="No projects found" body="Import or refresh after adding top-level Git repositories." />
        ) : visibleProjects.length === 0 ? (
          <EmptyState title="No matching projects" body="Adjust filters to show more repositories." />
        ) : (
          <div className="table-list">
            {visibleProjects.map((project) => {
              const projectError = projectErrorById.get(project.id);
              return (
                <article className="list-row project-row" key={project.id}>
                  <div className="project-summary">
                    <div className="project-title-line">
                      <h2>{project.name}</h2>
                    </div>
                    <p>{project.remoteUrl || project.path}</p>
                  </div>
                  <div className="project-insights" aria-label={`Project summary for ${project.name}`}>
                    <div className="project-insight">
                      <span>Skills</span>
                      <strong>{project.skillCount}</strong>
                      <small>{project.skillCount === 1 ? "skill" : "skills"}</small>
                    </div>
                    <div
                      className={
                        project.gitStatus === "up_to_date"
                          ? "project-insight update-status is-current"
                          : "project-insight update-status needs-attention"
                      }
                    >
                      <span>Update status</span>
                      <strong>{projectUpdateTitle(project)}</strong>
                      <small>{projectUpdateDetail(project)}</small>
                      {projectError && (
                        <button
                          className="text-button error-link"
                          disabled={rowActionBusy}
                          onClick={() => openTaskLogOnce(projectError.task.id)}
                          type="button"
                        >
                          {projectError.outcome.error || projectError.outcome.summary}
                        </button>
                      )}
                    </div>
                  </div>
                  <div className="row-actions">
                    <button className="secondary-button" disabled={operationBusy} onClick={() => onCheckProject(project.id)} type="button">Check</button>
                    <button className="secondary-button" disabled={operationBusy} onClick={() => onPullProject(project.id, autostash)} type="button">Pull</button>
                    <button className="secondary-button" disabled={rowActionBusy} onClick={() => openProjectPath(project)} type="button">Open project</button>
                    <button
                      className="secondary-button"
                      disabled={rowActionBusy || operationBusy}
                      onClick={() => toggleProjectHidden(project)}
                      type="button"
                    >
                      {project.hidden ? "Show" : "Hide"}
                    </button>
                  </div>
                </article>
              );
            })}
          </div>
        )}
      </section>
    </>
  );
}
