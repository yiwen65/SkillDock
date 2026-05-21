use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub root: String,
    pub projects: Vec<Project>,
    pub skills: Vec<Skill>,
    pub agent_profiles: Vec<AgentProfileState>,
}

impl Workspace {
    pub fn placeholder() -> Self {
        Self {
            root: String::new(),
            projects: Vec::new(),
            skills: Vec::new(),
            agent_profiles: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub remote_url: Option<String>,
    pub provider: GitProvider,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub git_status: GitStatus,
    pub ahead_count: u32,
    pub behind_count: u32,
    pub pull_all_eligible: bool,
    pub category: ProjectCategory,
    pub license_file: Option<String>,
    pub readme_file: Option<String>,
    pub readme_summary: Option<String>,
    pub skill_count: usize,
    pub hidden: bool,
    pub favorite: bool,
    pub tags: Vec<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source_project_id: String,
    pub relative_path: String,
    pub absolute_path: String,
    pub default_link_name: String,
    pub has_assets: bool,
    pub has_scripts: bool,
    pub has_references: bool,
    pub installed_agents: Vec<InstalledAgentSkill>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMarkdownPreview {
    pub skill_id: String,
    pub markdown: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub skills_dir: String,
    pub enabled: bool,
    pub built_in: bool,
    pub link_mode: LinkMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileState {
    pub profile: AgentProfile,
    pub skills_dir: String,
    pub exists: bool,
    pub writable: bool,
    pub symlink_count: usize,
    pub workspace_link_count: usize,
    pub entries: Vec<AgentDirectoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDirectoryEntry {
    pub name: String,
    pub path: String,
    pub target_path: Option<String>,
    pub source_path: Option<String>,
    pub kind: AgentDirectoryEntryKind,
    pub status: InstalledAgentSkillStatus,
    pub removable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDirectoryEntryKind {
    Symlink,
    File,
    Directory,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAgentSkill {
    pub agent_profile_id: String,
    pub link_name: String,
    pub target_path: String,
    pub source_path: String,
    pub status: InstalledAgentSkillStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    pub kind: TaskKind,
    pub status: TaskStatus,
    pub summary: String,
    pub error: Option<String>,
    pub stdout: String,
    pub stderr: String,
    pub project_outcomes: Vec<ProjectTaskRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTaskRecord {
    pub project_id: String,
    pub status: TaskStatus,
    pub summary: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskOperationResult {
    pub task: TaskRecord,
    pub workspace: Workspace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProjectRequest {
    pub source: String,
    pub directory_name: Option<String>,
    pub shallow: bool,
    #[serde(default)]
    pub skill_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullProjectRequest {
    pub project_id: String,
    pub autostash: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullAllProjectsRequest {
    pub autostash: bool,
    pub safe_project_ids: Option<Vec<String>>,
}

pub fn is_pull_all_eligible(git_status: &GitStatus, upstream: Option<&str>) -> bool {
    upstream.is_some()
        && matches!(
            git_status,
            GitStatus::UpToDate
                | GitStatus::Behind
                | GitStatus::Ahead
                | GitStatus::Diverged
                | GitStatus::Dirty
        )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkSkillRequest {
    pub skill_id: String,
    pub agent_profile_id: String,
    pub link_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteLinkSkillRequest {
    pub preview: LinkPreview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchLinkPreviewRequest {
    pub items: Vec<LinkSkillRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchLinkPreview {
    pub previews: Vec<LinkPreview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchLinkExecuteRequest {
    pub previews: Vec<LinkPreview>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchLinkSummary {
    pub linked: usize,
    pub already_installed: usize,
    pub skipped: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchLinkOperationResult {
    pub task: TaskRecord,
    pub workspace: Workspace,
    pub summary: BatchLinkSummary,
    pub previews: Vec<LinkPreview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlinkSkillRequest {
    pub agent_profile_id: String,
    pub link_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteUnlinkSkillRequest {
    pub preview: UnlinkPreview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchUnlinkPreviewRequest {
    pub items: Vec<UnlinkSkillRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchUnlinkPreview {
    pub previews: Vec<UnlinkPreview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchUnlinkExecuteRequest {
    pub previews: Vec<UnlinkPreview>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchUnlinkSummary {
    pub unlinked: usize,
    pub skipped: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchUnlinkOperationResult {
    pub task: TaskRecord,
    pub workspace: Workspace,
    pub summary: BatchUnlinkSummary,
    pub previews: Vec<UnlinkPreview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlinkPreview {
    pub agent_profile_id: String,
    pub link_name: String,
    pub target_path: String,
    pub source_path: Option<String>,
    pub status: UnlinkPreviewStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkPreview {
    pub skill_id: String,
    pub agent_profile_id: String,
    pub link_name: String,
    pub source_path: String,
    pub target_path: String,
    pub status: LinkPreviewStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitProvider {
    Github,
    Gitlab,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitStatus {
    UpToDate,
    Behind,
    Ahead,
    Diverged,
    Dirty,
    NoUpstream,
    Detached,
    FetchFailed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectCategory {
    Skills,
    Plugins,
    Tools,
    DesignResources,
    Uncategorized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkMode {
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstalledAgentSkillStatus {
    Valid,
    Broken,
    External,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkPreviewStatus {
    WillLink,
    AlreadyInstalled,
    NameConflict,
    BlockedByRealFile,
    BlockedByRealDirectory,
    MissingSource,
    AgentPathMissing,
    AgentPathNotWritable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnlinkPreviewStatus {
    WillUnlink,
    BlockedByRealFile,
    BlockedByRealDirectory,
    ExternalSymlink,
    BrokenSymlink,
    NotWorkspaceSkill,
    AgentPathMissing,
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Running,
    Succeeded,
    Skipped,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    ScanWorkspace,
    ImportProject,
    DeleteProject,
    RestoreCatalog,
    SyncCatalog,
    FetchProject,
    PullProject,
    SyncAllProjects,
    LinkSkill,
    LinkSkillsBatch,
    UnlinkSkill,
    UnlinkSkillsBatch,
    CreateAgentDir,
}
