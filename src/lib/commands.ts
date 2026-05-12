import { invoke as tauriInvoke, type InvokeArgs } from "@tauri-apps/api/core";
import type {
  AgentProfile,
  BatchLinkExecuteRequest,
  BatchLinkOperationResult,
  BatchLinkPreview,
  BatchLinkPreviewRequest,
  ExecuteLinkSkillRequest,
  ExecuteUnlinkSkillRequest,
  ImportProjectRequest,
  LinkPreview,
  LinkSkillRequest,
  PullAllProjectsRequest,
  PullProjectRequest,
  SkillMarkdownPreview,
  TaskOperationResult,
  TaskRecord,
  UnlinkPreview,
  UnlinkSkillRequest,
  UserConfig,
  UserPreferencesPatch,
  Workspace,
  WorkspaceConfig,
} from "./types";

const TAURI_BRIDGE_UNAVAILABLE =
  "Tauri desktop bridge is unavailable. Start the app with `npm run tauri:dev` or use a desktop build instead of opening the Vite page directly.";

type TauriGlobal = typeof globalThis & {
  __TAURI_INTERNALS__?: {
    invoke?: unknown;
  };
};

function hasTauriBridge(): boolean {
  return typeof (globalThis as TauriGlobal).__TAURI_INTERNALS__?.invoke === "function";
}

function invokeCommand<T>(command: string, args?: InvokeArgs): Promise<T> {
  if (!hasTauriBridge()) {
    return Promise.reject(new Error(TAURI_BRIDGE_UNAVAILABLE));
  }
  return tauriInvoke<T>(command, args);
}

export function loadWorkspaceConfig(workspaceRoot: string): Promise<WorkspaceConfig> {
  return invokeCommand<WorkspaceConfig>("load_workspace_config_command", { workspaceRoot });
}

export function saveWorkspaceConfig(
  workspaceRoot: string,
  config: WorkspaceConfig,
): Promise<WorkspaceConfig> {
  return invokeCommand<WorkspaceConfig>("save_workspace_config_command", { workspaceRoot, config });
}

export function loadUserConfig(): Promise<UserConfig> {
  return invokeCommand<UserConfig>("load_user_config_command");
}

export function patchUserPreferences(patch: UserPreferencesPatch): Promise<UserConfig> {
  return invokeCommand<UserConfig>("patch_user_preferences_command", { patch });
}

export function saveAgentProfiles(profiles: AgentProfile[]): Promise<UserConfig> {
  return invokeCommand<UserConfig>("save_agent_profiles_command", { profiles });
}

export function createAgentProfileDir(
  workspaceRoot: string,
  profileId: string,
  confirmed: boolean,
): Promise<Workspace> {
  return invokeCommand<Workspace>("create_agent_profile_dir_command", {
    workspaceRoot,
    profileId,
    confirmed,
  });
}

export function restoreRecentWorkspace(): Promise<Workspace | null> {
  if (!hasTauriBridge()) {
    return Promise.resolve(null);
  }
  return invokeCommand<Workspace | null>("restore_recent_workspace_command");
}

export function selectWorkspace(workspaceRoot: string): Promise<Workspace> {
  return invokeCommand<Workspace>("select_workspace_command", { workspaceRoot });
}

export function scanWorkspace(workspaceRoot: string): Promise<Workspace> {
  return invokeCommand<Workspace>("scan_workspace_command", { workspaceRoot });
}

export function readSkillMarkdownPreview(
  workspaceRoot: string,
  skillId: string,
  maxBytes: number,
): Promise<SkillMarkdownPreview> {
  return invokeCommand<SkillMarkdownPreview>("read_skill_markdown_preview_command", {
    workspaceRoot,
    skillId,
    maxBytes,
  });
}

export function openWorkspacePath(workspaceRoot: string, path: string): Promise<void> {
  return invokeCommand<void>("open_workspace_path_command", { workspaceRoot, path });
}

export function importProject(
  workspaceRoot: string,
  request: ImportProjectRequest,
): Promise<TaskOperationResult> {
  return invokeCommand<TaskOperationResult>("import_project_command", { workspaceRoot, request });
}

export function checkProjectUpdates(
  workspaceRoot: string,
  projectId: string,
): Promise<TaskOperationResult> {
  return invokeCommand<TaskOperationResult>("check_project_updates_command", {
    workspaceRoot,
    projectId,
  });
}

export function checkAllProjectUpdates(workspaceRoot: string): Promise<TaskOperationResult> {
  return invokeCommand<TaskOperationResult>("check_all_project_updates_command", { workspaceRoot });
}

export function pullProject(
  workspaceRoot: string,
  request: PullProjectRequest,
): Promise<TaskOperationResult> {
  return invokeCommand<TaskOperationResult>("pull_project_command", { workspaceRoot, request });
}

export function pullAllProjects(
  workspaceRoot: string,
  request: PullAllProjectsRequest,
): Promise<TaskOperationResult> {
  return invokeCommand<TaskOperationResult>("pull_all_projects_command", {
    workspaceRoot,
    request,
  });
}

export function previewLinkSkill(
  workspaceRoot: string,
  request: LinkSkillRequest,
): Promise<LinkPreview> {
  return invokeCommand<LinkPreview>("preview_link_skill_command", { workspaceRoot, request });
}

export function linkSkill(
  workspaceRoot: string,
  request: ExecuteLinkSkillRequest,
): Promise<TaskOperationResult> {
  return invokeCommand<TaskOperationResult>("link_skill_command", { workspaceRoot, request });
}

export function previewLinkSkillsBatch(
  workspaceRoot: string,
  request: BatchLinkPreviewRequest,
): Promise<BatchLinkPreview> {
  return invokeCommand<BatchLinkPreview>("preview_link_skills_batch_command", {
    workspaceRoot,
    request,
  });
}

export function linkSkillsBatch(
  workspaceRoot: string,
  request: BatchLinkExecuteRequest,
): Promise<BatchLinkOperationResult> {
  return invokeCommand<BatchLinkOperationResult>("link_skills_batch_command", {
    workspaceRoot,
    request,
  });
}

export function previewUnlinkSkill(
  workspaceRoot: string,
  request: UnlinkSkillRequest,
): Promise<UnlinkPreview> {
  return invokeCommand<UnlinkPreview>("preview_unlink_skill_command", { workspaceRoot, request });
}

export function unlinkSkill(
  workspaceRoot: string,
  request: ExecuteUnlinkSkillRequest,
): Promise<TaskOperationResult> {
  return invokeCommand<TaskOperationResult>("unlink_skill_command", { workspaceRoot, request });
}

export function getTaskStatus(taskId: string): Promise<TaskRecord | null> {
  return invokeCommand<TaskRecord | null>("get_task_status_command", { taskId });
}

export function getTaskLogs(taskId: string): Promise<TaskRecord | null> {
  return invokeCommand<TaskRecord | null>("get_task_logs_command", { taskId });
}

export function recentTaskRecords(workspaceRoot?: string, limit?: number): Promise<TaskRecord[]> {
  return invokeCommand<TaskRecord[]>("recent_task_records_command", { workspaceRoot, limit });
}

export function cancelTask(taskId: string): Promise<TaskRecord | null> {
  return invokeCommand<TaskRecord | null>("cancel_task_command", { taskId });
}

// Desktop window chrome helpers. No-ops when running via Vite without the Tauri bridge
// so `npm run ui:smoke` and SSR paths don't fail. Errors are swallowed because window
// cosmetics must not block the app boot or normal UI flows.
async function withCurrentWindow(
  action: (window: Awaited<ReturnType<typeof getWindow>>) => Promise<void>,
) {
  if (!hasTauriBridge()) return;
  try {
    await action(await getWindow());
  } catch {
    // Non-fatal: older runtimes, web-only smoke runs, and unsupported platforms should not surface here.
  }
}

async function getWindow() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}

export async function setWindowTheme(theme: "light" | "dark" | null): Promise<void> {
  await withCurrentWindow((window) => window.setTheme(theme));
}
