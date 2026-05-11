import { useMemo } from "react";
import { SummaryMetric, type ThemePreference, type ViewName } from "./lib/shared";
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

export function CoreView({
  activeView,
  onCheckAll,
  onCheckProject,
  onImport,
  onCreateAgentDir,
  onBatchLinkResult,
  onOperationResult,
  onThemePreferenceChange,
  onWorkspaceChange,
  onTaskChange,
  onOpenTaskLog,
  onPullAll,
  onPullProject,
  onSetProjectHidden,
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
  onThemePreferenceChange: (theme: ThemePreference) => void;
  onWorkspaceChange: (workspace: Workspace, message: string) => void;
  onTaskChange: (task: TaskRecord) => void;
  onOpenTaskLog: (taskId: string) => void;
  onPullAll: (autostash: boolean) => void;
  onPullProject: (projectId: string, autostash: boolean) => void;
  onSetProjectHidden: (projectId: string, hidden: boolean) => void;
  operationBusy: boolean;
  focusedTaskId: string | null;
  taskHistory: TaskRecord[];
  workspace: Workspace;
}) {
  return (
    <>
      {activeView === "Skills" && <WorkspaceMetrics workspace={workspace} />}
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
}

function WorkspaceMetrics({ workspace }: { workspace: Workspace }) {
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
      <SummaryMetric label="Projects" value={workspace.projects.length} />
      <SummaryMetric label="Skills" value={workspace.skills.length} />
      <SummaryMetric label="Agents" value={workspace.agentProfiles.length} />
      <SummaryMetric label="Installs" value={installedCount} />
    </div>
  );
}

export default CoreView;
