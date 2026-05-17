export type Workspace = {
  root: string;
  projects: Project[];
  skills: Skill[];
  agentProfiles: AgentProfileState[];
};

export type Project = {
  id: string;
  name: string;
  path: string;
  remoteUrl?: string;
  provider: GitProvider;
  branch?: string;
  upstream?: string;
  gitStatus: GitStatus;
  aheadCount: number;
  behindCount: number;
  pullAllEligible: boolean;
  category: ProjectCategory;
  licenseFile?: string;
  readmeFile?: string;
  readmeSummary?: string;
  skillCount: number;
  hidden: boolean;
  favorite: boolean;
  tags: string[];
  notes?: string;
};

export type Skill = {
  id: string;
  name: string;
  description?: string;
  sourceProjectId: string;
  relativePath: string;
  absolutePath: string;
  defaultLinkName: string;
  hasAssets: boolean;
  hasScripts: boolean;
  hasReferences: boolean;
  installedAgents: InstalledAgentSkill[];
  lastModified?: string;
};

export type SkillMarkdownPreview = {
  skillId: string;
  markdown: string;
  truncated: boolean;
};

export type AgentProfile = {
  id: string;
  name: string;
  skillsDir: string;
  enabled: boolean;
  builtIn: boolean;
  linkMode: LinkMode;
};

export type AgentProfileState = {
  profile: AgentProfile;
  skillsDir: string;
  exists: boolean;
  writable: boolean;
  symlinkCount: number;
  workspaceLinkCount: number;
  entries: AgentDirectoryEntry[];
};

export type AgentDirectoryEntry = {
  name: string;
  path: string;
  targetPath?: string;
  sourcePath?: string;
  kind: AgentDirectoryEntryKind;
  status: InstalledAgentSkillStatus;
  removable: boolean;
};

export type InstalledAgentSkill = {
  agentProfileId: string;
  linkName: string;
  targetPath: string;
  sourcePath: string;
  status: InstalledAgentSkillStatus;
};

export type TaskRecord = {
  id: string;
  workspaceRoot?: string;
  kind: TaskKind;
  status: TaskStatus;
  summary: string;
  error?: string;
  stdout: string;
  stderr: string;
  projectOutcomes: ProjectTaskRecord[];
};

export type ProjectTaskRecord = {
  projectId: string;
  status: TaskStatus;
  summary: string;
  error?: string;
};

export type TaskOperationResult = {
  task: TaskRecord;
  workspace: Workspace;
};

export type ImportProjectRequest = {
  source: string;
  directoryName?: string;
  shallow: boolean;
  skillPath?: string;
};

export type PullProjectRequest = {
  projectId: string;
  autostash: boolean;
};

export type PullAllProjectsRequest = {
  autostash: boolean;
  safeProjectIds?: string[];
};

export type LinkSkillRequest = {
  skillId: string;
  agentProfileId: string;
  linkName?: string;
};

export type ExecuteLinkSkillRequest = {
  preview: LinkPreview;
};

export type BatchLinkPreviewRequest = {
  items: LinkSkillRequest[];
};

export type BatchLinkPreview = {
  previews: LinkPreview[];
};

export type BatchLinkExecuteRequest = {
  previews: LinkPreview[];
};

export type BatchLinkSummary = {
  linked: number;
  alreadyInstalled: number;
  skipped: number;
  failed: number;
};

export type BatchLinkOperationResult = {
  task: TaskRecord;
  workspace: Workspace;
  summary: BatchLinkSummary;
  previews: LinkPreview[];
};

export type UnlinkSkillRequest = {
  agentProfileId: string;
  linkName: string;
};

export type ExecuteUnlinkSkillRequest = {
  preview: UnlinkPreview;
};

export type BatchUnlinkPreviewRequest = {
  items: UnlinkSkillRequest[];
};

export type BatchUnlinkPreview = {
  previews: UnlinkPreview[];
};

export type BatchUnlinkExecuteRequest = {
  previews: UnlinkPreview[];
};

export type BatchUnlinkSummary = {
  unlinked: number;
  skipped: number;
  failed: number;
};

export type BatchUnlinkOperationResult = {
  task: TaskRecord;
  workspace: Workspace;
  summary: BatchUnlinkSummary;
  previews: UnlinkPreview[];
};

export type UnlinkPreview = {
  agentProfileId: string;
  linkName: string;
  targetPath: string;
  sourcePath?: string;
  status: UnlinkPreviewStatus;
  message?: string;
};

export type LinkPreview = {
  skillId: string;
  agentProfileId: string;
  linkName: string;
  sourcePath: string;
  targetPath: string;
  status: LinkPreviewStatus;
  message?: string;
};

export type WorkspaceConfig = {
  schemaVersion: number;
  projects: WorkspaceProjectMetadata[];
};

export type WorkspaceProjectMetadata = {
  projectId: string;
  displayName?: string;
  category?: ProjectCategory;
  favorite: boolean;
  hidden: boolean;
  tags: string[];
  notes?: string;
  autoCheck?: boolean;
  autoPull?: boolean;
};

export type CatalogRepository = {
  schemaVersion: number;
  id: string;
  remoteUrl: string;
  directoryName: string;
  state: CatalogRepositoryState;
  branch?: string;
  shallow: boolean;
  skillPaths?: string[];
  skillPath?: string;
  addedAt: string;
  updatedAt: string;
};

export type CatalogRepositoryState = "active" | "removed";

export type CatalogProjectComparison = {
  id: string;
  remoteUrl: string;
  directoryName: string;
  localPath?: string;
};

export type WorkspaceCatalogSummary = {
  catalogPath: string;
  repositories: CatalogRepository[];
  missing: CatalogProjectComparison[];
  localOnly: CatalogProjectComparison[];
  activeCount: number;
  missingCount: number;
  localOnlyCount: number;
  gitSyncAvailable: boolean;
  gitRemote?: string;
};

export type CatalogSyncResult = {
  status: TaskStatus;
  summary: string;
  stdout: string;
  stderr: string;
};

export type UserConfig = {
  schemaVersion: number;
  recentWorkspaces: string[];
  agentProfiles: AgentProfile[];
  uiPreferences: UiPreferences;
  windowSize: WindowSize;
  automaticChecks: AutomaticCheckSettings;
};

export type UserPreferencesPatch = {
  recentWorkspaces: string[];
  uiPreferences: UiPreferences;
  automaticChecks: AutomaticCheckSettings;
};

export type UiPreferences = {
  theme: ThemePreference;
  projectSort: ProjectSort;
  showHiddenProjects: boolean;
};

export type WindowSize = {
  width: number;
  height: number;
};

export type AutomaticCheckSettings = {
  enabled: boolean;
  intervalMinutes: number;
  pullAfterCheck: boolean;
};

export type GitProvider = "github" | "gitlab" | "unknown";

export type GitStatus =
  | "up_to_date"
  | "behind"
  | "ahead"
  | "diverged"
  | "dirty"
  | "no_upstream"
  | "detached"
  | "fetch_failed"
  | "unknown";

export type ProjectCategory = "skills" | "plugins" | "tools" | "design_resources" | "uncategorized";

export type LinkMode = "symlink";

export type ThemePreference = "system" | "light" | "dark";

export type ProjectSort = "name" | "updated" | "skill_count";

export type InstalledAgentSkillStatus = "valid" | "broken" | "external" | "conflict";

export type AgentDirectoryEntryKind = "symlink" | "file" | "directory" | "other";

export type LinkPreviewStatus =
  | "will_link"
  | "already_installed"
  | "name_conflict"
  | "blocked_by_real_file"
  | "blocked_by_real_directory"
  | "missing_source"
  | "agent_path_missing"
  | "agent_path_not_writable";

export type UnlinkPreviewStatus =
  | "will_unlink"
  | "blocked_by_real_file"
  | "blocked_by_real_directory"
  | "external_symlink"
  | "broken_symlink"
  | "not_workspace_skill"
  | "agent_path_missing"
  | "not_found";

export type WorkspaceErrorKind =
  | "path_missing"
  | "not_directory"
  | "io"
  | "config"
  | "outside_workspace";

export type WorkspaceError = {
  kind: WorkspaceErrorKind;
  path: string;
  message: string;
};

export type GitOperationError = {
  kind: GitOperationErrorKind;
  path?: string;
  message: string;
};

export type GitOperationErrorKind =
  | "invalid_repository"
  | "invalid_directory_name"
  | "workspace"
  | "io";

export type TaskStatus = "queued" | "running" | "succeeded" | "skipped" | "failed" | "cancelled";

export type TaskKind =
  | "scan_workspace"
  | "import_project"
  | "delete_project"
  | "restore_catalog"
  | "sync_catalog"
  | "fetch_project"
  | "pull_project"
  | "sync_all_projects"
  | "link_skill"
  | "link_skills_batch"
  | "unlink_skill"
  | "unlink_skills_batch"
  | "create_agent_dir";
