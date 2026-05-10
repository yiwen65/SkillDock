import { useEffect, useMemo, useRef, useState } from "react";
import {
  cancelTask,
  checkAllProjectUpdates,
  checkProjectUpdates,
  createAgentProfileDir,
  getTaskLogs,
  getTaskStatus,
  importProject,
  linkSkill,
  linkSkillsBatch,
  loadUserConfig,
  loadWorkspaceConfig,
  openWorkspacePath,
  patchUserPreferences,
  previewLinkSkill,
  previewLinkSkillsBatch,
  previewUnlinkSkill,
  pullAllProjects,
  pullProject,
  readSkillMarkdownPreview,
  recentTaskRecords,
  restoreRecentWorkspace,
  scanWorkspace,
  selectWorkspace,
  saveAgentProfiles,
  saveWorkspaceConfig,
  unlinkSkill,
} from "./lib/commands";
import type {
  AgentProfile,
  AgentProfileState,
  BatchLinkOperationResult,
  LinkPreview,
  Project,
  Skill,
  TaskOperationResult,
  TaskRecord,
  UnlinkPreview,
  UserConfig,
  Workspace,
  WorkspaceProjectMetadata,
} from "./lib/types";

const views = ["Skills", "Projects", "Agents", "Tasks / Logs", "Settings"] as const;

type ViewName = (typeof views)[number];

type LoadState =
  | { status: "loading"; message: string }
  | { status: "needs-workspace"; message?: string }
  | { status: "ready"; workspace: Workspace }
  | { status: "error"; message: string; workspace?: Workspace };

function App() {
  const [activeView, setActiveView] = useState<ViewName>("Skills");
  const [loadState, setLoadState] = useState<LoadState>({
    status: "loading",
    message: "Checking recent workspace",
  });
  const currentWorkspaceRootRef = useRef<string | null>(null);
  const [workspaceInput, setWorkspaceInput] = useState("");
  const [taskHistory, setTaskHistory] = useState<TaskRecord[]>([]);
  const [operationMessage, setOperationMessage] = useState<string | null>(null);
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
    if (!readyWorkspace) {
      return;
    }

    let cancelled = false;
    setTaskHistory([]);
    setFocusedTaskId(null);
    const refreshRecentTasks = async () => {
      try {
        const recent = await recentTaskRecords(readyWorkspace.root, 80);
        if (!cancelled) {
          setTaskHistory((tasks) => mergeTaskRecords(recent, tasks));
        }
      } catch {
        // Task polling should not replace the primary operation error surface.
      }
    };

    refreshRecentTasks();
    const interval = window.setInterval(refreshRecentTasks, 1500);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [readyWorkspace?.root]);

  const chooseWorkspace = async () => {
    const requestedPath = workspaceInput.trim();
    if (requestedPath.length === 0) {
      setTrackedLoadState({ status: "needs-workspace", message: "Enter a workspace directory path." });
      return;
    }

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
    }
  };

  const refreshWorkspace = async () => {
    if (!readyWorkspace) {
      return;
    }

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
    if (!readyWorkspace) {
      return;
    }
    const workspace = readyWorkspace;

    setOperationMessage(`${label}...`);
    try {
      applyOperationResult(await operation(workspace));
    } catch (error) {
      if (!isCurrentWorkspaceRoot(workspace.root)) {
        return;
      }
      setOperationMessage(errorMessage(error));
      setTrackedLoadState({ status: "error", message: errorMessage(error), workspace });
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
          <span className="brand-mark">SC</span>
          <span>Skills Collection</span>
        </div>
        <nav className="nav-list">
          {views.map((view) => (
            <button
              className={view === activeView ? "nav-item active" : "nav-item"}
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

        {operationMessage && <div className="operation-banner">{operationMessage}</div>}

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
            <CoreView
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
                setActiveView("Tasks / Logs");
              }}
              onCreateAgentDir={createMissingAgentDir}
              onBatchLinkResult={applyBatchLinkResult}
              onOperationResult={applyOperationResult}
              onWorkspaceChange={applyWorkspaceChange}
              onTaskChange={updateTaskRecord}
              onSetProjectHidden={setProjectHidden}
              workspace={readyWorkspace}
            />
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

export function CoreView({
  activeView,
  onCheckAll,
  onCheckProject,
  onImport,
  onCreateAgentDir,
  onBatchLinkResult,
  onOperationResult,
  onWorkspaceChange,
  onTaskChange,
  onOpenTaskLog,
  onPullAll,
  onPullProject,
  onSetProjectHidden,
  focusedTaskId,
  taskHistory,
  workspace,
}: {
  activeView: ViewName;
  onCheckAll: () => void;
  onCheckProject: (projectId: string) => void;
  onImport: (source: string, directoryName: string, shallow: boolean) => void;
  onCreateAgentDir: (profile: AgentProfileState) => void;
  onBatchLinkResult: (result: BatchLinkOperationResult) => void;
  onOperationResult: (result: TaskOperationResult) => void;
  onWorkspaceChange: (workspace: Workspace, message: string) => void;
  onTaskChange: (task: TaskRecord) => void;
  onOpenTaskLog: (taskId: string) => void;
  onPullAll: (autostash: boolean) => void;
  onPullProject: (projectId: string, autostash: boolean) => void;
  onSetProjectHidden: (projectId: string, hidden: boolean) => void;
  focusedTaskId: string | null;
  taskHistory: TaskRecord[];
  workspace: Workspace;
}) {
  return (
    <div className="view-stack">
      <WorkspaceMetrics workspace={workspace} />
      {activeView === "Skills" && (
        <SkillsView
          onBatchLinkResult={onBatchLinkResult}
          onOperationResult={onOperationResult}
          workspace={workspace}
        />
      )}
      {activeView === "Projects" && (
        <ProjectsView
          onCheckAll={onCheckAll}
          onCheckProject={onCheckProject}
          onImport={onImport}
          onOpenTaskLog={onOpenTaskLog}
          onPullAll={onPullAll}
          onPullProject={onPullProject}
          onSetProjectHidden={onSetProjectHidden}
          projects={workspace.projects}
          taskHistory={taskHistory}
          workspaceRoot={workspace.root}
        />
      )}
      {activeView === "Agents" && (
        <AgentsView
          onCreateAgentDir={onCreateAgentDir}
          onOperationResult={onOperationResult}
          onWorkspaceChange={onWorkspaceChange}
          workspace={workspace}
        />
      )}
      {activeView === "Tasks / Logs" && (
        <TasksView focusedTaskId={focusedTaskId} onTaskChange={onTaskChange} tasks={taskHistory} />
      )}
      {activeView === "Settings" && (
        <SettingsView onWorkspaceChange={onWorkspaceChange} workspace={workspace} />
      )}
    </div>
  );
}

function WorkspaceMetrics({ workspace }: { workspace: Workspace }) {
  const installedCount = useMemo(
    () => workspace.skills.reduce((count, skill) => count + skill.installedAgents.length, 0),
    [workspace.skills],
  );

  return (
    <div className="metric-strip">
      <SummaryMetric label="Projects" value={workspace.projects.length} />
      <SummaryMetric label="Skills" value={workspace.skills.length} />
      <SummaryMetric label="Agents" value={workspace.agentProfiles.length} />
      <SummaryMetric label="Installs" value={installedCount} />
    </div>
  );
}

function SkillsView({
  onBatchLinkResult,
  onOperationResult,
  workspace,
}: {
  onBatchLinkResult: (result: BatchLinkOperationResult) => void;
  onOperationResult: (result: TaskOperationResult) => void;
  workspace: Workspace;
}) {
  const { agentProfiles, projects, skills } = workspace;
  const [query, setQuery] = useState("");
  const [projectFilter, setProjectFilter] = useState("all");
  const [categoryFilter, setCategoryFilter] = useState("all");
  const [tagFilter, setTagFilter] = useState("all");
  const [agentFilter, setAgentFilter] = useState("all");
  const [selectedSkillId, setSelectedSkillId] = useState(skills[0]?.id ?? "");
  const [selectedSkillIds, setSelectedSkillIds] = useState<string[]>([]);
  const [selectedProfileIds, setSelectedProfileIds] = useState<string[]>([]);
  const [singleProfileId, setSingleProfileId] = useState(agentProfiles[0]?.profile.id ?? "");
  const [skillPreview, setSkillPreview] = useState("");
  const [skillPreviewMessage, setSkillPreviewMessage] = useState<string | null>(null);
  const [singleLinkPreview, setSingleLinkPreview] = useState<LinkPreview | null>(null);
  const [singleUnlinkPreview, setSingleUnlinkPreview] = useState<UnlinkPreview | null>(null);
  const [previews, setPreviews] = useState<LinkPreview[]>([]);
  const [detailMessage, setDetailMessage] = useState<string | null>(null);
  const [batchMessage, setBatchMessage] = useState<string | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [executeBusy, setExecuteBusy] = useState(false);
  const [singleBusy, setSingleBusy] = useState(false);
  const singleBusyRef = useRef(false);
  const previewRequestId = useRef(0);
  const markdownRequestId = useRef(0);
  const singlePreviewRequestId = useRef(0);

  const projectById = useMemo(
    () => new Map(projects.map((project) => [project.id, project])),
    [projects],
  );
  const selectedSkill = skills.find((skill) => skill.id === selectedSkillId) ?? skills[0];
  const selectedProject = selectedSkill ? projectById.get(selectedSkill.sourceProjectId) : undefined;
  const selectedInstalledProfiles = selectedSkill
    ? selectedSkill.installedAgents.map((install) => install.agentProfileId)
    : [];

  const busy = previewBusy || executeBusy || singleBusy;
  const selectedPairCount = selectedSkillIds.length * selectedProfileIds.length;
  const safePreviewCount = previews.filter((preview) => isSafeLinkPreview(preview.status)).length;
  const currentSingleInstallKey = useMemo(
    () => installPreviewKey(selectedSkill?.id ?? "", singleProfileId),
    [selectedSkill?.id, singleProfileId],
  );
  const installPreviewMatchesCurrent =
    Boolean(singleLinkPreview) &&
    isSafeLinkPreview(singleLinkPreview!.status) &&
    installPreviewKey(singleLinkPreview!.skillId, singleLinkPreview!.agentProfileId) ===
      currentSingleInstallKey;
  const unlinkPreviewMatchesCurrent =
    Boolean(singleUnlinkPreview) &&
    singleUnlinkPreview!.status === "will_unlink" &&
    Boolean(selectedSkill) &&
    unlinkPreviewMatchesSkill(singleUnlinkPreview!, selectedSkill!);
  const selectionKey = useMemo(
    () => batchSelectionKey(selectedSkillIds, selectedProfileIds),
    [selectedProfileIds, selectedSkillIds],
  );
  const workspaceKey = useMemo(() => batchWorkspaceKey(workspace), [workspace]);
  const latestSelectionKey = useRef(selectionKey);
  const latestWorkspaceKey = useRef(workspaceKey);
  const latestSingleInstallKey = useRef(currentSingleInstallKey);
  const latestSelectedSkillId = useRef(selectedSkill?.id ?? "");

  useEffect(() => {
    latestSelectionKey.current = selectionKey;
  }, [selectionKey]);

  useEffect(() => {
    latestSingleInstallKey.current = currentSingleInstallKey;
    singlePreviewRequestId.current += 1;
    setSingleLinkPreview(null);
    setDetailMessage(null);
  }, [currentSingleInstallKey]);

  useEffect(() => {
    latestSelectedSkillId.current = selectedSkill?.id ?? "";
    singlePreviewRequestId.current += 1;
    setSingleLinkPreview(null);
    setSingleUnlinkPreview(null);
    setDetailMessage(null);
  }, [selectedSkill?.id]);

  useEffect(() => {
    latestWorkspaceKey.current = workspaceKey;
    previewRequestId.current += 1;
    singlePreviewRequestId.current += 1;
    setPreviews([]);
    setSingleLinkPreview(null);
    setSingleUnlinkPreview(null);
    setBatchMessage(null);
    setDetailMessage(null);
    setPreviewBusy(false);
    if (selectedSkillId && !skills.some((skill) => skill.id === selectedSkillId)) {
      setSelectedSkillId(skills[0]?.id ?? "");
    } else if (!selectedSkillId && skills[0]) {
      setSelectedSkillId(skills[0].id);
    }
    if (singleProfileId && !agentProfiles.some((state) => state.profile.id === singleProfileId)) {
      setSingleProfileId(agentProfiles[0]?.profile.id ?? "");
    } else if (!singleProfileId && agentProfiles[0]) {
      setSingleProfileId(agentProfiles[0].profile.id);
    }
  }, [agentProfiles, singleProfileId, selectedSkillId, skills, workspaceKey]);

  useEffect(() => {
    if (!selectedSkill) {
      setSkillPreview("");
      setSkillPreviewMessage(null);
      return;
    }

    const requestId = markdownRequestId.current + 1;
    markdownRequestId.current = requestId;
    setSkillPreview("");
    setSkillPreviewMessage("Loading SKILL.md...");
    readSkillMarkdownPreview(workspace.root, selectedSkill.id, 16000)
      .then((preview) => {
        if (markdownRequestId.current !== requestId) {
          return;
        }
        setSkillPreview(preview.markdown);
        setSkillPreviewMessage(preview.truncated ? "Preview truncated at 16 KB." : null);
      })
      .catch((error) => {
        if (markdownRequestId.current === requestId) {
          setSkillPreviewMessage(errorMessage(error));
        }
      });
  }, [selectedSkill, workspace.root]);

  const projectOptions = useMemo(
    () => Array.from(new Set(skills.map((skill) => skill.sourceProjectId))).sort(),
    [skills],
  );
  const categoryOptions = useMemo(
    () =>
      Array.from(
        new Set(
          skills
            .map((skill) => projectById.get(skill.sourceProjectId)?.category)
            .filter((category): category is Project["category"] => Boolean(category)),
        ),
      ).sort(),
    [projectById, skills],
  );
  const tagOptions = useMemo(
    () =>
      Array.from(
        new Set(
          skills.flatMap((skill) => projectById.get(skill.sourceProjectId)?.tags ?? []),
        ),
      ).sort(),
    [projectById, skills],
  );
  const filteredSkills = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    return skills.filter((skill) => {
      const project = projectById.get(skill.sourceProjectId);
      const installed = skill.installedAgents.length > 0;
      if (projectFilter !== "all" && skill.sourceProjectId !== projectFilter) {
        return false;
      }
      if (categoryFilter !== "all" && project?.category !== categoryFilter) {
        return false;
      }
      if (tagFilter !== "all" && !(project?.tags ?? []).includes(tagFilter)) {
        return false;
      }
      if (agentFilter === "installed" && !installed) {
        return false;
      }
      if (agentFilter === "not-installed" && installed) {
        return false;
      }
      if (
        !["all", "installed", "not-installed"].includes(agentFilter) &&
        !skill.installedAgents.some((install) => install.status === agentFilter)
      ) {
        return false;
      }
      if (!normalizedQuery) {
        return true;
      }
      return [skill.name, skill.description, skill.relativePath, skill.absolutePath]
        .filter(Boolean)
        .some((value) => value!.toLowerCase().includes(normalizedQuery));
    });
  }, [agentFilter, categoryFilter, projectById, projectFilter, query, skills, tagFilter]);

  const toggleValue = (kind: "skill" | "profile", value: string) => {
    const selected = kind === "skill" ? selectedSkillIds : selectedProfileIds;
    const nextSelected = selected.includes(value)
      ? selected.filter((item) => item !== value)
      : [...selected, value];
    const nextSkillIds = kind === "skill" ? nextSelected : selectedSkillIds;
    const nextProfileIds = kind === "profile" ? nextSelected : selectedProfileIds;
    previewRequestId.current += 1;
    latestSelectionKey.current = batchSelectionKey(nextSkillIds, nextProfileIds);
    if (kind === "skill") {
      setSelectedSkillIds(nextSelected);
    } else {
      setSelectedProfileIds(nextSelected);
    }
    setPreviewBusy(false);
    setPreviews([]);
    setBatchMessage(null);
  };

  const selectVisibleSkills = () => {
    previewRequestId.current += 1;
    setSelectedSkillIds(filteredSkills.map((skill) => skill.id));
    setPreviews([]);
    setBatchMessage(null);
  };

  const clearSelection = () => {
    previewRequestId.current += 1;
    setSelectedSkillIds([]);
    setSelectedProfileIds([]);
    setPreviews([]);
    setBatchMessage(null);
  };

  const previewSingleInstall = async () => {
    if (singleBusyRef.current) {
      return;
    }
    if (!selectedSkill || !singleProfileId) {
      setDetailMessage("Select a skill and target profile.");
      return;
    }
    singleBusyRef.current = true;
    setSingleBusy(true);
    setSingleLinkPreview(null);
    setSingleUnlinkPreview(null);
    setDetailMessage("Building install preview...");
    const requestId = singlePreviewRequestId.current + 1;
    singlePreviewRequestId.current = requestId;
    const requestKey = currentSingleInstallKey;
    const requestWorkspaceKey = workspaceKey;
    try {
      const preview = await previewLinkSkill(workspace.root, {
        agentProfileId: singleProfileId,
        skillId: selectedSkill.id,
      });
      if (
        singlePreviewRequestId.current !== requestId ||
        latestSingleInstallKey.current !== requestKey ||
        latestWorkspaceKey.current !== requestWorkspaceKey
      ) {
        return;
      }
      setSingleLinkPreview(preview);
      setDetailMessage(`Install preview: ${preview.status}${preview.message ? ` - ${preview.message}` : ""}`);
    } catch (error) {
      if (
        singlePreviewRequestId.current === requestId &&
        latestSingleInstallKey.current === requestKey &&
        latestWorkspaceKey.current === requestWorkspaceKey
      ) {
        setDetailMessage(errorMessage(error));
      }
    } finally {
      singleBusyRef.current = false;
      setSingleBusy(false);
    }
  };

  const executeSingleInstall = async () => {
    if (singleBusyRef.current) {
      return;
    }
    if (!singleLinkPreview || !installPreviewMatchesCurrent) {
      return;
    }
    singleBusyRef.current = true;
    setSingleBusy(true);
    setDetailMessage("Installing skill...");
    try {
      const result = await linkSkill(workspace.root, { preview: singleLinkPreview });
      onOperationResult(result);
      setSingleLinkPreview(null);
      setDetailMessage(result.task.summary);
    } catch (error) {
      setDetailMessage(errorMessage(error));
    } finally {
      singleBusyRef.current = false;
      setSingleBusy(false);
    }
  };

  const previewSingleUninstall = async (agentProfileId: string, linkName: string) => {
    if (singleBusyRef.current) {
      return;
    }
    if (!selectedSkill) {
      return;
    }
    singleBusyRef.current = true;
    setSingleBusy(true);
    setSingleLinkPreview(null);
    setSingleUnlinkPreview(null);
    setDetailMessage("Building uninstall preview...");
    const requestId = singlePreviewRequestId.current + 1;
    singlePreviewRequestId.current = requestId;
    const requestSkillId = selectedSkill.id;
    const requestWorkspaceKey = workspaceKey;
    try {
      const preview = await previewUnlinkSkill(workspace.root, { agentProfileId, linkName });
      if (
        singlePreviewRequestId.current !== requestId ||
        latestSelectedSkillId.current !== requestSkillId ||
        latestWorkspaceKey.current !== requestWorkspaceKey ||
        !unlinkPreviewMatchesSkill(preview, selectedSkill)
      ) {
        return;
      }
      setSingleUnlinkPreview(preview);
      setDetailMessage(`Uninstall preview: ${preview.status}${preview.message ? ` - ${preview.message}` : ""}`);
    } catch (error) {
      if (
        singlePreviewRequestId.current === requestId &&
        latestSelectedSkillId.current === requestSkillId &&
        latestWorkspaceKey.current === requestWorkspaceKey
      ) {
        setDetailMessage(errorMessage(error));
      }
    } finally {
      singleBusyRef.current = false;
      setSingleBusy(false);
    }
  };

  const executeSingleUninstall = async () => {
    if (singleBusyRef.current) {
      return;
    }
    if (!singleUnlinkPreview || !unlinkPreviewMatchesCurrent) {
      return;
    }
    singleBusyRef.current = true;
    setSingleBusy(true);
    setDetailMessage("Uninstalling skill...");
    try {
      const result = await unlinkSkill(workspace.root, { preview: singleUnlinkPreview });
      onOperationResult(result);
      setSingleUnlinkPreview(null);
      setDetailMessage(result.task.summary);
    } catch (error) {
      setDetailMessage(errorMessage(error));
    } finally {
      singleBusyRef.current = false;
      setSingleBusy(false);
    }
  };

  const openPath = async (path: string, label: string) => {
    try {
      await openWorkspacePath(workspace.root, path);
      setDetailMessage(`Opening ${label}.`);
    } catch (error) {
      setDetailMessage(errorMessage(error));
    }
  };

  const copyPath = async (path: string) => {
    try {
      await navigator.clipboard.writeText(path);
      setDetailMessage("Path copied.");
    } catch {
      window.prompt("Copy path", path);
      setDetailMessage("Copy path fallback opened.");
    }
  };

  const previewBatch = async () => {
    if (selectedPairCount === 0) {
      setBatchMessage("Select at least one skill and one target profile.");
      return;
    }

    setPreviewBusy(true);
    setBatchMessage("Building preview...");
    const requestId = previewRequestId.current + 1;
    previewRequestId.current = requestId;
    const requestSelectionKey = selectionKey;
    const requestWorkspaceKey = workspaceKey;
    const skillIds = [...selectedSkillIds];
    const profileIds = [...selectedProfileIds];
    try {
      const result = await previewLinkSkillsBatch(workspace.root, {
        items: skillIds.flatMap((skillId) =>
          profileIds.map((agentProfileId) => ({ skillId, agentProfileId })),
        ),
      });
      if (
        previewRequestId.current !== requestId ||
        latestSelectionKey.current !== requestSelectionKey ||
        latestWorkspaceKey.current !== requestWorkspaceKey
      ) {
        return;
      }
      setPreviews(result.previews);
      setBatchMessage(`${result.previews.length} link targets previewed.`);
    } catch (error) {
      if (
        previewRequestId.current !== requestId ||
        latestSelectionKey.current !== requestSelectionKey ||
        latestWorkspaceKey.current !== requestWorkspaceKey
      ) {
        return;
      }
      setBatchMessage(errorMessage(error));
    } finally {
      if (previewRequestId.current === requestId) {
        setPreviewBusy(false);
      }
    }
  };

  const executeBatch = async () => {
    if (previews.length === 0) {
      return;
    }

    setExecuteBusy(true);
    setBatchMessage("Executing batch...");
    try {
      const result = await linkSkillsBatch(workspace.root, { previews });
      onBatchLinkResult(result);
      setPreviews([]);
      setBatchMessage(
        `${result.summary.linked} linked, ${result.summary.alreadyInstalled} already installed, ${result.summary.skipped} skipped, ${result.summary.failed} failed.`,
      );
    } catch (error) {
      setBatchMessage(errorMessage(error));
    } finally {
      setExecuteBusy(false);
    }
  };

  if (skills.length === 0) {
    return <EmptyState title="No skills found" body="Refresh after adding projects with SKILL.md files." />;
  }

  return (
    <section className="skills-view-grid">
      <section className="data-panel">
        <PanelHeader title="Skills" detail={`${filteredSkills.length} of ${skills.length}`} />
        <div className="skill-filter-grid">
          <label>
            <span>Search</span>
            <input
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Name, description or path"
              value={query}
            />
          </label>
          <label>
            <span>Project</span>
            <select onChange={(event) => setProjectFilter(event.target.value)} value={projectFilter}>
              <option value="all">All projects</option>
              {projectOptions.map((projectId) => (
                <option key={projectId} value={projectId}>
                  {projectById.get(projectId)?.name ?? projectId}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>Category</span>
            <select onChange={(event) => setCategoryFilter(event.target.value)} value={categoryFilter}>
              <option value="all">All categories</option>
              {categoryOptions.map((category) => (
                <option key={category} value={category}>
                  {category}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>Tag</span>
            <select onChange={(event) => setTagFilter(event.target.value)} value={tagFilter}>
              <option value="all">All tags</option>
              {tagOptions.map((tag) => (
                <option key={tag} value={tag}>
                  {tag}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>Install status</span>
            <select onChange={(event) => setAgentFilter(event.target.value)} value={agentFilter}>
              <option value="all">All statuses</option>
              <option value="installed">Installed</option>
              <option value="not-installed">Not installed</option>
              <option value="valid">Valid</option>
              <option value="broken">Broken</option>
              <option value="external">External</option>
              <option value="conflict">Conflict</option>
            </select>
          </label>
        </div>
        <div className="table-list skill-list">
          {filteredSkills.map((skill) => {
            const project = projectById.get(skill.sourceProjectId);
            return (
              <article
                className={skill.id === selectedSkill?.id ? "list-row skill-row selected" : "list-row skill-row"}
                key={skill.id}
                onClick={() => setSelectedSkillId(skill.id)}
              >
                <label className="inline-check" onClick={(event) => event.stopPropagation()}>
                  <input
                    checked={selectedSkillIds.includes(skill.id)}
                    onChange={() => toggleValue("skill", skill.id)}
                    type="checkbox"
                  />
                </label>
                <div>
                  <h2>{skill.name}</h2>
                  <p>{skill.description || skill.relativePath}</p>
                </div>
                <div className="row-tags">
                  <span>{project?.name ?? skill.sourceProjectId}</span>
                  {project?.category && <span>{project.category}</span>}
                  {project?.tags.map((tag) => <span key={tag}>{tag}</span>)}
                  {skill.installedAgents.length > 0 && <span>{skill.installedAgents.length} installed</span>}
                </div>
              </article>
            );
          })}
          {filteredSkills.length === 0 && <p className="batch-message">No skills match the current filters.</p>}
        </div>
      </section>

      <section className="data-panel skill-detail-panel">
        <PanelHeader title={selectedSkill?.name ?? "Skill detail"} detail={selectedSkill?.relativePath ?? ""} />
        {selectedSkill && (
          <>
            <div className="detail-meta">
              <div>
                <span>Source project</span>
                <strong>{selectedProject?.name ?? selectedSkill.sourceProjectId}</strong>
              </div>
              <div>
                <span>Relative path</span>
                <strong>{selectedSkill.relativePath}</strong>
              </div>
              <div>
                <span>Installed agents</span>
                <strong>{selectedInstalledProfiles.length || "None"}</strong>
              </div>
              <div>
                <span>Contents</span>
                <strong>
                  {[selectedSkill.hasAssets && "assets", selectedSkill.hasScripts && "scripts", selectedSkill.hasReferences && "references"]
                    .filter(Boolean)
                    .join(", ") || "SKILL.md only"}
                </strong>
              </div>
            </div>
            {selectedSkill.description && <p className="detail-description">{selectedSkill.description}</p>}
            <div className="panel-actions">
              {selectedProject?.readmeFile && (
                <button
                  className="secondary-button"
                  onClick={() =>
                    openPath(joinPath(selectedProject.path, selectedProject.readmeFile!), "README")
                  }
                  type="button"
                >
                  Open README
                </button>
              )}
              {selectedProject?.path && (
                <button
                  className="secondary-button"
                  onClick={() => openPath(selectedProject.path, "project directory")}
                  type="button"
                >
                  Open project
                </button>
              )}
              <button
                className="secondary-button"
                onClick={() => copyPath(selectedSkill.absolutePath)}
                type="button"
              >
                Copy path
              </button>
            </div>
            <div className="single-action-grid">
              <label>
                <span>Install target</span>
                <select onChange={(event) => setSingleProfileId(event.target.value)} value={singleProfileId}>
                  {agentProfiles.map((state) => (
                    <option key={state.profile.id} value={state.profile.id}>
                      {state.profile.name}
                    </option>
                  ))}
                </select>
              </label>
              <button
                className="secondary-button"
                disabled={busy || !singleProfileId}
                onClick={previewSingleInstall}
                type="button"
              >
                Preview install
              </button>
              <button
                className="primary-button"
                disabled={busy || !installPreviewMatchesCurrent}
                onClick={executeSingleInstall}
                type="button"
              >
                Install
              </button>
            </div>
            {detailMessage && <p className="batch-message">{detailMessage}</p>}
            {singleLinkPreview && (
              <div className="preview-row">
                <span>{skillName(skills, singleLinkPreview.skillId)}</span>
                <span>{profileName(agentProfiles, singleLinkPreview.agentProfileId)}</span>
                <strong className={isSafeLinkPreview(singleLinkPreview.status) ? "status-safe" : "status-blocked"}>
                  {singleLinkPreview.status}
                </strong>
              </div>
            )}
            <div className="installed-list">
              <h2>Installed agents</h2>
              {selectedSkill.installedAgents.map((install) => (
                <div className="installed-row" key={`${install.agentProfileId}:${install.linkName}`}>
                  <div>
                    <strong>{profileName(agentProfiles, install.agentProfileId)}</strong>
                    <span>{install.linkName}</span>
                  </div>
                  <span className="subtle-pill">{install.status}</span>
                  <button
                    className="secondary-button"
                    disabled={busy}
                    onClick={() => previewSingleUninstall(install.agentProfileId, install.linkName)}
                    type="button"
                  >
                    Preview uninstall
                  </button>
                </div>
              ))}
              {selectedSkill.installedAgents.length === 0 && (
                <p className="batch-message">This skill is not installed in any agent profile.</p>
              )}
              {singleUnlinkPreview && (
                <div className="preview-row">
                  <span>{profileName(agentProfiles, singleUnlinkPreview.agentProfileId)}</span>
                  <span>{singleUnlinkPreview.linkName}</span>
                  <strong className={singleUnlinkPreview.status === "will_unlink" ? "status-safe" : "status-blocked"}>
                    {singleUnlinkPreview.status}
                  </strong>
                </div>
              )}
              <button
                className="primary-button"
                disabled={busy || !unlinkPreviewMatchesCurrent}
                onClick={executeSingleUninstall}
                type="button"
              >
                Uninstall previewed
              </button>
            </div>
            <div className="skill-preview-box">
              <div className="panel-header">
                <h2>SKILL.md</h2>
                {skillPreviewMessage && <span>{skillPreviewMessage}</span>}
              </div>
              <pre>{skillPreview || "No preview available."}</pre>
            </div>
          </>
        )}
      </section>

      <section className="data-panel batch-panel">
        <PanelHeader title="Batch install" detail={`${selectedPairCount} targets selected`} />
        <div className="batch-picker">
          <div>
            <h2>Selection</h2>
            <div className="panel-actions">
              <button className="secondary-button" onClick={selectVisibleSkills} type="button">
                Select visible
              </button>
              <button className="secondary-button" onClick={clearSelection} type="button">
                Clear
              </button>
            </div>
            <p className="batch-message">{selectedSkillIds.length} skills selected.</p>
          </div>
          <div>
            <h2>Targets</h2>
            <div className="check-list">
              {agentProfiles.map((state) => (
                <label className="inline-check" key={state.profile.id}>
                  <input
                    checked={selectedProfileIds.includes(state.profile.id)}
                    onChange={() => toggleValue("profile", state.profile.id)}
                    type="checkbox"
                  />
                  <span>{state.profile.name}</span>
                </label>
              ))}
            </div>
          </div>
        </div>
        <div className="panel-actions">
          <button
            className="secondary-button"
            disabled={busy || selectedPairCount === 0}
            onClick={previewBatch}
            type="button"
          >
            Preview
          </button>
          <button
            className="primary-button"
            disabled={busy || previews.length === 0 || safePreviewCount === 0}
            onClick={executeBatch}
            type="button"
          >
            Execute safe
          </button>
        </div>
        {batchMessage && <p className="batch-message">{batchMessage}</p>}
        {previews.length > 0 && (
          <div className="preview-list">
            {previews.map((preview) => (
              <div className="preview-row" key={`${preview.skillId}:${preview.agentProfileId}:${preview.linkName}`}>
                <span>{skillName(skills, preview.skillId)}</span>
                <span>{profileName(agentProfiles, preview.agentProfileId)}</span>
                <strong className={isSafeLinkPreview(preview.status) ? "status-safe" : "status-blocked"}>
                  {preview.status}
                </strong>
              </div>
            ))}
          </div>
        )}
      </section>
    </section>
  );
}

function ProjectsView({
  onCheckAll,
  onCheckProject,
  onImport,
  onOpenTaskLog,
  onPullAll,
  onPullProject,
  onSetProjectHidden,
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

  const visibleProjects = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    return projects.filter((project) => {
      if (hiddenFilter === "visible" && project.hidden) {
        return false;
      }
      if (hiddenFilter === "hidden" && !project.hidden) {
        return false;
      }
      if (statusFilter !== "all" && project.gitStatus !== statusFilter) {
        return false;
      }
      if (!normalizedQuery) {
        return true;
      }
      return [project.name, project.id, project.remoteUrl, project.branch, project.upstream]
        .filter(Boolean)
        .some((value) => value!.toLowerCase().includes(normalizedQuery));
    });
  }, [hiddenFilter, projects, query, statusFilter]);

  const statusOptions = useMemo(
    () => Array.from(new Set(projects.map((project) => project.gitStatus))).sort(),
    [projects],
  );

  const submitImport = () => {
    if (!source.trim()) {
      return;
    }
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
              <button className="primary-button" type="submit">
                Import
              </button>
              <button className="secondary-button" onClick={() => setImportOpen(false)} type="button">
                Cancel
              </button>
            </div>
          </form>
        </div>
      )}
      <section className="data-panel">
        <PanelHeader title="Projects" detail={`${visibleProjects.length} of ${projects.length} repositories`} />
        <div className="panel-actions">
          <button className="primary-button" onClick={() => setImportOpen(true)} type="button">
            Import
          </button>
          <button className="secondary-button" onClick={onCheckAll} type="button">
            Check all
          </button>
          <button className="secondary-button" onClick={() => onPullAll(autostash)} type="button">
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
              onChange={(event) => setStatusFilter(event.target.value as "all" | Project["gitStatus"])}
              value={statusFilter}
            >
              <option value="all">All statuses</option>
              {statusOptions.map((status) => (
                <option key={status} value={status}>
                  {statusLabel(status)}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>Visibility</span>
            <select
              onChange={(event) => setHiddenFilter(event.target.value as "visible" | "all" | "hidden")}
              value={hiddenFilter}
            >
              <option value="visible">Visible projects</option>
              <option value="all">All projects</option>
              <option value="hidden">Hidden only</option>
            </select>
          </label>
        </div>
        {projects.length === 0 ? (
          <EmptyState title="No projects found" body="Import or refresh after adding top-level Git repositories." />
        ) : visibleProjects.length === 0 ? (
          <EmptyState title="No matching projects" body="Adjust filters to show more repositories." />
        ) : (
          <div className="table-list">
            {visibleProjects.map((project) => {
              const projectError = latestProjectError(project, taskHistory, workspaceRoot);
              return (
                <article className="list-row project-row" key={project.id}>
                  <div>
                    <div className="project-title-line">
                      <h2>{project.name}</h2>
                      {project.hidden && <span className="subtle-pill">hidden</span>}
                    </div>
                    <p>{project.remoteUrl || project.path}</p>
                    <p>{project.readmeSummary || "No README summary"}</p>
                    {projectError && (
                      <button
                        className="text-button error-link"
                        onClick={() => onOpenTaskLog(projectError.task.id)}
                        type="button"
                      >
                        {projectError.outcome.error || projectError.outcome.summary}
                      </button>
                    )}
                  </div>
                  <div className="project-meta">
                    <span>{statusLabel(project.gitStatus)}</span>
                    <span>{project.branch || "detached"}</span>
                    <span>{project.upstream || "no upstream"}</span>
                    <span>{divergenceLabel(project)}</span>
                    <span>{project.gitStatus === "dirty" ? "dirty" : "clean"}</span>
                    <span>{project.licenseFile ? `license: ${project.licenseFile}` : "no license"}</span>
                    <span>{project.skillCount} skills</span>
                  </div>
                  <div className="row-actions">
                    <button className="secondary-button" onClick={() => onCheckProject(project.id)} type="button">
                      Check
                    </button>
                    <button
                      className="secondary-button"
                      onClick={() => onPullProject(project.id, autostash)}
                      type="button"
                    >
                      Pull
                    </button>
                    <button
                      className="secondary-button"
                      onClick={() => onSetProjectHidden(project.id, !project.hidden)}
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

function AgentsView({
  onCreateAgentDir,
  onOperationResult,
  onWorkspaceChange,
  workspace,
}: {
  onCreateAgentDir: (profile: AgentProfileState) => Promise<void> | void;
  onOperationResult: (result: TaskOperationResult) => void;
  onWorkspaceChange: (workspace: Workspace, message: string) => void;
  workspace: Workspace;
}) {
  const profiles = workspace.agentProfiles;
  const [draftProfiles, setDraftProfiles] = useState<AgentProfile[]>(
    profiles.map((state) => state.profile),
  );
  const [editingId, setEditingId] = useState<string | null>(null);
  const [formProfile, setFormProfile] = useState<AgentProfile>(() =>
    emptyCustomProfile(draftProfiles),
  );
  const [profileMessage, setProfileMessage] = useState<string | null>(null);
  const [agentsBusy, setAgentsBusy] = useState(false);
  const agentsBusyRef = useRef(false);
  const [unlinkPreview, setUnlinkPreview] = useState<AgentUnlinkPreview | null>(null);

  useEffect(() => {
    const nextProfiles = profiles.map((state) => state.profile);
    setDraftProfiles(nextProfiles);
    setEditingId(null);
    setFormProfile(emptyCustomProfile(nextProfiles));
    setProfileMessage(null);
    setUnlinkPreview(null);
  }, [profiles, workspace.root]);

  const linkedSkillsByProfile = useMemo(() => {
    const grouped = new Map<string, LinkedProfileSkill[]>();
    for (const skill of workspace.skills) {
      for (const install of skill.installedAgents) {
        const links = grouped.get(install.agentProfileId) ?? [];
        links.push({
          linkName: install.linkName,
          skillId: skill.id,
          skillName: skill.name,
          sourcePath: install.sourcePath,
          status: install.status,
          targetPath: install.targetPath,
        });
        grouped.set(install.agentProfileId, links);
      }
    }

    for (const links of grouped.values()) {
      links.sort((left, right) => left.skillName.localeCompare(right.skillName));
    }

    return grouped;
  }, [workspace.skills]);

  const startAddProfile = () => {
    if (agentsBusyRef.current) {
      return;
    }
    setEditingId(null);
    setFormProfile(emptyCustomProfile(draftProfiles));
    setProfileMessage(null);
  };

  const startEditProfile = (profile: AgentProfile) => {
    if (agentsBusyRef.current) {
      return;
    }
    setEditingId(profile.id);
    setFormProfile({ ...profile });
    setProfileMessage(null);
  };

  const persistProfiles = async (nextProfiles: AgentProfile[], message: string) => {
    if (agentsBusyRef.current) {
      return;
    }
    const validationError = validateProfileDrafts(nextProfiles);
    if (validationError) {
      setProfileMessage(validationError);
      return;
    }

    agentsBusyRef.current = true;
    setAgentsBusy(true);
    setProfileMessage("Saving profiles...");
    try {
      await saveAgentProfiles(nextProfiles);
      const nextWorkspace = await scanWorkspace(workspace.root);
      setDraftProfiles(nextProfiles);
      setEditingId(null);
      setFormProfile(emptyCustomProfile(nextProfiles));
      setUnlinkPreview(null);
      setProfileMessage(message);
      onWorkspaceChange(nextWorkspace, message);
    } catch (error) {
      setProfileMessage(errorMessage(error));
    } finally {
      agentsBusyRef.current = false;
      setAgentsBusy(false);
    }
  };

  const saveProfileForm = async () => {
    const normalized: AgentProfile = {
      ...formProfile,
      id: formProfile.id,
      name: formProfile.name.trim(),
      skillsDir: formProfile.skillsDir.trim(),
    };
    const nextProfiles = editingId
      ? draftProfiles.map((profile) => (profile.id === editingId ? normalized : profile))
      : [...draftProfiles, normalized];
    await persistProfiles(nextProfiles, `${normalized.name} saved.`);
  };

  const toggleProfile = async (profile: AgentProfile) => {
    const nextProfile = { ...profile, enabled: !profile.enabled };
    await persistProfiles(
      draftProfiles.map((item) => (item.id === profile.id ? nextProfile : item)),
      `${profile.name} ${nextProfile.enabled ? "enabled" : "disabled"}.`,
    );
  };

  const previewLinkedSkillUninstall = async (profile: AgentProfileState, link: LinkedProfileSkill) => {
    if (agentsBusyRef.current) {
      return;
    }
    const rowKey = linkedSkillKey(profile.profile.id, link.linkName);
    agentsBusyRef.current = true;
    setAgentsBusy(true);
    setUnlinkPreview(null);
    setProfileMessage(`Previewing uninstall for ${link.skillName}...`);
    try {
      const preview = await previewUnlinkSkill(workspace.root, {
        agentProfileId: profile.profile.id,
        linkName: link.linkName,
      });
      const previewMatchesRow =
        preview.agentProfileId === profile.profile.id &&
        preview.linkName === link.linkName &&
        preview.sourcePath === link.sourcePath;
      if (!previewMatchesRow) {
        setProfileMessage("Uninstall preview no longer matches the selected linked skill.");
        return;
      }
      setUnlinkPreview({
        key: rowKey,
        preview,
        skillName: link.skillName,
        sourcePath: link.sourcePath,
      });
      setProfileMessage(`Uninstall preview: ${preview.status}${preview.message ? ` - ${preview.message}` : ""}`);
    } catch (error) {
      setProfileMessage(errorMessage(error));
    } finally {
      agentsBusyRef.current = false;
      setAgentsBusy(false);
    }
  };

  const executeLinkedSkillUninstall = async (previewToExecute: AgentUnlinkPreview) => {
    if (agentsBusyRef.current || previewToExecute.preview.status !== "will_unlink") {
      return;
    }
    agentsBusyRef.current = true;
    setAgentsBusy(true);
    setProfileMessage(`Uninstalling ${previewToExecute.skillName}...`);
    try {
      const result = await unlinkSkill(workspace.root, { preview: previewToExecute.preview });
      onOperationResult(result);
      setUnlinkPreview(null);
      setProfileMessage(result.task.summary);
    } catch (error) {
      setProfileMessage(errorMessage(error));
    } finally {
      agentsBusyRef.current = false;
      setAgentsBusy(false);
    }
  };

  const createProfileDir = async (profile: AgentProfileState) => {
    if (agentsBusyRef.current) {
      return;
    }
    agentsBusyRef.current = true;
    setAgentsBusy(true);
    try {
      await onCreateAgentDir(profile);
    } finally {
      agentsBusyRef.current = false;
      setAgentsBusy(false);
    }
  };

  return (
    <section className="data-panel agents-panel">
      <PanelHeader
        title="Agents"
        detail={`${profiles.length} profiles / ${workspace.skills.reduce(
          (count, skill) => count + skill.installedAgents.length,
          0,
        )} workspace links`}
      />
      <div className="agents-layout">
        <div className="table-list">
          {profiles.length === 0 && (
            <EmptyState title="No agent profiles" body="Add a custom profile to start linking skills." />
          )}
          {profiles.map((state) => {
            const linkedSkills = linkedSkillsByProfile.get(state.profile.id) ?? [];
            return (
              <article className="list-row agent-row" key={state.profile.id}>
                <div className="agent-main">
                  <div className="agent-title-line">
                    <h2>{state.profile.name}</h2>
                    <span className="subtle-pill">{state.profile.builtIn ? "Built-in" : "Custom"}</span>
                    <span className={state.profile.enabled ? "subtle-pill" : "subtle-pill muted-pill"}>
                      {state.profile.enabled ? "Enabled" : "Disabled"}
                    </span>
                  </div>
                  <p>{state.skillsDir}</p>
                  <div className="project-meta agent-meta">
                    <span>{state.exists ? "exists" : "missing"}</span>
                    <span>{state.writable ? "writable" : "not writable"}</span>
                    <span>{state.workspaceLinkCount} workspace links</span>
                    <span>{state.symlinkCount} symlinks</span>
                    <span>{state.entries.length} entries</span>
                  </div>
                  <div className="agent-linked-list">
                    {linkedSkills.length === 0 && <p>No workspace skills linked.</p>}
                    {linkedSkills.map((link) => (
                      <div className="agent-linked-row" key={linkedSkillKey(state.profile.id, link.linkName)}>
                        <div>
                          <strong>{link.skillName}</strong>
                          <span>{link.linkName}</span>
                        </div>
                        <span>{link.status}</span>
                        <button
                          className="secondary-button"
                          disabled={agentsBusy}
                          onClick={() => previewLinkedSkillUninstall(state, link)}
                          type="button"
                        >
                          Preview uninstall
                        </button>
                        {unlinkPreview?.key === linkedSkillKey(state.profile.id, link.linkName) && (
                          <div className="agent-unlink-preview">
                            <span>
                              {unlinkPreview.preview.status}
                              {unlinkPreview.preview.message ? ` - ${unlinkPreview.preview.message}` : ""}
                            </span>
                            <button
                              className="secondary-button"
                              disabled={agentsBusy || unlinkPreview.preview.status !== "will_unlink"}
                              onClick={() => executeLinkedSkillUninstall(unlinkPreview)}
                              type="button"
                            >
                              Execute uninstall
                            </button>
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                </div>
                <div className="row-actions agent-actions">
                  {!state.exists && (
                    <button
                      className="secondary-button"
                      disabled={agentsBusy}
                      onClick={() => createProfileDir(state)}
                      type="button"
                    >
                      Create directory
                    </button>
                  )}
                  <button
                    className="secondary-button"
                    disabled={agentsBusy}
                    onClick={() => toggleProfile(state.profile)}
                    type="button"
                  >
                    {state.profile.enabled ? "Disable" : "Enable"}
                  </button>
                  {!state.profile.builtIn && (
                    <button
                      className="secondary-button"
                      disabled={agentsBusy}
                      onClick={() => startEditProfile(state.profile)}
                      type="button"
                    >
                      Edit
                    </button>
                  )}
                </div>
              </article>
            );
          })}
        </div>
        <form
          className="compact-form agent-editor"
          onSubmit={(event) => {
            event.preventDefault();
            saveProfileForm();
          }}
        >
          <div className="form-title-row">
            <h2>{editingId ? "Edit custom profile" : "Add custom profile"}</h2>
            <button className="secondary-button" disabled={agentsBusy} onClick={startAddProfile} type="button">
              New
            </button>
          </div>
          <label>
            <span>Profile id</span>
            <input
              disabled={Boolean(editingId) || agentsBusy}
              readOnly={agentsBusy}
              onChange={(event) => setFormProfile({ ...formProfile, id: event.target.value })}
              placeholder="my-agent"
              value={formProfile.id}
            />
          </label>
          <label>
            <span>Name</span>
            <input
              onChange={(event) => setFormProfile({ ...formProfile, name: event.target.value })}
              placeholder="My Agent"
              value={formProfile.name}
              readOnly={agentsBusy}
            />
          </label>
          <label>
            <span>Skills directory</span>
            <input
              onChange={(event) => setFormProfile({ ...formProfile, skillsDir: event.target.value })}
              placeholder="~/.my-agent/skills"
              value={formProfile.skillsDir}
              readOnly={agentsBusy}
            />
          </label>
          <label className="inline-check">
            <input
              checked={formProfile.enabled}
              disabled={agentsBusy}
              onChange={(event) => setFormProfile({ ...formProfile, enabled: event.target.checked })}
              type="checkbox"
            />
            <span>Enabled</span>
          </label>
          <button className="primary-button" disabled={agentsBusy} type="submit">
            Save profile
          </button>
          {profileMessage && <p className="form-error">{profileMessage}</p>}
        </form>
      </div>
    </section>
  );
}

type LinkedProfileSkill = {
  linkName: string;
  skillId: string;
  skillName: string;
  sourcePath: string;
  status: Skill["installedAgents"][number]["status"];
  targetPath: string;
};

type AgentUnlinkPreview = {
  key: string;
  preview: UnlinkPreview;
  skillName: string;
  sourcePath: string;
};

function emptyCustomProfile(existingProfiles: AgentProfile[]): AgentProfile {
  let suffix = existingProfiles.length + 1;
  const existingIds = new Set(existingProfiles.map((profile) => profile.id));
  const existingSkillsDirs = new Set(
    existingProfiles.map((profile) => normalizeProfilePathForCompare(profile.skillsDir)),
  );
  let id = `custom-agent-${suffix}`;
  let skillsDir = `~/skills/${id}`;
  while (
    existingIds.has(id) ||
    existingSkillsDirs.has(normalizeProfilePathForCompare(skillsDir))
  ) {
    suffix += 1;
    id = `custom-agent-${suffix}`;
    skillsDir = `~/skills/${id}`;
  }

  return {
    id,
    name: `Custom Agent ${suffix}`,
    skillsDir,
    enabled: true,
    builtIn: false,
    linkMode: "symlink",
  };
}

function validateProfileDrafts(profiles: AgentProfile[]) {
  const ids = new Set<string>();
  const skillsDirs = new Set<string>();
  for (const profile of profiles) {
    const id = profile.id.trim();
    if (!id) {
      return "Profile id is required.";
    }
    if (profile.id !== id) {
      return `Profile id '${id}' must not contain leading or trailing whitespace.`;
    }
    if (!/^[a-zA-Z0-9._-]+$/.test(id)) {
      return `Profile id '${id}' may only use letters, numbers, dots, underscores and hyphens.`;
    }
    if (ids.has(id)) {
      return `Profile id '${id}' is duplicated.`;
    }
    ids.add(id);
    if (!profile.name.trim()) {
      return `Profile '${id}' requires a name.`;
    }
    const skillsDir = profile.skillsDir.trim();
    if (!isValidProfilePath(skillsDir)) {
      return `Profile '${id}' requires an absolute or home-relative skills directory.`;
    }
    const normalizedSkillsDir = normalizeProfilePathForCompare(skillsDir);
    if (skillsDirs.has(normalizedSkillsDir)) {
      return `Profile skills directory '${skillsDir}' is duplicated.`;
    }
    skillsDirs.add(normalizedSkillsDir);
  }

  return null;
}

function isValidProfilePath(path: string) {
  if (path.includes("\0")) {
    return false;
  }
  if (path.startsWith("\\\\") || path.startsWith("//")) {
    return isValidUncPath(path);
  }
  return (
    path === "~" ||
    path.startsWith("~/") ||
    path.startsWith("~\\") ||
    path.startsWith("/") ||
    /^[a-zA-Z]:[\\/]/.test(path)
  );
}

function isValidUncPath(path: string) {
  const normalized = path.replace(/\\/g, "/");
  if (!normalized.startsWith("//")) {
    return false;
  }
  const parts = normalized
    .slice(2)
    .split("/")
    .filter((part) => part.length > 0);
  return parts.length >= 2;
}

function normalizeProfilePathForCompare(path: string) {
  let normalized = path.trim().replace(/\\/g, "/");
  const hasUncPrefix = normalized.startsWith("//");
  if (hasUncPrefix) {
    normalized = `//${normalized.slice(2).replace(/\/+/g, "/")}`;
  } else {
    normalized = normalized.replace(/\/+/g, "/");
  }
  while (normalized.length > 1 && normalized.endsWith("/")) {
    normalized = normalized.slice(0, -1);
  }
  if (/^[a-zA-Z]:/.test(normalized)) {
    normalized = `${normalized[0].toLowerCase()}${normalized.slice(1)}`;
  }
  return normalized;
}

function linkedSkillKey(profileId: string, linkName: string) {
  return `${profileId}\u0000${linkName}`;
}

function mergeTaskRecords(incoming: TaskRecord[], existing: TaskRecord[]) {
  const records = new Map<string, TaskRecord>();
  for (const task of incoming) {
    records.set(task.id, task);
  }
  for (const task of existing) {
    if (!records.has(task.id)) {
      records.set(task.id, task);
    }
  }
  return Array.from(records.values()).slice(0, 100);
}

const LOG_PREVIEW_CHARS = 12000;

function TasksView({
  focusedTaskId,
  onTaskChange,
  tasks,
}: {
  focusedTaskId: string | null;
  onTaskChange: (task: TaskRecord) => void;
  tasks: TaskRecord[];
}) {
  const [message, setMessage] = useState<string | null>(null);

  const refreshAll = async () => {
    setMessage("Refreshing task statuses...");
    try {
      const refreshed = await Promise.all(tasks.slice(0, 40).map((task) => getTaskStatus(task.id)));
      refreshed.filter((task): task is TaskRecord => Boolean(task)).forEach(onTaskChange);
      setMessage("Task statuses refreshed.");
    } catch (error) {
      setMessage(errorMessage(error));
    }
  };

  if (tasks.length === 0) {
    return <EmptyState title="No task logs" body="Import, check or pull a project to create logs." />;
  }

  return (
    <section className="data-panel">
      <PanelHeader title="Tasks / Logs" detail={`${tasks.length} recent tasks`} />
      <div className="panel-actions">
        <button className="secondary-button" onClick={refreshAll} type="button">
          Refresh statuses
        </button>
      </div>
      {message && <p className="batch-message">{message}</p>}
      <div className="table-list">
        {tasks.map((task) => (
          <TaskLogRow
            focused={task.id === focusedTaskId}
            key={task.id}
            onTaskChange={onTaskChange}
            task={task}
          />
        ))}
      </div>
    </section>
  );
}

function TaskLogRow({
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

  useEffect(() => {
    setLogTask((current) => {
      if (current.id !== task.id) {
        return task;
      }
      return {
        ...task,
        stdout: task.stdout || current.stdout,
        stderr: task.stderr || current.stderr,
      };
    });
  }, [task]);

  useEffect(() => {
    if (focused) {
      setExpanded(true);
    }
  }, [focused]);

  const refreshStatus = async () => {
    setMessage("Refreshing...");
    try {
      const nextTask = await getTaskStatus(task.id);
      if (nextTask) {
        setLogTask(nextTask);
        onTaskChange(nextTask);
        setMessage("Status refreshed.");
      } else {
        setMessage("Task record is no longer available.");
      }
    } catch (error) {
      setMessage(errorMessage(error));
    }
  };

  const loadFullLogs = async () => {
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
  };

  const cancelQueuedTask = async () => {
    setMessage("Cancelling...");
    try {
      const nextTask = await cancelTask(task.id);
      if (nextTask) {
        setLogTask(nextTask);
        onTaskChange(nextTask);
        setMessage("Task cancelled.");
      } else {
        setMessage("Task record is no longer available.");
      }
    } catch (error) {
      setMessage(errorMessage(error));
    }
  };

  const copyLogs = async () => {
    const raw = rawTaskLogs(logTask);
    try {
      await navigator.clipboard.writeText(raw);
      setMessage("Logs copied.");
    } catch {
      window.prompt("Copy logs", raw);
      setMessage("Copy fallback opened.");
    }
  };

  const stdout = boundedLog(logTask.stdout);
  const stderr = boundedLog(logTask.stderr);
  const projectErrors = logTask.projectOutcomes.filter((outcome) => outcome.error);

  return (
    <article
      className={focused ? "list-row task-row focused-task" : "list-row task-row"}
      id={`task-${logTask.id}`}
    >
      <div className="task-summary">
        <h2>
          {logTask.kind} / {logTask.status}
        </h2>
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
          <button className="secondary-button" onClick={refreshStatus} type="button">
            Refresh
          </button>
          <button className="secondary-button" onClick={loadFullLogs} type="button">
            {expanded ? "Reload logs" : "Expand logs"}
          </button>
          <button className="secondary-button" onClick={copyLogs} type="button">
            Copy raw
          </button>
          {logTask.status === "queued" && (
            <button className="secondary-button" onClick={cancelQueuedTask} type="button">
              Cancel
            </button>
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

type BoundedLog = {
  text: string;
  truncated: boolean;
};

function boundedLog(log: string): BoundedLog {
  if (log.length <= LOG_PREVIEW_CHARS) {
    return { text: log, truncated: false };
  }
  return {
    text: log.slice(log.length - LOG_PREVIEW_CHARS),
    truncated: true,
  };
}

function rawTaskLogs(task: TaskRecord) {
  return [
    task.stdout ? `--- stdout ---\n${task.stdout}` : "",
    task.stderr ? `--- stderr ---\n${task.stderr}` : "",
  ]
    .filter(Boolean)
    .join("\n\n");
}

function uniqueNonEmptyLines(value: string) {
  const seen = new Set<string>();
  const lines: string[] = [];
  for (const line of value.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || seen.has(trimmed)) {
      continue;
    }
    seen.add(trimmed);
    lines.push(trimmed);
  }
  return lines;
}

function clampAutomaticCheckInterval(value: number) {
  if (!Number.isFinite(value)) {
    return 60;
  }
  return Math.min(1440, Math.max(1, Math.round(value)));
}

function SettingsView({
  onWorkspaceChange,
  workspace,
}: {
  onWorkspaceChange: (workspace: Workspace, message: string) => void;
  workspace: Workspace;
}) {
  const [config, setConfig] = useState<UserConfig | null>(null);
  const [recentDraft, setRecentDraft] = useState("");
  const [workspaceDraft, setWorkspaceDraft] = useState(workspace.root);
  const [profileDrafts, setProfileDrafts] = useState<AgentProfile[]>(
    workspace.agentProfiles.map((state) => state.profile),
  );
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setMessage("Loading settings...");
    loadUserConfig()
      .then((loaded) => {
        if (cancelled) {
          return;
        }
        setConfig(loaded);
        setRecentDraft(loaded.recentWorkspaces.join("\n"));
        setProfileDrafts(loaded.agentProfiles);
        setMessage(null);
      })
      .catch((error) => {
        if (!cancelled) {
          setMessage(errorMessage(error));
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    setWorkspaceDraft(workspace.root);
  }, [workspace.root]);

  const updateConfig = (update: (current: UserConfig) => UserConfig) => {
    if (!config) {
      return;
    }
    setConfig(update(config));
  };

  const saveGeneralSettings = async () => {
    if (!config) {
      return;
    }
    setBusy(true);
    setMessage("Saving settings...");
    const patch = {
      recentWorkspaces: uniqueNonEmptyLines(recentDraft),
      automaticChecks: {
        ...config.automaticChecks,
        intervalMinutes: clampAutomaticCheckInterval(config.automaticChecks.intervalMinutes),
      },
      uiPreferences: config.uiPreferences,
    };
    try {
      const saved = await patchUserPreferences(patch);
      setConfig(saved);
      setRecentDraft(saved.recentWorkspaces.join("\n"));
      setMessage("Settings saved.");
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const switchWorkspace = async (path: string) => {
    const nextPath = path.trim();
    if (!nextPath) {
      setMessage("Workspace path is required.");
      return;
    }
    setBusy(true);
    setMessage("Opening workspace...");
    try {
      const nextWorkspace = await selectWorkspace(nextPath);
      const saved = await loadUserConfig();
      setConfig(saved);
      setRecentDraft(saved.recentWorkspaces.join("\n"));
      setWorkspaceDraft(nextWorkspace.root);
      onWorkspaceChange(nextWorkspace, "Workspace changed.");
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const saveProfiles = async () => {
    const normalizedProfiles = profileDrafts.map((profile) => ({
      ...profile,
      name: profile.name.trim(),
      skillsDir: profile.skillsDir.trim(),
    }));
    const validationError = validateProfileDrafts(normalizedProfiles);
    if (validationError) {
      setMessage(validationError);
      return;
    }

    setBusy(true);
    setMessage("Saving agent profiles...");
    try {
      const saved = await saveAgentProfiles(normalizedProfiles);
      const nextWorkspace = await scanWorkspace(workspace.root);
      setConfig(saved);
      setProfileDrafts(saved.agentProfiles);
      onWorkspaceChange(nextWorkspace, "Agent profiles saved.");
      setMessage("Agent profiles saved.");
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const updateProfile = (profileIndex: number, update: (profile: AgentProfile) => AgentProfile) => {
    setProfileDrafts((profiles) =>
      profiles.map((profile, index) => (index === profileIndex ? update(profile) : profile)),
    );
  };

  const addProfile = () => {
    setProfileDrafts((profiles) => [...profiles, emptyCustomProfile(profiles)]);
  };

  if (!config) {
    return (
      <section className="data-panel settings-grid">
        <PanelHeader title="Settings" detail="Loading" />
        <p className="batch-message">{message || "Loading settings..."}</p>
      </section>
    );
  }

  return (
    <section className="settings-layout">
      <section className="data-panel compact-form settings-grid">
        <PanelHeader title="Workspace" detail="Current and recent" />
        <label>
          <span>Current workspace</span>
          <input onChange={(event) => setWorkspaceDraft(event.target.value)} value={workspaceDraft} />
        </label>
        <div className="panel-actions">
          <button className="primary-button" disabled={busy} onClick={() => switchWorkspace(workspaceDraft)} type="button">
            Open workspace
          </button>
        </div>
        <div className="setting-row">
          <span>Projects</span>
          <strong>{workspace.projects.length}</strong>
        </div>
        <div className="setting-row">
          <span>Skills</span>
          <strong>{workspace.skills.length}</strong>
        </div>
        <label>
          <span>Recent workspaces</span>
          <textarea
            onChange={(event) => setRecentDraft(event.target.value)}
            rows={Math.max(3, Math.min(8, config.recentWorkspaces.length + 1))}
            value={recentDraft}
          />
        </label>
        <div className="recent-workspace-list">
          {config.recentWorkspaces.map((path) => (
            <button className="text-button" disabled={busy} key={path} onClick={() => switchWorkspace(path)} type="button">
              {path}
            </button>
          ))}
        </div>
      </section>

      <section className="data-panel compact-form settings-grid">
        <PanelHeader title="Preferences" detail="UI and automatic checks" />
        <label>
          <span>Theme</span>
          <select
            onChange={(event) =>
              updateConfig((current) => ({
                ...current,
                uiPreferences: {
                  ...current.uiPreferences,
                  theme: event.target.value as UserConfig["uiPreferences"]["theme"],
                },
              }))
            }
            value={config.uiPreferences.theme}
          >
            <option value="system">System</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </label>
        <label>
          <span>Project sort</span>
          <select
            onChange={(event) =>
              updateConfig((current) => ({
                ...current,
                uiPreferences: {
                  ...current.uiPreferences,
                  projectSort: event.target.value as UserConfig["uiPreferences"]["projectSort"],
                },
              }))
            }
            value={config.uiPreferences.projectSort}
          >
            <option value="name">Name</option>
            <option value="updated">Updated</option>
            <option value="skill_count">Skill count</option>
          </select>
        </label>
        <label className="inline-check">
          <input
            checked={config.uiPreferences.showHiddenProjects}
            onChange={(event) =>
              updateConfig((current) => ({
                ...current,
                uiPreferences: {
                  ...current.uiPreferences,
                  showHiddenProjects: event.target.checked,
                },
              }))
            }
            type="checkbox"
          />
          <span>Show hidden projects by default</span>
        </label>
        <label className="inline-check">
          <input
            checked={config.automaticChecks.enabled}
            onChange={(event) =>
              updateConfig((current) => ({
                ...current,
                automaticChecks: {
                  ...current.automaticChecks,
                  enabled: event.target.checked,
                },
              }))
            }
            type="checkbox"
          />
          <span>Enable automatic checks</span>
        </label>
        <label>
          <span>Check interval minutes</span>
          <input
            min={1}
            onChange={(event) =>
              updateConfig((current) => ({
                ...current,
                automaticChecks: {
                  ...current.automaticChecks,
                  intervalMinutes: Number(event.target.value),
                },
              }))
            }
            type="number"
            value={config.automaticChecks.intervalMinutes}
          />
        </label>
        <label className="inline-check">
          <input
            checked={config.automaticChecks.pullAfterCheck}
            onChange={(event) =>
              updateConfig((current) => ({
                ...current,
                automaticChecks: {
                  ...current.automaticChecks,
                  pullAfterCheck: event.target.checked,
                },
              }))
            }
            type="checkbox"
          />
          <span>Pull after automatic checks</span>
        </label>
        <button className="primary-button" disabled={busy} onClick={saveGeneralSettings} type="button">
          Save settings
        </button>
      </section>

      <section className="data-panel compact-form settings-grid settings-profiles">
        <PanelHeader title="Agent profiles" detail={`${profileDrafts.length} profiles`} />
        <div className="panel-actions">
          <button className="secondary-button" disabled={busy} onClick={addProfile} type="button">
            Add custom profile
          </button>
          <button className="primary-button" disabled={busy} onClick={saveProfiles} type="button">
            Save profiles
          </button>
        </div>
        <div className="settings-profile-list">
          {profileDrafts.map((profile, index) => (
            <article className="settings-profile-row" key={`${index}:${profile.id}`}>
              <label>
                <span>Id</span>
                <input
                  disabled={profile.builtIn}
                  onChange={(event) => updateProfile(index, (item) => ({ ...item, id: event.target.value }))}
                  value={profile.id}
                />
              </label>
              <label>
                <span>Name</span>
                <input
                  onChange={(event) => updateProfile(index, (item) => ({ ...item, name: event.target.value }))}
                  value={profile.name}
                />
              </label>
              <label>
                <span>Skills directory</span>
                <input
                  onChange={(event) =>
                    updateProfile(index, (item) => ({ ...item, skillsDir: event.target.value }))
                  }
                  value={profile.skillsDir}
                />
              </label>
              <label className="inline-check">
                <input
                  checked={profile.enabled}
                  onChange={(event) =>
                    updateProfile(index, (item) => ({ ...item, enabled: event.target.checked }))
                  }
                  type="checkbox"
                />
                <span>Enabled</span>
              </label>
              {!profile.builtIn && (
                <button
                  className="secondary-button"
                  disabled={busy}
                  onClick={() =>
                    setProfileDrafts((profiles) => profiles.filter((_, itemIndex) => itemIndex !== index))
                  }
                  type="button"
                >
                  Remove
                </button>
              )}
            </article>
          ))}
        </div>
      </section>
      {message && <p className="batch-message settings-message">{message}</p>}
    </section>
  );
}

function PanelHeader({ detail, title }: { detail: string; title: string }) {
  return (
    <header className="panel-header">
      <h2>{title}</h2>
      <span>{detail}</span>
    </header>
  );
}

function EmptyState({ body, title }: { body: string; title: string }) {
  return <StatusPanel title={title} body={body} tone="neutral" />;
}

function StatusPanel({
  body,
  title,
  tone = "neutral",
}: {
  body: string;
  title: string;
  tone?: "neutral" | "error";
}) {
  return (
    <article className={tone === "error" ? "status-panel error" : "status-panel"}>
      <h2>{title}</h2>
      <p>{body}</p>
    </article>
  );
}

function SummaryMetric({ label, value }: { label: string; value: number }) {
  return (
    <article className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </article>
  );
}

function isSafeLinkPreview(status: LinkPreview["status"]) {
  return status === "will_link" || status === "already_installed";
}

function batchSelectionKey(skillIds: string[], profileIds: string[]) {
  return JSON.stringify({
    profiles: [...profileIds].sort(),
    skills: [...skillIds].sort(),
  });
}

function batchWorkspaceKey(workspace: Workspace) {
  return JSON.stringify({
    profiles: workspace.agentProfiles
      .map((state) => ({
        exists: state.exists,
        id: state.profile.id,
        skillsDir: state.skillsDir,
        writable: state.writable,
      }))
      .sort((left, right) => left.id.localeCompare(right.id)),
    root: workspace.root,
    skills: workspace.skills
      .map((skill) => ({
        id: skill.id,
        path: skill.absolutePath,
        linkName: skill.defaultLinkName,
      }))
      .sort((left, right) => left.id.localeCompare(right.id)),
  });
}

function skillName(skills: Skill[], skillId: string) {
  return skills.find((skill) => skill.id === skillId)?.name || skillId;
}

function profileName(profiles: AgentProfileState[], profileId: string) {
  return profiles.find((state) => state.profile.id === profileId)?.profile.name || profileId;
}

function installPreviewKey(skillId: string, agentProfileId: string) {
  return `${skillId}\u0000${agentProfileId}`;
}

function unlinkPreviewMatchesSkill(preview: UnlinkPreview, skill: Skill) {
  return skill.installedAgents.some(
    (install) =>
      install.agentProfileId === preview.agentProfileId &&
      install.linkName === preview.linkName &&
      install.sourcePath === preview.sourcePath,
  );
}

function joinPath(parent: string, child: string) {
  return `${parent.replace(/[\\/]+$/, "")}/${child.replace(/^[\\/]+/, "")}`;
}

function loadStateWorkspaceRoot(state: LoadState) {
  if (state.status === "ready") {
    return state.workspace.root;
  }
  if (state.status === "error") {
    return state.workspace?.root ?? null;
  }
  return null;
}

function latestProjectError(project: Project, tasks: TaskRecord[], workspaceRoot: string) {
  for (const task of tasks) {
    if (task.workspaceRoot !== workspaceRoot) {
      continue;
    }
    const outcome = task.projectOutcomes.find(
      (outcome) => outcome.projectId === project.id && outcome.status === "failed",
    );
    if (outcome) {
      return { task, outcome };
    }
  }
  return null;
}

function divergenceLabel(project: Project) {
  return `${project.aheadCount} ahead / ${project.behindCount} behind`;
}

function statusLabel(status: Project["gitStatus"]) {
  return status.replace(/_/g, " ");
}

function errorMessage(error: unknown) {
  if (error && typeof error === "object" && "message" in error) {
    return String(error.message);
  }

  return error instanceof Error ? error.message : String(error);
}

export default App;
