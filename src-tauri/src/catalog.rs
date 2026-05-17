use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{
    clone_import_project, git_command_output_with_timeout, load_user_config,
    run_workspace_task_background, run_workspace_task_blocking, safe_import_skill_path,
    safe_project_dir_name, scan_workspace_at, AgentProfile, ConfigError, GitOperationError,
    Project, ProjectTaskRecord, TaskKind, TaskOperationResult, TaskOutcome, TaskStatus, Workspace,
    WorkspaceError,
};

const CATALOG_SCHEMA_VERSION: u32 = 1;
const CATALOG_DIR: &str = ".skilldock/catalog";
const CATALOG_REPOS_DIR: &str = ".skilldock/catalog/repos";
const CATALOG_TOMBSTONES_DIR: &str = ".skilldock/catalog/tombstones";
const RESTORE_CLONE_MAX_ATTEMPTS: usize = 3;
const RESTORE_CLONE_TIMEOUT: Duration = Duration::from_secs(120);
const CATALOG_GIT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRepository {
    pub schema_version: u32,
    pub id: String,
    pub remote_url: String,
    pub directory_name: String,
    pub state: CatalogRepositoryState,
    pub branch: Option<String>,
    pub shallow: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<String>,
    pub added_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogRepositoryState {
    Active,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogProjectComparison {
    pub id: String,
    pub remote_url: String,
    pub directory_name: String,
    pub local_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCatalogSummary {
    pub catalog_path: String,
    pub repositories: Vec<CatalogRepository>,
    pub missing: Vec<CatalogProjectComparison>,
    pub local_only: Vec<CatalogProjectComparison>,
    pub active_count: usize,
    pub missing_count: usize,
    pub local_only_count: usize,
    pub git_sync_available: bool,
    pub git_remote: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSyncResult {
    pub status: TaskStatus,
    pub summary: String,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogError {
    pub kind: CatalogErrorKind,
    pub path: Option<String>,
    pub message: String,
}

impl CatalogError {
    fn io(path: &Path, message: impl Into<String>) -> Self {
        Self {
            kind: CatalogErrorKind::Io,
            path: Some(path.display().to_string()),
            message: message.into(),
        }
    }

    fn invalid_catalog(path: &Path, message: impl Into<String>) -> Self {
        Self {
            kind: CatalogErrorKind::InvalidCatalog,
            path: Some(path.display().to_string()),
            message: message.into(),
        }
    }

    fn workspace(error: WorkspaceError) -> Self {
        Self {
            kind: CatalogErrorKind::Workspace,
            path: Some(error.path),
            message: error.message,
        }
    }

    fn config(error: ConfigError) -> Self {
        Self {
            kind: CatalogErrorKind::Io,
            path: Some(error.path),
            message: error.message,
        }
    }

    fn git(message: impl Into<String>) -> Self {
        Self {
            kind: CatalogErrorKind::Git,
            path: None,
            message: message.into(),
        }
    }

    fn git_operation(error: GitOperationError) -> Self {
        Self {
            kind: CatalogErrorKind::Git,
            path: error.path,
            message: error.message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogErrorKind {
    Io,
    InvalidCatalog,
    Workspace,
    Git,
}

pub fn workspace_catalog_path(workspace_root: impl AsRef<Path>) -> PathBuf {
    workspace_root.as_ref().join(CATALOG_DIR)
}

pub fn load_workspace_catalog_summary_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
) -> Result<WorkspaceCatalogSummary, CatalogError> {
    let workspace_root =
        crate::validate_workspace_root(workspace_root.as_ref()).map_err(CatalogError::workspace)?;
    let repositories = load_catalog_repositories(&workspace_root)?;
    let workspace =
        scan_workspace_at(&workspace_root, agent_profiles).map_err(CatalogError::workspace)?;
    Ok(catalog_summary_from_workspace(
        &workspace_root,
        repositories,
        &workspace,
    ))
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn load_workspace_catalog_summary_command(
    workspace_root: String,
) -> Result<WorkspaceCatalogSummary, CatalogError> {
    let user_config = load_user_config().map_err(CatalogError::config)?;
    load_workspace_catalog_summary_at(workspace_root, &user_config.agent_profiles)
}

pub fn sync_workspace_catalog_from_projects_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
) -> Result<WorkspaceCatalogSummary, CatalogError> {
    let workspace_root =
        crate::validate_workspace_root(workspace_root.as_ref()).map_err(CatalogError::workspace)?;
    let workspace =
        scan_workspace_at(&workspace_root, agent_profiles).map_err(CatalogError::workspace)?;
    let existing = load_catalog_repositories(&workspace_root)?;
    let mut existing_by_id = existing
        .into_iter()
        .map(|record| (record.id.clone(), record))
        .collect::<HashMap<_, _>>();
    let now = unix_timestamp();

    for project in &workspace.projects {
        let Some(remote_url) = project.remote_url.as_ref() else {
            continue;
        };
        let id = catalog_repository_id(remote_url);
        let mut record = existing_by_id
            .remove(&id)
            .unwrap_or_else(|| CatalogRepository {
                schema_version: CATALOG_SCHEMA_VERSION,
                id: id.clone(),
                remote_url: remote_url.clone(),
                directory_name: project.id.clone(),
                state: CatalogRepositoryState::Active,
                branch: None,
                shallow: false,
                skill_path: None,
                added_at: now.clone(),
                updated_at: now.clone(),
            });
        record.schema_version = CATALOG_SCHEMA_VERSION;
        record.remote_url = remote_url.clone();
        record.directory_name = project.id.clone();
        record.state = CatalogRepositoryState::Active;
        record.skill_path = sparse_skill_path_for_project(project);
        record.updated_at = now.clone();
        save_catalog_repository(&workspace_root, &record)?;
        remove_catalog_tombstone(&workspace_root, &record.id)?;
    }

    let repositories = load_catalog_repositories(&workspace_root)?;
    Ok(catalog_summary_from_workspace(
        &workspace_root,
        repositories,
        &workspace,
    ))
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn sync_workspace_catalog_from_projects_command(
    workspace_root: String,
) -> Result<WorkspaceCatalogSummary, CatalogError> {
    let user_config = load_user_config().map_err(CatalogError::config)?;
    sync_workspace_catalog_from_projects_at(workspace_root, &user_config.agent_profiles)
}

pub fn restore_missing_catalog_repositories_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
) -> Result<TaskOperationResult, CatalogError> {
    restore_missing_catalog_repositories_with_mode(workspace_root, agent_profiles, false)
}

pub fn restore_missing_catalog_repositories_background_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
) -> Result<TaskOperationResult, CatalogError> {
    restore_missing_catalog_repositories_with_mode(workspace_root, agent_profiles, true)
}

fn restore_missing_catalog_repositories_with_mode(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    background: bool,
) -> Result<TaskOperationResult, CatalogError> {
    let workspace_root =
        crate::validate_workspace_root(workspace_root.as_ref()).map_err(CatalogError::workspace)?;
    let repositories = load_catalog_repositories(&workspace_root)?;
    let workspace =
        scan_workspace_at(&workspace_root, agent_profiles).map_err(CatalogError::workspace)?;
    let summary = catalog_summary_from_workspace(&workspace_root, repositories.clone(), &workspace);
    let missing_ids = summary
        .missing
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let missing = repositories
        .into_iter()
        .filter(|record| {
            matches!(record.state, CatalogRepositoryState::Active)
                && missing_ids.contains(record.id.as_str())
        })
        .collect::<Vec<_>>();
    let profiles = agent_profiles.to_vec();
    let result_workspace = Arc::new(Mutex::new(None::<Workspace>));
    let result_workspace_for_task = Arc::clone(&result_workspace);
    let workspace_root_for_task = workspace_root.clone();
    let workspace_root_display = workspace_root.display().to_string();

    let workspace_root_for_task_record = workspace_root.display().to_string();
    let summary = format!("Restore {} catalog repositories", missing.len());
    let job = move |context: &mut crate::TaskContext| {
        if missing.is_empty() {
            return TaskOutcome::skipped("No missing catalog repositories.");
        }

        let mut ok = 0usize;
        let mut skipped = 0usize;
        let mut failed = 0usize;
        let mut cancelled = false;
        let mut project_outcomes = Vec::new();

        for item in &missing {
            if context.is_cancelled() {
                cancelled = true;
                context.stderr("cancelled after current repository");
                break;
            }

            let target_path = workspace_root_for_task.join(&item.directory_name);
            let mut outcome = ProjectTaskRecord {
                project_id: item.directory_name.clone(),
                status: TaskStatus::Failed,
                summary: "restore failed".to_string(),
                error: None,
            };

            if target_path.exists() {
                outcome.status = TaskStatus::Skipped;
                outcome.summary = "target path already exists".to_string();
                outcome.error = Some(format!(
                    "Target path '{}' already exists but is not a matching Git repository.",
                    target_path.display()
                ));
                context.stderr(outcome.error.clone().unwrap_or_default());
                skipped += 1;
                project_outcomes.push(outcome);
                continue;
            }

            if item.remote_url.is_empty() {
                outcome.error = Some("Catalog repository has an empty remote URL.".to_string());
                context.stderr(outcome.error.clone().unwrap_or_default());
                failed += 1;
                project_outcomes.push(outcome);
                continue;
            }
            match clone_import_project(
                &workspace_root_for_task,
                &target_path,
                &item.remote_url,
                &item.directory_name,
                item.skill_path.as_deref(),
                item.branch.as_deref(),
                item.shallow,
                RESTORE_CLONE_MAX_ATTEMPTS,
                RESTORE_CLONE_TIMEOUT,
                context,
            ) {
                Ok((stdout, stderr)) => {
                    context.stdout(stdout);
                    context.stderr(stderr);
                    outcome.status = TaskStatus::Succeeded;
                    outcome.summary = "restored".to_string();
                    ok += 1;
                }
                Err((stdout, stderr)) => {
                    context.stdout(stdout);
                    context.stderr(&stderr);
                    outcome.status = TaskStatus::Failed;
                    outcome.summary = "clone failed".to_string();
                    outcome.error = Some(
                        stderr
                            .lines()
                            .next()
                            .unwrap_or("git clone failed")
                            .to_string(),
                    );
                    failed += 1;
                }
            }
            project_outcomes.push(outcome);
        }

        if let Ok(workspace) = scan_workspace_at(&workspace_root_for_task, &profiles) {
            *result_workspace_for_task
                .lock()
                .expect("catalog restore result workspace lock poisoned") = Some(workspace);
        }

        let summary = format!("summary: ok={ok} skipped={skipped} failed={failed}");
        context.stdout(&summary);
        let outcome = if cancelled {
            TaskOutcome::cancelled(format!("Task cancelled. {summary}"))
        } else if failed > 0 {
            TaskOutcome::failed("Some catalog repositories failed to restore", summary)
        } else if ok == 0 {
            TaskOutcome::skipped(summary)
        } else {
            TaskOutcome::succeeded(summary)
        };
        outcome.with_project_outcomes(project_outcomes)
    };
    let task = if background {
        run_workspace_task_background(
            workspace_root_for_task_record,
            TaskKind::RestoreCatalog,
            summary,
            job,
        )
    } else {
        run_workspace_task_blocking(
            workspace_root_for_task_record,
            TaskKind::RestoreCatalog,
            summary,
            job,
        )
    };

    let workspace = result_workspace
        .lock()
        .expect("catalog restore result workspace lock poisoned")
        .clone()
        .unwrap_or_else(|| {
            scan_workspace_at(&workspace_root, agent_profiles).unwrap_or_else(|_| Workspace {
                root: workspace_root_display,
                projects: Vec::new(),
                skills: Vec::new(),
                agent_profiles: Vec::new(),
            })
        });

    Ok(TaskOperationResult { task, workspace })
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn restore_missing_catalog_repositories_command(
    workspace_root: String,
) -> Result<TaskOperationResult, CatalogError> {
    let user_config = load_user_config().map_err(CatalogError::config)?;
    restore_missing_catalog_repositories_background_at(workspace_root, &user_config.agent_profiles)
}

pub fn initialize_catalog_git_sync_at(
    workspace_root: impl AsRef<Path>,
    remote_url: Option<String>,
) -> Result<CatalogSyncResult, CatalogError> {
    let workspace_root =
        crate::validate_workspace_root(workspace_root.as_ref()).map_err(CatalogError::workspace)?;
    ensure_catalog_directories(&workspace_root)?;
    ensure_catalog_gitignore(&workspace_root)?;

    let mut stdout = String::new();
    let mut stderr = String::new();
    if !workspace_root.join(".git").exists() {
        let (out, err) = git_output(&workspace_root, &["init"])?;
        append_output(&mut stdout, &out);
        append_output(&mut stderr, &err);
    }

    if let Some(remote_url) = remote_url
        .as_ref()
        .map(|value| value.trim())
        .filter(|v| !v.is_empty())
    {
        let remotes = Command::new("git")
            .arg("-C")
            .arg(&workspace_root)
            .args(["remote"])
            .output()
            .map_err(|error| CatalogError::io(&workspace_root, error.to_string()))?;
        let remote_exists = String::from_utf8_lossy(&remotes.stdout)
            .lines()
            .any(|remote| remote == "origin");
        let args = if remote_exists {
            vec!["remote", "set-url", "origin", remote_url]
        } else {
            vec!["remote", "add", "origin", remote_url]
        };
        let (out, err) = git_output(&workspace_root, &args)?;
        append_output(&mut stdout, &out);
        append_output(&mut stderr, &err);
    }

    Ok(CatalogSyncResult {
        status: TaskStatus::Succeeded,
        summary: "Catalog Git sync is initialized.".to_string(),
        stdout,
        stderr,
    })
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn initialize_catalog_git_sync_command(
    workspace_root: String,
    remote_url: Option<String>,
) -> Result<CatalogSyncResult, CatalogError> {
    initialize_catalog_git_sync_at(workspace_root, remote_url)
}

pub fn pull_catalog_git_sync_at(
    workspace_root: impl AsRef<Path>,
) -> Result<CatalogSyncResult, CatalogError> {
    let workspace_root =
        crate::validate_workspace_root(workspace_root.as_ref()).map_err(CatalogError::workspace)?;
    if !workspace_root.join(".git").exists() {
        return Err(CatalogError::git(
            "Catalog Git sync is not initialized for this workspace.",
        ));
    }
    remove_generated_gitignore_if_untracked(&workspace_root)?;
    let mut last_error = None;
    for branch in catalog_pull_branch_candidates(&workspace_root) {
        match git_output_owned(
            &workspace_root,
            &[
                "pull".to_string(),
                "--ff-only".to_string(),
                "origin".to_string(),
                branch,
            ],
        ) {
            Ok((stdout, stderr)) => {
                return Ok(CatalogSyncResult {
                    status: TaskStatus::Succeeded,
                    summary: "Catalog updates pulled.".to_string(),
                    stdout,
                    stderr,
                });
            }
            Err(error) => last_error = Some(error),
        }
    }
    if let Some(error) = last_error {
        return Err(error);
    }
    return Err(CatalogError::git(
        "No catalog branch candidates were available.",
    ));
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn pull_catalog_git_sync_command(
    workspace_root: String,
) -> Result<CatalogSyncResult, CatalogError> {
    pull_catalog_git_sync_at(workspace_root)
}

pub fn publish_catalog_git_sync_at(
    workspace_root: impl AsRef<Path>,
) -> Result<CatalogSyncResult, CatalogError> {
    let workspace_root =
        crate::validate_workspace_root(workspace_root.as_ref()).map_err(CatalogError::workspace)?;
    if !workspace_root.join(".git").exists() {
        return Err(CatalogError::git(
            "Catalog Git sync is not initialized for this workspace.",
        ));
    }
    ensure_catalog_gitignore(&workspace_root)?;

    let mut stdout = String::new();
    let mut stderr = String::new();
    let (out, err) = git_output(
        &workspace_root,
        &["add", ".gitignore", ".skilldock/catalog"],
    )?;
    append_output(&mut stdout, &out);
    append_output(&mut stderr, &err);

    let diff = Command::new("git")
        .arg("-C")
        .arg(&workspace_root)
        .args(["diff", "--cached", "--quiet"])
        .output()
        .map_err(|error| CatalogError::io(&workspace_root, error.to_string()))?;
    if diff.status.success() {
        return Ok(CatalogSyncResult {
            status: TaskStatus::Skipped,
            summary: "No catalog changes to publish.".to_string(),
            stdout,
            stderr,
        });
    }

    let (out, err) = git_output(
        &workspace_root,
        &["commit", "-m", "chore: sync skilldock catalog"],
    )?;
    append_output(&mut stdout, &out);
    append_output(&mut stderr, &err);
    let branch = current_branch(&workspace_root)?;
    let (out, err) = git_output_owned(
        &workspace_root,
        &[
            "push".to_string(),
            "-u".to_string(),
            "origin".to_string(),
            branch,
        ],
    )?;
    append_output(&mut stdout, &out);
    append_output(&mut stderr, &err);

    Ok(CatalogSyncResult {
        status: TaskStatus::Succeeded,
        summary: "Catalog changes published.".to_string(),
        stdout,
        stderr,
    })
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn publish_catalog_git_sync_command(
    workspace_root: String,
) -> Result<CatalogSyncResult, CatalogError> {
    publish_catalog_git_sync_at(workspace_root)
}

pub fn sync_workspace_catalog_from_projects_background_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
) -> Result<TaskOperationResult, CatalogError> {
    let workspace_root =
        crate::validate_workspace_root(workspace_root.as_ref()).map_err(CatalogError::workspace)?;
    let profiles = agent_profiles.to_vec();
    let workspace_root_for_task = workspace_root.clone();
    let result_workspace = Arc::new(Mutex::new(None::<Workspace>));
    let result_workspace_for_task = Arc::clone(&result_workspace);

    let task = run_workspace_task_background(
        workspace_root.display().to_string(),
        TaskKind::SyncCatalog,
        "Save local project list to catalog",
        move |_context| match sync_workspace_catalog_from_projects_at(
            &workspace_root_for_task,
            &profiles,
        ) {
            Ok(summary) => {
                if let Ok(workspace) = scan_workspace_at(&workspace_root_for_task, &profiles) {
                    *result_workspace_for_task
                        .lock()
                        .expect("catalog sync result workspace lock poisoned") = Some(workspace);
                }
                TaskOutcome::succeeded(format!(
                    "Catalog has {} active repositories.",
                    summary.active_count
                ))
            }
            Err(error) => TaskOutcome::failed("Catalog sync failed", error.message),
        },
    );
    let workspace = result_workspace
        .lock()
        .expect("catalog sync result workspace lock poisoned")
        .clone()
        .unwrap_or_else(|| {
            scan_workspace_at(&workspace_root, agent_profiles).unwrap_or_else(|_| Workspace {
                root: workspace_root.display().to_string(),
                projects: Vec::new(),
                skills: Vec::new(),
                agent_profiles: Vec::new(),
            })
        });
    Ok(TaskOperationResult { task, workspace })
}

fn catalog_summary_from_workspace(
    workspace_root: &Path,
    repositories: Vec<CatalogRepository>,
    workspace: &Workspace,
) -> WorkspaceCatalogSummary {
    let active_repositories = repositories
        .iter()
        .filter(|record| matches!(record.state, CatalogRepositoryState::Active))
        .collect::<Vec<_>>();
    let local_projects_by_id = workspace
        .projects
        .iter()
        .filter_map(|project| {
            project
                .remote_url
                .as_ref()
                .map(|remote_url| (catalog_repository_id(remote_url), project))
        })
        .collect::<HashMap<_, _>>();
    let active_ids = active_repositories
        .iter()
        .map(|record| record.id.clone())
        .collect::<HashSet<_>>();
    let active_count = active_repositories.len();

    let mut missing = active_repositories
        .iter()
        .filter(|record| !local_projects_by_id.contains_key(&record.id))
        .map(|record| CatalogProjectComparison {
            id: record.id.clone(),
            remote_url: record.remote_url.clone(),
            directory_name: record.directory_name.clone(),
            local_path: None,
        })
        .collect::<Vec<_>>();
    missing.sort_by(|left, right| left.directory_name.cmp(&right.directory_name));

    let mut local_only = workspace
        .projects
        .iter()
        .filter_map(|project| local_only_comparison(project, &active_ids))
        .collect::<Vec<_>>();
    local_only.sort_by(|left, right| left.directory_name.cmp(&right.directory_name));

    let git_remote = git_config_value(workspace_root, "remote.origin.url");
    WorkspaceCatalogSummary {
        catalog_path: workspace_catalog_path(workspace_root).display().to_string(),
        repositories,
        active_count,
        missing_count: missing.len(),
        local_only_count: local_only.len(),
        missing,
        local_only,
        git_sync_available: workspace_root.join(".git").exists(),
        git_remote,
    }
}

fn local_only_comparison(
    project: &Project,
    active_ids: &HashSet<String>,
) -> Option<CatalogProjectComparison> {
    let remote_url = project.remote_url.as_ref()?;
    let id = catalog_repository_id(remote_url);
    if active_ids.contains(&id) {
        return None;
    }
    Some(CatalogProjectComparison {
        id,
        remote_url: remote_url.clone(),
        directory_name: project.id.clone(),
        local_path: Some(project.path.clone()),
    })
}

fn sparse_skill_path_for_project(project: &Project) -> Option<String> {
    let project_path = Path::new(&project.path);
    let (stdout, _) = git_command_output_with_timeout(
        project_path,
        &["sparse-checkout".to_string(), "list".to_string()],
        CATALOG_GIT_TIMEOUT,
    )
    .ok()?;
    let paths = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if paths.len() != 1 {
        return None;
    }
    let skill_path = safe_import_skill_path(paths[0]).ok()?;
    if project_path.join(&skill_path).join("SKILL.md").is_file() {
        Some(skill_path)
    } else {
        None
    }
}

fn load_catalog_repositories(
    workspace_root: &Path,
) -> Result<Vec<CatalogRepository>, CatalogError> {
    ensure_catalog_directories(workspace_root)?;
    let mut records = Vec::new();
    for directory in [
        workspace_root.join(CATALOG_REPOS_DIR),
        workspace_root.join(CATALOG_TOMBSTONES_DIR),
    ] {
        for entry in fs::read_dir(&directory)
            .map_err(|error| CatalogError::io(&directory, error.to_string()))?
        {
            let entry = entry.map_err(|error| CatalogError::io(&directory, error.to_string()))?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let contents = fs::read_to_string(&path)
                .map_err(|error| CatalogError::io(&path, error.to_string()))?;
            let record = serde_json::from_str::<CatalogRepository>(&contents)
                .map_err(|error| CatalogError::invalid_catalog(&path, error.to_string()))?;
            if record.schema_version != CATALOG_SCHEMA_VERSION {
                return Err(CatalogError::invalid_catalog(
                    &path,
                    format!(
                        "unsupported catalog schema version {}",
                        record.schema_version
                    ),
                ));
            }
            safe_project_dir_name(&record.directory_name).map_err(CatalogError::git_operation)?;
            if let Some(skill_path) = record.skill_path.as_deref() {
                safe_import_skill_path(skill_path).map_err(CatalogError::git_operation)?;
            }
            records.push(record);
        }
    }
    records.sort_by(|left, right| left.directory_name.cmp(&right.directory_name));
    Ok(records)
}

fn save_catalog_repository(
    workspace_root: &Path,
    record: &CatalogRepository,
) -> Result<(), CatalogError> {
    ensure_catalog_directories(workspace_root)?;
    safe_project_dir_name(&record.directory_name).map_err(CatalogError::git_operation)?;
    let directory = match record.state {
        CatalogRepositoryState::Active => workspace_root.join(CATALOG_REPOS_DIR),
        CatalogRepositoryState::Removed => workspace_root.join(CATALOG_TOMBSTONES_DIR),
    };
    let path = directory.join(format!("{}.json", catalog_filename(&record.id)));
    let serialized = serde_json::to_string_pretty(record)
        .map_err(|error| CatalogError::io(&path, error.to_string()))?;
    let mut file =
        fs::File::create(&path).map_err(|error| CatalogError::io(&path, error.to_string()))?;
    file.write_all(serialized.as_bytes())
        .map_err(|error| CatalogError::io(&path, error.to_string()))?;
    file.write_all(b"\n")
        .map_err(|error| CatalogError::io(&path, error.to_string()))?;
    Ok(())
}

fn remove_catalog_tombstone(workspace_root: &Path, id: &str) -> Result<(), CatalogError> {
    let path = workspace_root
        .join(CATALOG_TOMBSTONES_DIR)
        .join(format!("{}.json", catalog_filename(id)));
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CatalogError::io(&path, error.to_string())),
    }
}

fn ensure_catalog_directories(workspace_root: &Path) -> Result<(), CatalogError> {
    for directory in [
        workspace_root.join(CATALOG_REPOS_DIR),
        workspace_root.join(CATALOG_TOMBSTONES_DIR),
    ] {
        fs::create_dir_all(&directory)
            .map_err(|error| CatalogError::io(&directory, error.to_string()))?;
    }
    Ok(())
}

fn ensure_catalog_gitignore(workspace_root: &Path) -> Result<(), CatalogError> {
    let path = workspace_root.join(".gitignore");
    let managed = [
        "# SkillDock catalog sync",
        "*",
        "!.gitignore",
        "!.skilldock/",
        "!.skilldock/catalog/",
        "!.skilldock/catalog/**",
        "",
    ]
    .join("\n");
    if path.exists() {
        let current = fs::read_to_string(&path)
            .map_err(|error| CatalogError::io(&path, error.to_string()))?;
        if current.contains("# SkillDock catalog sync") {
            return Ok(());
        }
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|error| CatalogError::io(&path, error.to_string()))?;
        file.write_all(b"\n")
            .and_then(|_| file.write_all(managed.as_bytes()))
            .map_err(|error| CatalogError::io(&path, error.to_string()))?;
        return Ok(());
    }
    fs::write(&path, managed).map_err(|error| CatalogError::io(&path, error.to_string()))
}

fn remove_generated_gitignore_if_untracked(workspace_root: &Path) -> Result<(), CatalogError> {
    let path = workspace_root.join(".gitignore");
    if !path.exists() {
        return Ok(());
    }
    let tracked = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(["ls-files", "--error-unmatch", ".gitignore"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if tracked {
        return Ok(());
    }
    let contents =
        fs::read_to_string(&path).map_err(|error| CatalogError::io(&path, error.to_string()))?;
    if contents.contains("# SkillDock catalog sync") {
        fs::remove_file(&path).map_err(|error| CatalogError::io(&path, error.to_string()))?;
    }
    Ok(())
}

fn catalog_repository_id(remote_url: &str) -> String {
    remote_url
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_ascii_lowercase()
}

fn catalog_filename(id: &str) -> String {
    let mut output = String::new();
    for ch in id.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    while output.contains("__") {
        output = output.replace("__", "_");
    }
    output.trim_matches('_').to_string()
}

fn unix_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn git_output(path: &Path, args: &[&str]) -> Result<(String, String), CatalogError> {
    git_output_owned(
        path,
        &args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>(),
    )
}

fn git_output_owned(path: &Path, args: &[String]) -> Result<(String, String), CatalogError> {
    git_command_output_with_timeout(path, args, CATALOG_GIT_TIMEOUT).map_err(|(_stdout, stderr)| {
        CatalogError::git(stderr.lines().next().unwrap_or("git command failed"))
    })
}

fn current_branch(path: &Path) -> Result<String, CatalogError> {
    let (stdout, _) = git_output(path, &["branch", "--show-current"])?;
    let branch = stdout.trim();
    if branch.is_empty() {
        Ok("main".to_string())
    } else {
        Ok(branch.to_string())
    }
}

fn remote_head_branch(path: &Path) -> Option<String> {
    let (stdout, _) = git_command_output_with_timeout(
        path,
        &[
            "ls-remote".to_string(),
            "--symref".to_string(),
            "origin".to_string(),
            "HEAD".to_string(),
        ],
        CATALOG_GIT_TIMEOUT,
    )
    .ok()?;
    stdout.lines().find_map(|line| {
        let prefix = "ref: refs/heads/";
        let suffix = "\tHEAD";
        if line.starts_with(prefix) && line.ends_with(suffix) {
            Some(
                line.trim_start_matches(prefix)
                    .trim_end_matches(suffix)
                    .to_string(),
            )
        } else {
            None
        }
    })
}

fn catalog_pull_branch_candidates(path: &Path) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(branch) = remote_head_branch(path) {
        candidates.push(branch);
    }
    if let Ok(branch) = current_branch(path) {
        candidates.push(branch);
    }
    candidates.push("main".to_string());
    candidates.push("master".to_string());

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|branch| !branch.is_empty() && seen.insert(branch.clone()))
        .collect()
}

fn git_config_value(path: &Path, key: &str) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["config", "--get", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn append_output(target: &mut String, value: &str) {
    if value.is_empty() {
        return;
    }
    target.push_str(value);
    if !target.ends_with('\n') {
        target.push('\n');
    }
}
