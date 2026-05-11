import { lazy, Suspense, useEffect, useRef, useState } from "react";
import {
  checkAllProjectUpdates,
  checkProjectUpdates,
  createAgentProfileDir,
  importProject,
  loadUserConfig,
  loadWorkspaceConfig,
  openWorkspacePath,
  pullAllProjects,
  pullProject,
  recentTaskRecords,
  restoreRecentWorkspace,
  saveWorkspaceConfig,
  scanWorkspace,
  selectWorkspace,
  setWindowTheme,
  setWindowTitle,
} from "./lib/commands";
import {
  applyThemePreference,
  errorMessage,
  isTerminalTaskStatus,
  mergeTaskRecords,
  StatusPanel,
  type ThemePreference,
  type ViewName,
  views,
} from "./lib/shared";
import type {
  AgentProfileState,
  BatchLinkOperationResult,
  TaskOperationResult,
  TaskRecord,
  Workspace,
  WorkspaceProjectMetadata,
} from "./lib/types";

const CoreViewLazy = lazy(() => import("./CoreView"));

// Task polling cadence. Slow fallback; real-time updates come via Tauri events.
const TASKS_IDLE_POLL_MS = 5000;
// Max number of recent task records to fetch per poll; matches backend cap behaviour.
const TASKS_RECENT_LIMIT = 80;

type LoadState =
  | { status: "loading"; message: string }
  | { status: "needs-workspace"; message?: string }
  | { status: "ready"; workspace: Workspace }
  | { status: "error"; message: string; workspace?: Workspace };

function loadStateWorkspaceRoot(state: LoadState) {
  if (state.status === "ready") return state.workspace.root;
  if (state.status === "error") return state.workspace?.root ?? null;
  return null;
}

function App() {
  const [activeView, setActiveView] = useState<ViewName>("Skills");
  const [themePreference, setThemePreference] = useState<ThemePreference>("dark");
  const [loadState, setLoadState] = useState<LoadState>({
    status: "loading",
    message: "Checking recent workspace",
  });
  const currentWorkspaceRootRef = useRef<string | null>(null);
  const [workspaceInput, setWorkspaceInput] = useState("");
  const [taskHistory, setTaskHistory] = useState<TaskRecord[]>([]);
  const [operationMessage, setOperationMessage] = useState<string | null>(null);
  const [operationBusy, setOperationBusy] = useState(false);
  const operationBusyRef = useRef(false);
  // Ref guards so rapid double-clicks on workspace-level buttons
  // (Refresh / Open workspace / Recent workspace switch) cannot spawn
  // overlapping scans before the loading state masks them.
  const refreshWorkspaceBusyRef = useRef(false);
  const chooseWorkspaceBusyRef = useRef(false);
  const workspaceRefreshTaskIdsRef = useRef(new Set<string>());
  const [focusedTaskId, setFocusedTaskId] = useState<string | null>(null);
  const readyWorkspace =
    loadState.status === "ready"
      ? loadState.workspace
      : loadState.status === "error"
        ? loadState.workspace
        : undefined;
  const workspacePath =
      readyWorkspace && readyWorkspace.root.length > 0 ? readyWorkspace.root : "No workspace selected";

  const setTrackedLoadState = (next: LoadState | ((current: LoadState) => LoadState)) => {
    setLoadState((current) => {
      const resolved = typeof next === "function" ? next(current) : next;
      currentWorkspaceRootRef.current = loadStateWorkspaceRoot(resolved);
      return resolved;
    });
  };

  const isCurrentWorkspaceRoot = (root: string) => currentWorkspaceRootRef.current === root;

  useEffect(() => {
    let cancelled = false;

    restoreRecentWorkspace()
      .then((workspace) => {
        if (cancelled) {
          return;
        }
        if (workspace) {
          setTrackedLoadState({ status: "ready", workspace });
        } else {
          setTrackedLoadState({ status: "needs-workspace" });
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setTrackedLoadState({
            status: "error",
            message: errorMessage(error),
          });
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;

    loadUserConfig()
      .then((loaded) => {
        if (!cancelled) {
          setThemePreference(loaded.uiPreferences.theme);
        }
      })
      .catch(() => {
        // The Vite-only smoke path has no Tauri bridge; keep the system default.
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    applyThemePreference(themePreference);
    // Mirror to the OS title bar so Windows dark mode and macOS appearance
    // follow the in-app preference. "system" maps to null (follow OS).
    const osTheme: "light" | "dark" | null =
      themePreference === "system" ? null : themePreference;
    void setWindowTheme(osTheme);
  }, [themePreference]);

  // Keep the OS window title in sync with the current workspace so multi-window /
  // taskbar / Dock entries are distinguishable. Falls back to the bare product name.
  useEffect(() => {
    const root = readyWorkspace?.root;
    const title = root && root.length > 0 ? `SkillDock — ${root}` : "SkillDock";
    void setWindowTitle(title);
  }, [readyWorkspace?.root]);

  useEffect(() => {
    if (!readyWorkspace) {
      return;
    }

    let cancelled = false;
    let timer: number | null = null;
    let unlisten: (() => void) | null = null;
    setTaskHistory([]);
    setFocusedTaskId(null);

    const scheduleNext = (delay: number) => {
      if (cancelled) {
        return;
      }
      timer = window.setTimeout(refreshRecentTasks, delay);
    };

    const refreshRecentTasks = async () => {
      try {
        const recent = await recentTaskRecords(readyWorkspace.root, TASKS_RECENT_LIMIT);
        if (cancelled) {
          return;
        }
        setTaskHistory((tasks) => mergeTaskRecords(recent, tasks));
        recent.forEach(refreshWorkspaceAfterTerminalProjectTask);
      } catch {
        // Task polling should not replace the primary operation error surface.
      }
      scheduleNext(TASKS_IDLE_POLL_MS);
    };

    // Initial load
    refreshRecentTasks();

    // Listen for real-time task updates from the backend
    void import("@tauri-apps/api/event").then(({ listen }) => {
      if (cancelled) return;
      listen<TaskRecord>("task-update", (event) => {
        if (cancelled) return;
        const record = event.payload;
        if (record.workspaceRoot && record.workspaceRoot !== readyWorkspace.root) return;
        setTaskHistory((tasks) => mergeTaskRecords([record], tasks));
        refreshWorkspaceAfterTerminalProjectTask(record);
      }).then((fn) => {
        unlisten = fn;
      }).catch(() => {
        // No Tauri bridge (Vite-only mode) — rely on polling
      });
    }).catch(() => {});

    return () => {
      cancelled = true;
      if (timer !== null) {
        window.clearTimeout(timer);
      }
      if (unlisten) {
        unlisten();
      }
    };
  }, [readyWorkspace?.root]);

  const chooseWorkspace = async () => {
    if (chooseWorkspaceBusyRef.current) return;
    const requestedPath = workspaceInput.trim();
    if (requestedPath.length === 0) {
      setTrackedLoadState({ status: "needs-workspace", message: "Enter a workspace directory path." });
      return;
    }

    chooseWorkspaceBusyRef.current = true;
    setTrackedLoadState({ status: "loading", message: "Opening workspace" });
    try {
      const workspace = await selectWorkspace(requestedPath);
      setTrackedLoadState({ status: "ready", workspace });
      setWorkspaceInput("");
      setOperationMessage(null);
    } catch (error) {
      setTrackedLoadState({
        status: "needs-workspace",
        message: errorMessage(error),
      });
    } finally {
      chooseWorkspaceBusyRef.current = false;
    }
  };

  const refreshWorkspace = async () => {
    if (!readyWorkspace) {
      return;
    }
    if (refreshWorkspaceBusyRef.current) {
      return;
    }

    refreshWorkspaceBusyRef.current = true;
    setTrackedLoadState({ status: "loading", message: "Scanning workspace" });
    try {
      const workspace = await scanWorkspace(readyWorkspace.root);
      setTrackedLoadState({ status: "ready", workspace });
      setOperationMessage("Workspace refreshed.");
    } catch (error) {
      setTrackedLoadState({
        status: "error",
        message: errorMessage(error),
        workspace: readyWorkspace,
      });
    } finally {
      refreshWorkspaceBusyRef.current = false;
    }
  };

  const applyOperationResult = (result: TaskOperationResult) => {
    if (!isCurrentWorkspaceRoot(result.workspace.root)) {
      return;
    }
    setTaskHistory((tasks) => mergeTaskRecords([result.task], tasks));
    setTrackedLoadState({ status: "ready", workspace: result.workspace });
    setOperationMessage(result.task.summary);
  };

  const applyWorkspaceChange = (workspace: Workspace, message: string) => {
    if (readyWorkspace?.root !== workspace.root) {
      setTaskHistory([]);
      setFocusedTaskId(null);
    }
    setTrackedLoadState({ status: "ready", workspace });
    setOperationMessage(message);
  };

  const updateTaskRecord = (record: TaskRecord) => {
    if (record.workspaceRoot && !isCurrentWorkspaceRoot(record.workspaceRoot)) {
      return;
    }
    setTaskHistory((tasks) => mergeTaskRecords([record], tasks));
    refreshWorkspaceAfterTerminalProjectTask(record);
  };

  const refreshWorkspaceAfterTerminalProjectTask = async (record: TaskRecord) => {
    if (
      !["fetch_project", "pull_project", "sync_all_projects"].includes(record.kind) ||
      !record.workspaceRoot ||
      !isTerminalTaskStatus(record.status) ||
      workspaceRefreshTaskIdsRef.current.has(record.id)
    ) {
      return;
    }

    workspaceRefreshTaskIdsRef.current.add(record.id);
    try {
      const workspace = await scanWorkspace(record.workspaceRoot);
      if (!isCurrentWorkspaceRoot(record.workspaceRoot)) {
        return;
      }
      setTrackedLoadState({ status: "ready", workspace });
      setOperationMessage(record.summary);
    } catch (error) {
      if (!isCurrentWorkspaceRoot(record.workspaceRoot)) {
        return;
      }
      setOperationMessage(errorMessage(error));
    }
  };

  const applyBatchLinkResult = (result: BatchLinkOperationResult) => {
    if (!isCurrentWorkspaceRoot(result.workspace.root)) {
      return;
    }
    setTaskHistory((tasks) => mergeTaskRecords([result.task], tasks));
    setTrackedLoadState({ status: "ready", workspace: result.workspace });
    setOperationMessage(
      `Batch link: ${result.summary.linked} linked, ${result.summary.alreadyInstalled} already installed, ${result.summary.skipped} skipped, ${result.summary.failed} failed.`,
    );
  };

  const runWorkspaceOperation = async (
    label: string,
    operation: (workspace: Workspace) => Promise<TaskOperationResult>,
  ) => {
    if (!readyWorkspace || operationBusyRef.current) {
      return;
    }
    const workspace = readyWorkspace;

    operationBusyRef.current = true;
    setOperationBusy(true);
    setOperationMessage(`${label}...`);
    try {
      applyOperationResult(await operation(workspace));
    } catch (error) {
      if (!isCurrentWorkspaceRoot(workspace.root)) {
        return;
      }
      setOperationMessage(errorMessage(error));
      setTrackedLoadState({ status: "error", message: errorMessage(error), workspace });
    } finally {
      operationBusyRef.current = false;
      setOperationBusy(false);
    }
  };

  const createMissingAgentDir = async (profile: AgentProfileState) => {
    if (!readyWorkspace) {
      return;
    }
    const workspace = readyWorkspace;

    const confirmed = window.confirm(
      `Create skills directory for ${profile.profile.name}?\n\n${profile.skillsDir}`,
    );
    if (!confirmed) {
      return;
    }

    setOperationMessage(`Creating ${profile.profile.name} directory...`);
    try {
      const nextWorkspace = await createAgentProfileDir(workspace.root, profile.profile.id, true);
      if (!isCurrentWorkspaceRoot(workspace.root)) {
        return;
      }
      setTrackedLoadState({ status: "ready", workspace: nextWorkspace });
      setOperationMessage(`${profile.profile.name} directory created.`);
    } catch (error) {
      if (!isCurrentWorkspaceRoot(workspace.root)) {
        return;
      }
      setOperationMessage(errorMessage(error));
      setTrackedLoadState({ status: "error", message: errorMessage(error), workspace });
    }
  };

  const setProjectHidden = async (projectId: string, hidden: boolean) => {
    if (!readyWorkspace) {
      return;
    }
    const workspace = readyWorkspace;

    setOperationMessage(`${hidden ? "Hiding" : "Showing"} ${projectId}...`);
    try {
      const config = await loadWorkspaceConfig(workspace.root);
      const project = workspace.projects.find((item) => item.id === projectId);
      const existing = config.projects.find((item) => item.projectId === projectId);
      const nextProject: WorkspaceProjectMetadata = {
        projectId,
        displayName: existing?.displayName,
        category: existing?.category ?? project?.category,
        favorite: existing?.favorite ?? project?.favorite ?? false,
        hidden,
        tags: existing?.tags ?? project?.tags ?? [],
        notes: existing?.notes ?? project?.notes,
        autoCheck: existing?.autoCheck,
        autoPull: existing?.autoPull,
      };
      await saveWorkspaceConfig(workspace.root, {
        ...config,
        projects: [...config.projects.filter((item) => item.projectId !== projectId), nextProject],
      });
      const nextWorkspace = await scanWorkspace(workspace.root);
      if (!isCurrentWorkspaceRoot(workspace.root)) {
        return;
      }
      setTrackedLoadState({ status: "ready", workspace: nextWorkspace });
      setOperationMessage(`${project?.name ?? projectId} is ${hidden ? "hidden" : "visible"}.`);
    } catch (error) {
      if (!isCurrentWorkspaceRoot(workspace.root)) {
        return;
      }
      setOperationMessage(errorMessage(error));
      setTrackedLoadState({ status: "error", message: errorMessage(error), workspace });
    }
  };

  return (
    <main className="app-shell">
      <aside className="sidebar" aria-label="Primary navigation">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">
            <img src="/app-icon.png" alt="" />
          </span>
          <span>SkillDock</span>
        </div>
        <nav className="nav-list">
          {views.map((view) => (
            <button
              className={view === activeView ? "nav-item active" : "nav-item"}
              aria-current={view === activeView ? "page" : undefined}
              key={view}
              onClick={() => setActiveView(view)}
              type="button"
            >
              {view}
            </button>
          ))}
        </nav>
      </aside>

      <section className="workspace">
        <header className="topbar">
          <div>
            <p className="eyebrow">Workspace</p>
            <h1>{activeView}</h1>
          </div>
          <div className="topbar-actions">
            {readyWorkspace && (
              <button className="secondary-button" onClick={refreshWorkspace} type="button">
                Refresh
              </button>
            )}
            <code className="workspace-path" title={workspacePath}>
              {workspacePath}
            </code>
          </div>
        </header>

        {operationMessage && <div className="operation-banner" role="status">{operationMessage}</div>}

        <section className="content" aria-live="polite">
          {loadState.status === "loading" && (
            <StatusPanel title="Loading" body={loadState.message} tone="neutral" />
          )}
          {loadState.status === "needs-workspace" && (
            <WorkspaceSelector
              message={loadState.message}
              onInputChange={setWorkspaceInput}
              onSubmit={chooseWorkspace}
              value={workspaceInput}
            />
          )}
          {loadState.status === "error" && (
            <StatusPanel title="Command error" body={loadState.message} tone="error" />
          )}
          {readyWorkspace && (
            <Suspense fallback={null}>
            <CoreViewLazy
              activeView={activeView}
              onCheckAll={() =>
                runWorkspaceOperation("Checking updates", (workspace) =>
                  checkAllProjectUpdates(workspace.root),
                )
              }
              onCheckProject={(projectId) =>
                runWorkspaceOperation(`Checking ${projectId}`, (workspace) =>
                  checkProjectUpdates(workspace.root, projectId),
                )
              }
              onImport={(source, directoryName, shallow) =>
                runWorkspaceOperation("Importing repository", (workspace) =>
                  importProject(workspace.root, {
                    source,
                    directoryName: directoryName || undefined,
                    shallow,
                  }),
                )
              }
              onPullAll={(autostash) =>
                runWorkspaceOperation("Pulling projects", (workspace) =>
                  pullAllProjects(workspace.root, {
                    autostash,
                    safeProjectIds: workspace.projects
                      .filter((project) => project.pullAllEligible)
                      .map((project) => project.id),
                  }),
                )
              }
              onPullProject={(projectId, autostash) =>
                runWorkspaceOperation(`Pulling ${projectId}`, (workspace) =>
                  pullProject(workspace.root, { projectId, autostash }),
                )
              }
              focusedTaskId={focusedTaskId}
              taskHistory={taskHistory}
              onOpenTaskLog={(taskId) => {
                setFocusedTaskId(taskId);
                setActiveView("Logs");
              }}
              onCreateAgentDir={createMissingAgentDir}
              onBatchLinkResult={applyBatchLinkResult}
              onOperationResult={applyOperationResult}
              onThemePreferenceChange={setThemePreference}
              onWorkspaceChange={applyWorkspaceChange}
              onTaskChange={updateTaskRecord}
              onSetProjectHidden={setProjectHidden}
              operationBusy={operationBusy}
              workspace={readyWorkspace}
            />
            </Suspense>
          )}
        </section>
      </section>
    </main>
  );
}

export function WorkspaceSelector({
  message,
  onInputChange,
  onSubmit,
  value,
}: {
  message?: string;
  onInputChange: (value: string) => void;
  onSubmit: () => void;
  value: string;
}) {
  return (
    <form
      className="workspace-selector"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit();
      }}
    >
      <div>
        <h2>Select workspace</h2>
        <p>Choose a collection directory to scan projects and skills.</p>
      </div>
      <label>
        <span>Workspace path</span>
        <input
          autoFocus
          onChange={(event) => onInputChange(event.target.value)}
          placeholder="/home/user/Skills-repo"
          value={value}
        />
      </label>
      {message && <p className="form-error">{message}</p>}
      <button className="primary-button" type="submit">
        Open workspace
      </button>
    </form>
  );
}

export default App;
