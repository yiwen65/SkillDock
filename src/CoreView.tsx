import React, { useMemo } from "react";
import { SummaryMetric, type ViewName } from "./lib/shared";
import type {
  AgentProfileState,
  BatchLinkOperationResult,
  TaskOperationResult,
  TaskRecord,
  Workspace,
} from "./lib/types";
import { AgentsView } from "./views/AgentsView";
import { ProjectsView } from "./views/ProjectsView";
import { SettingsView } from "./views/SettingsView";
import { SkillsView } from "./views/SkillsView";
import { TasksView } from "./views/TasksView";

export const CoreView = React.memo(function CoreView({
  activeView,
  onCheckAll,
  onCheckProject,
  onImport,
  onCreateAgentDir,
  onBatchLinkResult,
  onOperationResult,
  onWorkspaceChange,
  onThemePreferenceChange,
  onTaskChange,
  onOpenTaskLog,
  onPullAll,
  onPullProject,
  onSetProjectHidden,
  onDeleteProject,
  operationBusy,
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
  onThemePreferenceChange: (theme: "dark" | "light" | "system") => void;
  onTaskChange: (task: TaskRecord) => void;
  onOpenTaskLog: (taskId: string) => void;
  onPullAll: (autostash: boolean) => void;
  onPullProject: (projectId: string, autostash: boolean) => void;
  onSetProjectHidden: (projectId: string, hidden: boolean) => void;
  onDeleteProject: (projectId: string) => void;
  operationBusy: boolean;
  focusedTaskId: string | null;
  taskHistory: TaskRecord[];
  workspace: Workspace;
}) {
  return (
    <>
      {activeView === "Skills" && (
        <>
          <WorkspaceMetrics workspace={workspace} />
          <SkillsView
            onBatchLinkResult={onBatchLinkResult}
            onOperationResult={onOperationResult}
            workspace={workspace}
          />
        </>
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
          onDeleteProject={onDeleteProject}
          operationBusy={operationBusy}
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
      {activeView === "Logs" && (
        <TasksView focusedTaskId={focusedTaskId} onTaskChange={onTaskChange} tasks={taskHistory} />
      )}
      {activeView === "Settings" && (
        <SettingsView
          onThemePreferenceChange={onThemePreferenceChange}
          onWorkspaceChange={onWorkspaceChange}
          workspace={workspace}
        />
      )}
    </>
  );
});

const WorkspaceMetrics = React.memo(function WorkspaceMetrics({
  workspace,
}: {
  workspace: Workspace;
}) {
  const installedCount = useMemo(
    () =>
      workspace.skills.reduce(
        (count, skill) => count + (skill.installedAgents.length > 0 ? 1 : 0),
        0,
      ),
    [workspace.skills],
  );

  return (
    <div className="metric-strip">
      <SummaryMetric
        icon={<MetricIcon kind="projects" />}
        label="Projects"
        value={workspace.projects.length}
      />
      <SummaryMetric
        icon={<MetricIcon kind="skills" />}
        label="Skills"
        value={workspace.skills.length}
      />
      <SummaryMetric
        icon={<MetricIcon kind="agents" />}
        label="Agents"
        value={workspace.agentProfiles.length}
      />
      <SummaryMetric
        icon={<MetricIcon kind="installs" />}
        label="Installs"
        value={installedCount}
      />
    </div>
  );
});

function MetricIcon({ kind }: { kind: "agents" | "installs" | "projects" | "skills" }) {
  if (kind === "projects") {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
        <path d="M3 7.5a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v7.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />
        <path d="M3 10h18" />
      </svg>
    );
  }
  if (kind === "agents") {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
        <path d="M8 10a4 4 0 1 1 8 0" />
        <path d="M4 20a8 8 0 0 1 16 0" />
        <path d="M9 13h6" />
      </svg>
    );
  }
  if (kind === "installs") {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
        <path d="M12 3v12" />
        <path d="m7 10 5 5 5-5" />
        <path d="M5 20h14" />
      </svg>
    );
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <path d="m12 2 2.2 6.2L20 10.5l-5.8 2.2L12 19l-2.2-6.3L4 10.5l5.8-2.3Z" />
      <path d="m19 4 .8 2.2L22 7l-2.2.8L19 10l-.8-2.2L16 7l2.2-.8Z" />
    </svg>
  );
}

export default CoreView;
