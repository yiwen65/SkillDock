import { memo, useEffect, useMemo, useRef, useState } from "react";
import { projectUpdateDetail, projectUpdateTitle } from "../lib/format";
import { openWorkspacePathWithCopyFallback } from "../lib/openPathFallback";
import { EmptyState, PanelHeader } from "../lib/shared";
import { VirtualList } from "../lib/VirtualList";
import type { GitStatus, Project, TaskRecord } from "../lib/types";

type ProjectErrorLink = {
  task: TaskRecord;
  outcome: { projectId: string; status: string; summary: string; error?: string };
};

type ProjectStatusFilter =
  | "all"
  | "up_to_date"
  | "updates_available"
  | "local_changes"
  | "attention";

type ProjectStatusGroup = Exclude<ProjectStatusFilter, "all">;

const PROJECT_STATUS_GROUPS: readonly { value: ProjectStatusGroup; label: string }[] = [
  { value: "up_to_date", label: "Up to date" },
  { value: "updates_available", label: "Updates available" },
  { value: "local_changes", label: "Local changes" },
  { value: "attention", label: "Needs attention" },
];

function projectStatusGroup(status: GitStatus): ProjectStatusGroup {
  switch (status) {
    case "up_to_date":
      return "up_to_date";
    case "behind":
      return "updates_available";
    case "ahead":
    case "dirty":
      return "local_changes";
    default:
      return "attention";
  }
}

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

export const ProjectsView = memo(function ProjectsView({
  onCheckAll,
  onCheckProject,
  onImport,
  onOpenTaskLog,
  onPullAll,
  onPullProject,
  onSetProjectHidden,
  onDeleteProject,
  operationBusy,
  projects,
  taskHistory,
  workspaceRoot,
}: {
  onCheckAll: () => void;
  onCheckProject: (projectId: string) => void;
  onImport: (source: string, directoryName: string, shallow: boolean, skillPath: string) => void;
  onOpenTaskLog: (taskId: string) => void;
  onPullAll: (autostash: boolean) => void;
  onPullProject: (projectId: string, autostash: boolean) => void;
  onSetProjectHidden: (projectId: string, hidden: boolean) => void;
  onDeleteProject: (projectId: string) => void;
  operationBusy: boolean;
  projects: Project[];
  taskHistory: TaskRecord[];
  workspaceRoot: string;
}) {
  const [source, setSource] = useState("");
  const [directoryName, setDirectoryName] = useState("");
  const [skillPath, setSkillPath] = useState("");
  const [shallow, setShallow] = useState(false);
  const [autostash, setAutostash] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Project | null>(null);
  const [hiddenFilter, setHiddenFilter] = useState<"visible" | "all" | "hidden">("visible");
  const [statusFilter, setStatusFilter] = useState<ProjectStatusFilter>("all");
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
      setMessage(
        await openWorkspacePathWithCopyFallback({
          label: project.name,
          path: project.path,
          workspaceRoot,
        }),
      );
    });

  const toggleProjectHidden = (project: Project) =>
    runRowAction(async () => {
      await onSetProjectHidden(project.id, !project.hidden);
    });

  const confirmDeleteProject = (project: Project) => {
    setDeleteTarget(project);
  };

  const executeDeleteProject = () =>
    runRowAction(async () => {
      if (!deleteTarget) return;
      setDeleteTarget(null);
      await onDeleteProject(deleteTarget.id);
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
      if (statusFilter !== "all" && projectStatusGroup(project.gitStatus) !== statusFilter) {
        return false;
      }
      if (!normalizedQuery) return true;
      return [project.name, project.id, project.remoteUrl, project.branch, project.upstream]
        .filter(Boolean)
        .some((value) => value!.toLowerCase().includes(normalizedQuery));
    });
  }, [hiddenFilter, projects, query, statusFilter]);

  // Count by user-facing status groups instead of raw Git states; raw states
  // such as detached, no upstream, and fetch failed all need the same action.
  const statusCounts = useMemo(() => {
    const counts = new Map<ProjectStatusGroup, number>();
    for (const project of projects) {
      const group = projectStatusGroup(project.gitStatus);
      counts.set(group, (counts.get(group) ?? 0) + 1);
    }
    return counts;
  }, [projects]);

  const statusOptions = useMemo(() => {
    return PROJECT_STATUS_GROUPS.filter((option) => (statusCounts.get(option.value) ?? 0) > 0);
  }, [statusCounts]);

  useEffect(() => {
    if (statusFilter === "all") return;
    if (!statusCounts.has(statusFilter)) {
      setStatusFilter("all");
    }
  }, [statusFilter, statusCounts]);
  const projectErrorById = useMemo(
    () => buildLatestProjectErrorIndex(taskHistory, workspaceRoot),
    [taskHistory, workspaceRoot],
  );

  const submitImport = () => {
    if (!source.trim()) return;
    onImport(source.trim(), directoryName.trim(), shallow, skillPath.trim());
    setSource("");
    setDirectoryName("");
    setSkillPath("");
    setShallow(false);
    setImportOpen(false);
  };

  return (
    <>
      {deleteTarget && (
        <div className="dialog-backdrop" role="presentation" onClick={() => setDeleteTarget(null)}>
          <div
            className="data-panel compact-form import-dialog"
            role="dialog"
            aria-modal="true"
            onClick={(e) => e.stopPropagation()}
          >
            <PanelHeader title="Delete project" detail="" />
            <p>
              Are you sure you want to delete <strong>{deleteTarget.name}</strong>?
            </p>
            <p className="form-error">
              This will permanently remove the directory:
              <br />
              {deleteTarget.path}
            </p>
            <div className="panel-actions">
              <button
                className="primary-button danger-button"
                onClick={executeDeleteProject}
                type="button"
              >
                Delete
              </button>
              <button
                className="secondary-button"
                onClick={() => setDeleteTarget(null)}
                type="button"
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
      {importOpen && (
        <ProjectImportDialog
          directoryName={directoryName}
          onCancel={() => setImportOpen(false)}
          onDirectoryNameChange={setDirectoryName}
          onShallowChange={setShallow}
          onSkillPathChange={setSkillPath}
          onSourceChange={setSource}
          onSubmit={submitImport}
          shallow={shallow}
          skillPath={skillPath}
          source={source}
        />
      )}
      <section className="data-panel">
        <PanelHeader
          title="Projects"
          detail={`${visibleProjects.length} of ${projects.length} repositories`}
        />
        <div className="panel-actions">
          <button
            className="primary-button"
            disabled={operationBusy}
            onClick={() => setImportOpen(true)}
            type="button"
          >
            Import
          </button>
          <button
            className="secondary-button"
            disabled={operationBusy}
            onClick={onCheckAll}
            type="button"
          >
            Check all
          </button>
          <button
            className="secondary-button"
            disabled={operationBusy}
            onClick={() => onPullAll(autostash)}
            type="button"
          >
            Pull safe
          </button>
          <label className="inline-check">
            <input
              checked={autostash}
              onChange={(event) => setAutostash(event.target.checked)}
              type="checkbox"
            />
            <span>Autostash</span>
          </label>
        </div>
        <div className="filter-bar">
          <label>
            <span>Filter</span>
            <input
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Name, remote, branch"
              value={query}
            />
          </label>
          <label>
            <span>Status</span>
            <select
              onChange={(event) => setStatusFilter(event.target.value as ProjectStatusFilter)}
              value={statusFilter}
            >
              <option value="all">All statuses ({projects.length})</option>
              {statusOptions.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label} ({statusCounts.get(option.value) ?? 0})
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>Visibility</span>
            <select
              onChange={(event) =>
                setHiddenFilter(event.target.value as "visible" | "all" | "hidden")
              }
              value={hiddenFilter}
            >
              <option value="visible">Visible projects</option>
              <option value="all">All projects</option>
              <option value="hidden">Hidden only</option>
            </select>
          </label>
        </div>
        {message && <p className="batch-message">{message}</p>}
        {projects.length === 0 ? (
          <EmptyState
            title="No projects found"
            body="Import or refresh after adding top-level Git repositories."
          />
        ) : visibleProjects.length === 0 ? (
          <EmptyState
            title="No matching projects"
            body="Adjust filters to show more repositories."
          />
        ) : (
          <VirtualList
            className="table-list project-list"
            estimateSize={132}
            itemKey={(project) => project.id}
            items={visibleProjects}
            renderItem={(project) => {
              const projectError = projectErrorById.get(project.id);
              return (
                <article className="list-row project-row" key={project.id}>
                  <div className="project-summary">
                    <div className="project-title-line">
                      <h2>{project.name}</h2>
                    </div>
                    <p>{project.remoteUrl || project.path}</p>
                  </div>
                  <div
                    className="project-insights"
                    aria-label={`Project summary for ${project.name}`}
                  >
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
                    <button
                      className="secondary-button"
                      disabled={operationBusy}
                      onClick={() => onCheckProject(project.id)}
                      type="button"
                    >
                      Check
                    </button>
                    <button
                      className="secondary-button"
                      disabled={operationBusy}
                      onClick={() => onPullProject(project.id, autostash)}
                      type="button"
                    >
                      Pull
                    </button>
                    <button
                      className="secondary-button"
                      disabled={rowActionBusy}
                      onClick={() => openProjectPath(project)}
                      type="button"
                    >
                      Open project
                    </button>
                    <button
                      className="secondary-button"
                      disabled={rowActionBusy || operationBusy}
                      onClick={() => toggleProjectHidden(project)}
                      type="button"
                    >
                      {project.hidden ? "Show" : "Hide"}
                    </button>
                    <button
                      className="secondary-button danger-button"
                      disabled={rowActionBusy || operationBusy}
                      onClick={() => confirmDeleteProject(project)}
                      type="button"
                    >
                      Delete
                    </button>
                  </div>
                </article>
              );
            }}
          />
        )}
      </section>
    </>
  );
});

export function ProjectImportDialog({
  directoryName,
  onCancel,
  onDirectoryNameChange,
  onShallowChange,
  onSkillPathChange,
  onSourceChange,
  onSubmit,
  shallow,
  skillPath,
  source,
}: {
  directoryName: string;
  onCancel: () => void;
  onDirectoryNameChange: (value: string) => void;
  onShallowChange: (value: boolean) => void;
  onSkillPathChange: (value: string) => void;
  onSourceChange: (value: string) => void;
  onSubmit: () => void;
  shallow: boolean;
  skillPath: string;
  source: string;
}) {
  return (
    <div className="dialog-backdrop" role="presentation">
      <form
        aria-modal="true"
        className="data-panel compact-form import-dialog"
        onSubmit={(event) => {
          event.preventDefault();
          onSubmit();
        }}
        role="dialog"
      >
        <PanelHeader title="Import repository" detail="GitHub shorthand or Git URL" />
        <label>
          <span>Repository</span>
          <input
            autoFocus
            onChange={(event) => onSourceChange(event.target.value)}
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
                onChange={(event) => onDirectoryNameChange(event.target.value)}
                placeholder="Optional, e.g. awesome-copilot"
                value={directoryName}
              />
            </label>
            <label>
              <span>Skill path</span>
              <input
                onChange={(event) => onSkillPathChange(event.target.value)}
                placeholder="Optional, e.g. skills/github-release"
                value={skillPath}
              />
            </label>
            <label className="inline-check">
              <input
                checked={shallow}
                onChange={(event) => onShallowChange(event.target.checked)}
                type="checkbox"
              />
              <span>Shallow clone</span>
            </label>
          </div>
        </details>
        <div className="panel-actions">
          <button className="primary-button" type="submit">
            Import
          </button>
          <button className="secondary-button" onClick={onCancel} type="button">
            Cancel
          </button>
        </div>
      </form>
    </div>
  );
}
