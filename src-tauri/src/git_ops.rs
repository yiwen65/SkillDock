use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{
    catalog_skill_paths_for_project, is_pull_all_eligible, load_user_config,
    run_workspace_task_background, run_workspace_task_blocking, scan_workspace_at,
    tombstone_catalog_project_at, upsert_catalog_project_at, AgentProfile, ConfigError, GitStatus,
    ImportProjectRequest, Project, ProjectTaskRecord, PullAllProjectsRequest, PullProjectRequest,
    TaskKind, TaskOperationResult, TaskOutcome, TaskRecord, TaskStatus, Workspace, WorkspaceError,
};

const IMPORT_CLONE_MAX_ATTEMPTS: usize = 3;
const IMPORT_CLONE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProjectPlan {
    pub remote_url: String,
    pub directory_name: String,
    pub target_path: String,
    pub will_clone: bool,
    pub skill_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitOperationError {
    pub kind: GitOperationErrorKind,
    pub path: Option<String>,
    pub message: String,
}

impl GitOperationError {
    fn invalid_repository(message: impl Into<String>) -> Self {
        Self {
            kind: GitOperationErrorKind::InvalidRepository,
            path: None,
            message: message.into(),
        }
    }

    fn invalid_directory_name(name: &str) -> Self {
        Self {
            kind: GitOperationErrorKind::InvalidDirectoryName,
            path: None,
            message: format!("Project directory name '{name}' is not a safe single path segment."),
        }
    }

    fn invalid_skill_path(path: &str) -> Self {
        Self {
            kind: GitOperationErrorKind::InvalidRepository,
            path: None,
            message: format!("Skill path '{path}' is not a safe relative path."),
        }
    }

    fn workspace(error: WorkspaceError) -> Self {
        Self {
            kind: GitOperationErrorKind::Workspace,
            path: Some(error.path),
            message: error.message,
        }
    }

    fn config(error: ConfigError) -> Self {
        Self {
            kind: GitOperationErrorKind::Io,
            path: Some(error.path),
            message: error.message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitOperationErrorKind {
    InvalidRepository,
    InvalidDirectoryName,
    Workspace,
    Io,
}

pub fn plan_import_project(
    workspace_root: impl AsRef<Path>,
    request: &ImportProjectRequest,
) -> Result<ImportProjectPlan, GitOperationError> {
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())
        .map_err(GitOperationError::workspace)?;
    let normalized_source = normalize_import_source(&request.source)?;
    let remote_url = normalized_source.remote_url;
    let mut skill_path = request
        .skill_path
        .as_deref()
        .map(safe_import_skill_path)
        .transpose()?
        .or(normalized_source.skill_path);
    let mut directory_name_is_explicit = false;
    let directory_name = match request.directory_name.as_deref() {
        Some(directory_name) => match safe_project_dir_name(directory_name) {
            Ok(directory_name) => {
                directory_name_is_explicit = true;
                directory_name
            }
            Err(error) => {
                if skill_path.is_none() && looks_like_misplaced_skill_path(directory_name) {
                    if let Ok(path) = safe_import_skill_path(directory_name) {
                        skill_path = Some(path);
                        infer_import_directory_name(&remote_url, skill_path.as_deref())?
                    } else {
                        return Err(error);
                    }
                } else {
                    return Err(error);
                }
            }
        },
        None => infer_import_directory_name(&remote_url, skill_path.as_deref())?,
    };
    let directory_name = if directory_name_is_explicit {
        directory_name
    } else {
        disambiguate_default_directory_name(&workspace_root, &directory_name, &remote_url)?
    };
    let target_path = workspace_root.join(&directory_name);
    let will_clone = !target_path.exists();

    Ok(ImportProjectPlan {
        remote_url,
        directory_name,
        target_path: target_path.display().to_string(),
        will_clone,
        skill_path,
    })
}

pub fn import_project_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: ImportProjectRequest,
) -> Result<TaskOperationResult, GitOperationError> {
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())
        .map_err(GitOperationError::workspace)?;
    let plan = plan_import_project(&workspace_root, &request)?;
    let target_path = PathBuf::from(&plan.target_path);
    let profiles = agent_profiles.to_vec();
    let result_workspace = Arc::new(Mutex::new(None::<Workspace>));
    let result_workspace_for_task = Arc::clone(&result_workspace);
    let workspace_root_for_task = workspace_root.clone();

    let task = run_import_project_task(
        false,
        workspace_root.display().to_string(),
        plan,
        target_path,
        request.shallow,
        profiles,
        result_workspace_for_task,
        workspace_root_for_task,
    );

    let workspace = result_workspace
        .lock()
        .expect("import result workspace lock poisoned")
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

pub fn import_project_background_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: ImportProjectRequest,
) -> Result<TaskOperationResult, GitOperationError> {
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())
        .map_err(GitOperationError::workspace)?;
    let plan = plan_import_project(&workspace_root, &request)?;
    let target_path = PathBuf::from(&plan.target_path);
    let profiles = agent_profiles.to_vec();
    let result_workspace = Arc::new(Mutex::new(None::<Workspace>));
    let result_workspace_for_task = Arc::clone(&result_workspace);
    let workspace_root_for_task = workspace_root.clone();

    let task = run_import_project_task(
        true,
        workspace_root.display().to_string(),
        plan,
        target_path,
        request.shallow,
        profiles,
        result_workspace_for_task,
        workspace_root_for_task,
    );

    let workspace = result_workspace
        .lock()
        .expect("import result workspace lock poisoned")
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

fn run_import_project_task(
    background: bool,
    workspace_root_display: String,
    plan: ImportProjectPlan,
    target_path: PathBuf,
    shallow: bool,
    profiles: Vec<AgentProfile>,
    result_workspace_for_task: Arc<Mutex<Option<Workspace>>>,
    workspace_root_for_task: PathBuf,
) -> TaskRecord {
    let summary = format!("Import {}", plan.directory_name);
    let job = move |context: &mut crate::TaskContext| {
        let outcome = if target_path.exists() {
            if is_git_repository(&target_path) {
                if let Some(existing_remote) =
                    existing_origin_remote_if_different(&target_path, &plan.remote_url)
                {
                    return TaskOutcome::failed(
                        format!("Import blocked for {}", plan.directory_name),
                        format!(
                            "Project directory '{}' already points at a different remote: {}",
                            plan.directory_name, existing_remote
                        ),
                    );
                }
                if let Some(skill_path) = plan.skill_path.as_deref() {
                    if let Err(message) = ensure_sparse_skill_exists(&target_path, skill_path) {
                        match add_skill_to_existing_sparse_checkout(
                            &target_path,
                            skill_path,
                            IMPORT_CLONE_TIMEOUT,
                            context,
                        ) {
                            Ok((stdout, stderr)) => {
                                let outcome = TaskOutcome::succeeded(format!(
                                    "Added {} to {}",
                                    skill_path, plan.directory_name
                                ))
                                .with_stdout(stdout)
                                .with_stderr(stderr);
                                return finalize_import_outcome(
                                    outcome,
                                    &workspace_root_for_task,
                                    &profiles,
                                    &result_workspace_for_task,
                                    &plan.directory_name,
                                    context,
                                );
                            }
                            Err((stdout, stderr)) => {
                                return TaskOutcome::failed(
                                    format!("Import blocked for {}", plan.directory_name),
                                    stderr.lines().next().unwrap_or(&message).to_string(),
                                )
                                .with_stdout(stdout)
                                .with_stderr(stderr);
                            }
                        }
                    }
                }
                context.stdout(format!(
                    "adopt existing Git directory: {}",
                    plan.directory_name
                ));
                TaskOutcome::succeeded(format!("Adopted {}", plan.directory_name))
            } else {
                TaskOutcome::failed(
                    format!("Import blocked for {}", plan.directory_name),
                    format!(
                        "Target path '{}' is an existing non-Git directory.",
                        target_path.display()
                    ),
                )
            }
        } else {
            match clone_import_project(
                &workspace_root_for_task,
                &target_path,
                &plan.remote_url,
                &plan.directory_name,
                plan.skill_path.as_deref(),
                None,
                shallow,
                IMPORT_CLONE_MAX_ATTEMPTS,
                IMPORT_CLONE_TIMEOUT,
                context,
            ) {
                Ok((stdout, stderr)) => {
                    TaskOutcome::succeeded(format!("Imported {}", plan.directory_name))
                        .with_stdout(stdout)
                        .with_stderr(stderr)
                }
                Err((stdout, stderr)) => TaskOutcome::failed(
                    format!("Import failed for {}", plan.directory_name),
                    stderr
                        .lines()
                        .next()
                        .unwrap_or("git clone failed")
                        .to_string(),
                )
                .with_stdout(stdout)
                .with_stderr(stderr),
            }
        };

        finalize_import_outcome(
            outcome,
            &workspace_root_for_task,
            &profiles,
            &result_workspace_for_task,
            &plan.directory_name,
            context,
        )
    };

    if background {
        run_workspace_task_background(
            workspace_root_display,
            TaskKind::ImportProject,
            summary,
            job,
        )
    } else {
        run_workspace_task_blocking(
            workspace_root_display,
            TaskKind::ImportProject,
            summary,
            job,
        )
    }
}

fn finalize_import_outcome(
    outcome: TaskOutcome,
    workspace_root: &Path,
    profiles: &[AgentProfile],
    result_workspace_for_task: &Arc<Mutex<Option<Workspace>>>,
    directory_name: &str,
    context: &mut crate::TaskContext,
) -> TaskOutcome {
    if !matches!(outcome.status(), TaskStatus::Succeeded) {
        return outcome;
    }

    match scan_workspace_at(workspace_root, profiles) {
        Ok(workspace) => {
            if let Some(project) = workspace
                .projects
                .iter()
                .find(|project| project.id == directory_name)
            {
                match upsert_catalog_project_at(workspace_root, project) {
                    Ok(true) => context.stdout(format!("catalog updated: {}", project.id)),
                    Ok(false) => context.stdout(format!(
                        "catalog unchanged: {} has no origin remote",
                        project.id
                    )),
                    Err(error) => context.stderr(format!(
                        "catalog update failed for {}: {}",
                        project.id, error.message
                    )),
                }
            } else {
                context.stderr(format!(
                    "catalog update skipped: imported project '{}' was not found after scan",
                    directory_name
                ));
            }
            *result_workspace_for_task
                .lock()
                .expect("import result workspace lock poisoned") = Some(workspace);
        }
        Err(error) => context.stderr(format!(
            "catalog update skipped: workspace scan failed after import: {}",
            error.message
        )),
    }

    outcome
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn import_project_command(
    workspace_root: String,
    request: ImportProjectRequest,
) -> Result<TaskOperationResult, GitOperationError> {
    let user_config = load_user_config().map_err(GitOperationError::config)?;
    import_project_background_at(workspace_root, &user_config.agent_profiles, request)
}

pub fn delete_project_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    project_id: &str,
) -> Result<TaskOperationResult, GitOperationError> {
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())
        .map_err(GitOperationError::workspace)?;
    safe_project_dir_name(project_id)?;
    let target_path = workspace_root.join(project_id);
    if !target_path.exists() {
        return Err(GitOperationError {
            kind: GitOperationErrorKind::InvalidRepository,
            path: Some(target_path.display().to_string()),
            message: format!("Project directory '{}' does not exist.", project_id),
        });
    }
    let canonical_target = fs::canonicalize(&target_path).map_err(|e| {
        GitOperationError::workspace(WorkspaceError::io(&target_path, e.to_string()))
    })?;
    if !canonical_target.starts_with(&workspace_root) {
        return Err(GitOperationError::workspace(
            WorkspaceError::outside_workspace(&canonical_target),
        ));
    }

    let profiles = agent_profiles.to_vec();
    let result_workspace = Arc::new(Mutex::new(None::<Workspace>));
    let result_workspace_for_task = Arc::clone(&result_workspace);
    let workspace_root_for_task = workspace_root.clone();
    let project_id_owned = project_id.to_string();
    let project_before_delete = scan_workspace_at(&workspace_root, agent_profiles)
        .ok()
        .and_then(|workspace| {
            workspace
                .projects
                .into_iter()
                .find(|project| project.id == project_id)
        });

    let task = run_workspace_task_blocking(
        workspace_root.display().to_string(),
        TaskKind::DeleteProject,
        format!("Delete {}", project_id_owned),
        move |context| {
            context.stdout(format!(
                "Removing directory: {}",
                canonical_target.display()
            ));
            match fs::remove_dir_all(&canonical_target) {
                Ok(()) => {
                    if let Some(project) = project_before_delete.as_ref() {
                        match tombstone_catalog_project_at(&workspace_root_for_task, project) {
                            Ok(true) => {
                                context.stdout(format!("catalog tombstoned: {}", project.id))
                            }
                            Ok(false) => context.stdout(format!(
                                "catalog unchanged: {} had no origin remote",
                                project.id
                            )),
                            Err(error) => context.stderr(format!(
                                "catalog tombstone failed for {}: {}",
                                project.id, error.message
                            )),
                        }
                    } else {
                        context.stderr(format!(
                            "catalog tombstone skipped: project '{}' was not found before delete",
                            project_id_owned
                        ));
                    }
                    if let Ok(workspace) = scan_workspace_at(&workspace_root_for_task, &profiles) {
                        *result_workspace_for_task
                            .lock()
                            .expect("delete result workspace lock poisoned") = Some(workspace);
                    }
                    TaskOutcome::succeeded(format!("Deleted {}", project_id_owned))
                }
                Err(err) => TaskOutcome::failed(
                    format!("Delete failed for {}", project_id_owned),
                    err.to_string(),
                ),
            }
        },
    );

    let workspace = result_workspace
        .lock()
        .expect("delete result workspace lock poisoned")
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

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn delete_project_command(
    workspace_root: String,
    project_id: String,
) -> Result<TaskOperationResult, GitOperationError> {
    let user_config = load_user_config().map_err(GitOperationError::config)?;
    delete_project_at(workspace_root, &user_config.agent_profiles, &project_id)
}

pub fn check_project_updates_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    project_id: &str,
) -> Result<TaskOperationResult, GitOperationError> {
    run_project_git_task_with_mode(
        workspace_root,
        agent_profiles,
        TaskKind::FetchProject,
        format!("Check updates for {project_id}"),
        project_id.to_string(),
        ProjectTaskMode::Blocking,
        |project, context| fetch_project(project, context),
    )
}

pub fn check_project_updates_background_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    project_id: &str,
) -> Result<TaskOperationResult, GitOperationError> {
    run_project_git_task_with_mode(
        workspace_root,
        agent_profiles,
        TaskKind::FetchProject,
        format!("Check updates for {project_id}"),
        project_id.to_string(),
        ProjectTaskMode::Background,
        |project, context| fetch_project(project, context),
    )
}

pub fn check_all_project_updates_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
) -> Result<TaskOperationResult, GitOperationError> {
    run_all_project_git_task(
        workspace_root,
        agent_profiles,
        TaskKind::SyncAllProjects,
        "Check updates for all projects",
        |project, context| fetch_project(project, context),
    )
}

pub fn check_all_project_updates_background_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
) -> Result<TaskOperationResult, GitOperationError> {
    run_all_project_git_task_with_mode(
        workspace_root,
        agent_profiles,
        TaskKind::SyncAllProjects,
        "Check updates for all projects",
        AllProjectTaskMode::Background,
        |project, context| fetch_project(project, context),
    )
}

pub fn pull_project_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: PullProjectRequest,
) -> Result<TaskOperationResult, GitOperationError> {
    run_project_git_task_with_mode(
        workspace_root,
        agent_profiles,
        TaskKind::PullProject,
        format!("Pull {}", request.project_id),
        request.project_id,
        ProjectTaskMode::Blocking,
        move |project, context| pull_project(project, context),
    )
}

pub fn pull_project_background_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: PullProjectRequest,
) -> Result<TaskOperationResult, GitOperationError> {
    run_project_git_task_with_mode(
        workspace_root,
        agent_profiles,
        TaskKind::PullProject,
        format!("Pull {}", request.project_id),
        request.project_id,
        ProjectTaskMode::Background,
        move |project, context| pull_project(project, context),
    )
}

pub fn pull_all_projects_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: PullAllProjectsRequest,
) -> Result<TaskOperationResult, GitOperationError> {
    let safe_project_ids = request
        .safe_project_ids
        .map(|project_ids| project_ids.into_iter().collect::<HashSet<_>>());
    run_all_project_git_task(
        workspace_root,
        agent_profiles,
        TaskKind::SyncAllProjects,
        "Pull all safe projects",
        move |project, context| {
            if let Some(safe_project_ids) = safe_project_ids.as_ref() {
                if !safe_project_ids.contains(&project.id) {
                    context.stdout(format!(
                        "skip: {} was not marked safe by current project status",
                        project.id
                    ));
                    return project_outcome(
                        project,
                        TaskStatus::Skipped,
                        "not marked safe by current project status",
                        None,
                    );
                }
            }
            pull_safe_project(project, context)
        },
    )
}

pub fn pull_all_projects_background_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: PullAllProjectsRequest,
) -> Result<TaskOperationResult, GitOperationError> {
    let safe_project_ids = request
        .safe_project_ids
        .map(|project_ids| project_ids.into_iter().collect::<HashSet<_>>());
    run_all_project_git_task_with_mode(
        workspace_root,
        agent_profiles,
        TaskKind::SyncAllProjects,
        "Pull all safe projects",
        AllProjectTaskMode::Background,
        move |project, context| {
            if let Some(safe_project_ids) = safe_project_ids.as_ref() {
                if !safe_project_ids.contains(&project.id) {
                    context.stdout(format!(
                        "skip: {} was not marked safe by current project status",
                        project.id
                    ));
                    return project_outcome(
                        project,
                        TaskStatus::Skipped,
                        "not marked safe by current project status",
                        None,
                    );
                }
            }
            pull_safe_project(project, context)
        },
    )
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn check_project_updates_command(
    workspace_root: String,
    project_id: String,
) -> Result<TaskOperationResult, GitOperationError> {
    let user_config = load_user_config().map_err(GitOperationError::config)?;
    check_project_updates_background_at(workspace_root, &user_config.agent_profiles, &project_id)
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn check_all_project_updates_command(
    workspace_root: String,
) -> Result<TaskOperationResult, GitOperationError> {
    let user_config = load_user_config().map_err(GitOperationError::config)?;
    check_all_project_updates_background_at(workspace_root, &user_config.agent_profiles)
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn pull_project_command(
    workspace_root: String,
    request: PullProjectRequest,
) -> Result<TaskOperationResult, GitOperationError> {
    let user_config = load_user_config().map_err(GitOperationError::config)?;
    pull_project_background_at(workspace_root, &user_config.agent_profiles, request)
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn pull_all_projects_command(
    workspace_root: String,
    request: PullAllProjectsRequest,
) -> Result<TaskOperationResult, GitOperationError> {
    let user_config = load_user_config().map_err(GitOperationError::config)?;
    pull_all_projects_background_at(workspace_root, &user_config.agent_profiles, request)
}

enum ProjectTaskMode {
    Blocking,
    Background,
}

fn run_project_git_task_with_mode<F>(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    kind: TaskKind,
    summary: String,
    project_id: String,
    mode: ProjectTaskMode,
    operation: F,
) -> Result<TaskOperationResult, GitOperationError>
where
    F: FnOnce(&Project, &mut crate::TaskContext) -> ProjectTaskOutcome + Send + 'static,
{
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())
        .map_err(GitOperationError::workspace)?;
    let workspace =
        scan_workspace_at(&workspace_root, agent_profiles).map_err(GitOperationError::workspace)?;
    let project = workspace
        .projects
        .iter()
        .find(|project| project.id == project_id)
        .cloned()
        .ok_or_else(|| GitOperationError::invalid_repository("Project was not found."))?;
    let profiles = agent_profiles.to_vec();
    let result_workspace = Arc::new(Mutex::new(None::<Workspace>));
    let result_workspace_for_task = Arc::clone(&result_workspace);
    let workspace_root_for_task = workspace_root.clone();
    let workspace_root_display = workspace_root.display().to_string();
    let job = move |context: &mut crate::TaskContext| {
        let result = operation(&project, context);
        let mut workspace = scan_workspace_at(&workspace_root_for_task, &profiles);
        if let Ok(workspace) = workspace.as_mut() {
            if let Some(status) = result.project_status.clone() {
                if let Some(project) = workspace
                    .projects
                    .iter_mut()
                    .find(|project| project.id == result.project_id)
                {
                    project.git_status = status;
                    project.pull_all_eligible =
                        is_pull_all_eligible(&project.git_status, project.upstream.as_deref());
                }
            }
        }
        if let Ok(workspace) = workspace {
            *result_workspace_for_task
                .lock()
                .expect("git result workspace lock poisoned") = Some(workspace);
        }
        result.into_task_outcome()
    };
    let task = match mode {
        ProjectTaskMode::Blocking => {
            run_workspace_task_blocking(workspace_root_display, kind, summary, job)
        }
        ProjectTaskMode::Background => {
            run_workspace_task_background(workspace_root_display, kind, summary, job)
        }
    };
    let workspace = result_workspace
        .lock()
        .expect("git result workspace lock poisoned")
        .clone()
        .unwrap_or(workspace);
    Ok(TaskOperationResult { task, workspace })
}

fn run_all_project_git_task<F>(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    kind: TaskKind,
    summary: impl Into<String>,
    operation: F,
) -> Result<TaskOperationResult, GitOperationError>
where
    F: Fn(&Project, &mut crate::TaskContext) -> ProjectTaskOutcome + Send + 'static,
{
    run_all_project_git_task_with_mode(
        workspace_root,
        agent_profiles,
        kind,
        summary,
        AllProjectTaskMode::Blocking,
        operation,
    )
}

enum AllProjectTaskMode {
    Blocking,
    Background,
}

fn run_all_project_git_task_with_mode<F>(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    kind: TaskKind,
    summary: impl Into<String>,
    mode: AllProjectTaskMode,
    operation: F,
) -> Result<TaskOperationResult, GitOperationError>
where
    F: Fn(&Project, &mut crate::TaskContext) -> ProjectTaskOutcome + Send + 'static,
{
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())
        .map_err(GitOperationError::workspace)?;
    let workspace =
        scan_workspace_at(&workspace_root, agent_profiles).map_err(GitOperationError::workspace)?;
    let projects = workspace.projects.clone();
    let profiles = agent_profiles.to_vec();
    let result_workspace = Arc::new(Mutex::new(None::<Workspace>));
    let result_workspace_for_task = Arc::clone(&result_workspace);
    let workspace_root_for_task = workspace_root.clone();
    let workspace_root_display = workspace_root.display().to_string();
    let summary = summary.into();

    let job = move |context: &mut crate::TaskContext| {
        let mut ok = 0usize;
        let mut skipped = 0usize;
        let mut failed = 0usize;
        let mut cancelled = false;
        let mut status_overrides = Vec::new();
        let mut project_outcomes = Vec::new();

        for project in &projects {
            if context.is_cancelled() {
                context.stderr("cancelled after current substep");
                cancelled = true;
                break;
            }
            let result = operation(project, context);
            if let Some(status) = result.project_status.clone() {
                status_overrides.push((result.project_id.clone(), status));
            }
            project_outcomes.push(result.to_project_task_record());
            match result.status {
                TaskStatus::Succeeded => ok += 1,
                TaskStatus::Skipped => skipped += 1,
                TaskStatus::Failed => failed += 1,
                TaskStatus::Cancelled => {
                    cancelled = true;
                    break;
                }
                _ => {}
            }
        }
        if !cancelled && context.is_cancelled() {
            context.stderr("cancelled after current substep");
            cancelled = true;
        }

        let mut workspace = scan_workspace_at(&workspace_root_for_task, &profiles);
        if let Ok(workspace) = workspace.as_mut() {
            for (project_id, status) in status_overrides {
                if let Some(project) = workspace
                    .projects
                    .iter_mut()
                    .find(|project| project.id == project_id)
                {
                    project.git_status = status;
                    project.pull_all_eligible =
                        is_pull_all_eligible(&project.git_status, project.upstream.as_deref());
                }
            }
        }
        if let Ok(workspace) = workspace {
            *result_workspace_for_task
                .lock()
                .expect("git result workspace lock poisoned") = Some(workspace);
        }
        if !cancelled && context.is_cancelled() {
            context.stderr("cancelled after current substep");
            cancelled = true;
        }

        finish_all_project_task_outcome(ok, skipped, failed, cancelled, project_outcomes, context)
    };
    let task = match mode {
        AllProjectTaskMode::Blocking => {
            run_workspace_task_blocking(workspace_root_display, kind, summary, job)
        }
        AllProjectTaskMode::Background => {
            run_workspace_task_background(workspace_root_display, kind, summary, job)
        }
    };
    let workspace = result_workspace
        .lock()
        .expect("git result workspace lock poisoned")
        .clone()
        .unwrap_or(workspace);
    Ok(TaskOperationResult { task, workspace })
}

fn finish_all_project_task_outcome(
    ok: usize,
    skipped: usize,
    failed: usize,
    cancelled: bool,
    project_outcomes: Vec<ProjectTaskRecord>,
    context: &mut crate::TaskContext,
) -> TaskOutcome {
    let summary = format!("summary: ok={ok} skipped={skipped} failed={failed}");
    context.stdout(&summary);
    let outcome = if cancelled {
        TaskOutcome::cancelled(format!("Task cancelled. {summary}"))
    } else if failed > 0 {
        TaskOutcome::failed("Some projects failed", summary)
    } else if ok == 0 && skipped > 0 {
        TaskOutcome::skipped(summary)
    } else {
        TaskOutcome::succeeded(summary)
    };
    outcome.with_project_outcomes(project_outcomes)
}

struct ProjectTaskOutcome {
    project_id: String,
    status: TaskStatus,
    summary: String,
    error: Option<String>,
    project_status: Option<GitStatus>,
}

impl ProjectTaskOutcome {
    fn into_task_outcome(self) -> TaskOutcome {
        let project_outcomes = vec![self.to_project_task_record()];
        let outcome = match self.status {
            TaskStatus::Succeeded => TaskOutcome::succeeded(self.summary.clone()),
            TaskStatus::Skipped => TaskOutcome::skipped(self.summary.clone()),
            TaskStatus::Failed => TaskOutcome::failed(
                self.summary.clone(),
                self.error
                    .clone()
                    .unwrap_or_else(|| "git operation failed".to_string()),
            ),
            TaskStatus::Cancelled => TaskOutcome::cancelled(self.summary.clone()),
            _ => TaskOutcome::failed(self.summary.clone(), "invalid task outcome status"),
        };
        outcome.with_project_outcomes(project_outcomes)
    }

    fn to_project_task_record(&self) -> ProjectTaskRecord {
        ProjectTaskRecord {
            project_id: self.project_id.clone(),
            status: self.status.clone(),
            summary: self.summary.clone(),
            error: self.error.clone(),
        }
    }
}

fn fetch_project(project: &Project, context: &mut crate::TaskContext) -> ProjectTaskOutcome {
    if matches!(project.git_status, GitStatus::Detached) {
        context.stdout(format!("skip: {} is detached", project.id));
        return project_outcome(
            project,
            TaskStatus::Skipped,
            "detached",
            Some(GitStatus::Detached),
        );
    }
    if project.upstream.is_none() {
        context.stdout(format!("skip: {} has no upstream", project.id));
        return project_outcome(
            project,
            TaskStatus::Skipped,
            "no upstream",
            Some(GitStatus::NoUpstream),
        );
    }

    context.stdout(format!("git -C {} fetch --prune", project.path));
    match git_command_output(
        Path::new(&project.path),
        &["fetch".to_string(), "--prune".to_string()],
    ) {
        Ok((stdout, stderr)) => {
            context.stdout(stdout);
            context.stderr(stderr);
            project_outcome(project, TaskStatus::Succeeded, "fetch succeeded", None)
        }
        Err((stdout, stderr)) => {
            context.stdout(stdout);
            context.stderr(&stderr);
            project_outcome(
                project,
                TaskStatus::Failed,
                "fetch failed",
                Some(GitStatus::FetchFailed),
            )
            .with_error(stderr)
        }
    }
}

fn pull_project(project: &Project, context: &mut crate::TaskContext) -> ProjectTaskOutcome {
    if matches!(project.git_status, GitStatus::Detached) {
        context.stdout(format!("skip: {} is detached", project.id));
        return project_outcome(project, TaskStatus::Skipped, "detached", None);
    }
    if project.upstream.is_none() {
        context.stdout(format!("skip: {} has no upstream", project.id));
        return project_outcome(project, TaskStatus::Skipped, "no upstream", None);
    }

    context.stdout(format!(
        "remote authoritative pull: {} will be reset to {}",
        project.id,
        project.upstream.as_deref().unwrap_or("@{u}")
    ));

    let project_path = Path::new(&project.path);
    let fetch_args = vec!["fetch".to_string(), "--prune".to_string()];
    context.stdout(format!("git -C {} {}", project.path, fetch_args.join(" ")));
    match git_command_output(project_path, &fetch_args) {
        Ok((stdout, stderr)) => {
            context.stdout(stdout);
            context.stderr(stderr);
        }
        Err((stdout, stderr)) => {
            context.stdout(stdout);
            context.stderr(&stderr);
            return project_outcome(project, TaskStatus::Failed, "pull failed", None)
                .with_error(stderr);
        }
    }

    if let Err((stdout, stderr)) = align_sparse_checkout_with_catalog(project, context) {
        context.stdout(stdout);
        context.stderr(&stderr);
        return project_outcome(project, TaskStatus::Failed, "pull failed", None)
            .with_error(stderr);
    }

    for args in [
        vec![
            "reset".to_string(),
            "--hard".to_string(),
            "@{u}".to_string(),
        ],
        vec!["clean".to_string(), "-fd".to_string()],
    ] {
        context.stdout(format!("git -C {} {}", project.path, args.join(" ")));
        match git_command_output(project_path, &args) {
            Ok((stdout, stderr)) => {
                context.stdout(stdout);
                context.stderr(stderr);
            }
            Err((stdout, stderr)) => {
                context.stdout(stdout);
                context.stderr(&stderr);
                return project_outcome(project, TaskStatus::Failed, "pull failed", None)
                    .with_error(stderr);
            }
        }
    }

    project_outcome(project, TaskStatus::Succeeded, "pull succeeded", None)
}

fn align_sparse_checkout_with_catalog(
    project: &Project,
    context: &mut crate::TaskContext,
) -> Result<(), (String, String)> {
    let Some(workspace_root) = Path::new(&project.path).parent() else {
        return Ok(());
    };
    let skill_paths = catalog_skill_paths_for_project(workspace_root, project)
        .map_err(|error| (String::new(), error.message))?;
    if skill_paths.is_empty() {
        return Ok(());
    }

    context.stdout(format!(
        "catalog sparse skill paths: {}",
        skill_paths.join(", ")
    ));
    let mut args = vec![
        "sparse-checkout".to_string(),
        "set".to_string(),
        "--cone".to_string(),
    ];
    args.extend(skill_paths);
    context.stdout(format!("git -C {} {}", project.path, args.join(" ")));
    git_command_output(Path::new(&project.path), &args).map(|_| ())
}

fn pull_safe_project(project: &Project, context: &mut crate::TaskContext) -> ProjectTaskOutcome {
    if !project.pull_all_eligible {
        context.stdout(format!(
            "skip: {} is {} and is not safe for pull-all",
            project.id,
            git_status_label(&project.git_status)
        ));
        return project_outcome(project, TaskStatus::Skipped, "not safe for pull-all", None);
    }

    pull_project(project, context)
}

fn git_status_label(status: &GitStatus) -> &'static str {
    match status {
        GitStatus::UpToDate => "up to date",
        GitStatus::Behind => "behind",
        GitStatus::Ahead => "ahead",
        GitStatus::Diverged => "diverged",
        GitStatus::Dirty => "dirty",
        GitStatus::NoUpstream => "no upstream",
        GitStatus::Detached => "detached",
        GitStatus::FetchFailed => "fetch failed",
        GitStatus::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TaskQueue;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "skilldock_git_ops_{name}_{}_{}",
            std::process::id(),
            unique
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_project(workspace: &Path, name: &str) -> PathBuf {
        let project = workspace.join(name);
        fs::create_dir_all(&project).unwrap();
        git(&project, &["init"]);
        git(&project, &["config", "user.email", "test@example.com"]);
        git(&project, &["config", "user.name", "Test User"]);
        fs::write(project.join("README.md"), "# Project\n").unwrap();
        git(&project, &["add", "README.md"]);
        git(&project, &["commit", "-m", "initial"]);
        project
    }

    #[test]
    fn all_project_task_outcome_is_cancelled_before_any_substep() {
        let queue = TaskQueue::default();
        let task = queue.enqueue(TaskKind::SyncAllProjects, "cancel before", |context| {
            finish_all_project_task_outcome(0, 0, 0, true, Vec::new(), context)
        });

        let finished = queue.run_until_complete(&task.id).unwrap();

        assert_eq!(finished.status, TaskStatus::Cancelled);
        assert!(finished.summary.contains("Task cancelled"));
    }

    #[test]
    fn all_project_task_outcome_is_cancelled_after_a_substep() {
        let queue = TaskQueue::default();
        let task = queue.enqueue(TaskKind::SyncAllProjects, "cancel after", |context| {
            finish_all_project_task_outcome(
                1,
                0,
                0,
                true,
                vec![ProjectTaskRecord {
                    project_id: "project-one".to_string(),
                    status: TaskStatus::Succeeded,
                    summary: "fetch succeeded".to_string(),
                    error: None,
                }],
                context,
            )
        });

        let finished = queue.run_until_complete(&task.id).unwrap();

        assert_eq!(finished.status, TaskStatus::Cancelled);
        assert!(finished.summary.contains("ok=1"));
    }

    #[test]
    fn all_project_task_observes_cancellation_after_final_substep() {
        let workspace = temp_dir("cancel_final");
        init_project(&workspace, "project-one");

        let result = run_all_project_git_task(
            &workspace,
            &[],
            TaskKind::SyncAllProjects,
            "cancel after final substep",
            |project, context| {
                context.request_cancel_for_tests();
                project_outcome(
                    project,
                    TaskStatus::Succeeded,
                    "fetch succeeded",
                    Some(GitStatus::UpToDate),
                )
            },
        )
        .unwrap();

        assert_eq!(result.task.status, TaskStatus::Cancelled);
        assert!(result.task.summary.contains("ok=1"));
        assert!(result
            .task
            .stderr
            .contains("cancelled after current substep"));
    }
}

fn project_outcome(
    project: &Project,
    status: TaskStatus,
    summary: impl Into<String>,
    project_status: Option<GitStatus>,
) -> ProjectTaskOutcome {
    ProjectTaskOutcome {
        project_id: project.id.clone(),
        status,
        summary: format!("{}: {}", project.id, summary.into()),
        error: None,
        project_status,
    }
}

trait WithProjectError {
    fn with_error(self, error: String) -> Self;
}

impl WithProjectError for ProjectTaskOutcome {
    fn with_error(mut self, error: String) -> Self {
        self.error = Some(error);
        self
    }
}

struct NormalizedImportSource {
    remote_url: String,
    skill_path: Option<String>,
}

fn normalize_import_source(source: &str) -> Result<NormalizedImportSource, GitOperationError> {
    let source = source.trim();
    if source.is_empty() {
        return Err(GitOperationError::invalid_repository(
            "Repository source cannot be empty.",
        ));
    }

    if is_github_shorthand(source) {
        return Ok(NormalizedImportSource {
            remote_url: format!("https://github.com/{source}.git"),
            skill_path: None,
        });
    }

    if let Some(normalized) = parse_github_tree_source(source)? {
        return Ok(normalized);
    }

    Ok(NormalizedImportSource {
        remote_url: source.to_string(),
        skill_path: None,
    })
}

fn parse_github_tree_source(
    source: &str,
) -> Result<Option<NormalizedImportSource>, GitOperationError> {
    let Some(rest) = source
        .strip_prefix("https://github.com/")
        .or_else(|| source.strip_prefix("http://github.com/"))
    else {
        return Ok(None);
    };
    let path = rest
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_matches('/');
    let parts = path.split('/').collect::<Vec<_>>();
    if parts.len() < 5 || !matches!(parts[2], "tree" | "blob") {
        return Ok(None);
    }
    let owner = parts[0];
    let repo = parts[1].trim_end_matches(".git");
    if owner.is_empty() || repo.is_empty() {
        return Ok(None);
    }
    let skill_path = parts[4..].join("/");
    Ok(Some(NormalizedImportSource {
        remote_url: format!("https://github.com/{owner}/{repo}.git"),
        skill_path: Some(safe_import_skill_path(&skill_path)?),
    }))
}

fn looks_like_misplaced_skill_path(path: &str) -> bool {
    matches!(
        Path::new(path).components().next(),
        Some(Component::Normal(first)) if first == "skills"
    )
}

fn is_github_shorthand(source: &str) -> bool {
    if source.contains("://") || source.starts_with("git@") || source.contains('\\') {
        return false;
    }
    let parts = source.split('/').collect::<Vec<_>>();
    parts.len() == 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && *part != "." && *part != "..")
}

fn infer_directory_name(remote_url: &str) -> Result<String, GitOperationError> {
    let trimmed = remote_url.trim_end_matches('/');
    let name = trimmed
        .rsplit(['/', ':'])
        .next()
        .ok_or_else(|| GitOperationError::invalid_repository("Cannot infer directory name."))?
        .trim_end_matches(".git");
    safe_project_dir_name(name)
}

fn infer_import_directory_name(
    remote_url: &str,
    skill_path: Option<&str>,
) -> Result<String, GitOperationError> {
    let directory_name = infer_directory_name(remote_url)?;
    let Some(skill_path) = skill_path else {
        return Ok(directory_name);
    };
    let first_component = Path::new(skill_path)
        .components()
        .next()
        .and_then(|component| match component {
            Component::Normal(component) => component.to_str(),
            _ => None,
        });
    if first_component != Some(directory_name.as_str()) {
        return Ok(directory_name);
    }
    let Some((owner, repo)) = github_owner_repo_from_remote_url(remote_url) else {
        return Ok(directory_name);
    };
    safe_project_dir_name(&format!("{owner}-{repo}"))
}

fn github_owner_repo_from_remote_url(remote_url: &str) -> Option<(String, String)> {
    let trimmed = remote_url
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git");
    let path = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .or_else(|| trimmed.strip_prefix("ssh://git@github.com/"))
        .or_else(|| trimmed.strip_prefix("git@github.com:"))?;
    let mut parts = path.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

fn owner_qualified_directory_name(remote_url: &str) -> Option<String> {
    let (owner, repo) = github_owner_repo_from_remote_url(remote_url)?;
    safe_project_dir_name(&format!("{owner}-{repo}")).ok()
}

fn disambiguate_default_directory_name(
    workspace_root: &Path,
    directory_name: &str,
    remote_url: &str,
) -> Result<String, GitOperationError> {
    let target_path = workspace_root.join(directory_name);
    let Some(existing_remote) = existing_origin_remote_if_different(&target_path, remote_url)
    else {
        return Ok(directory_name.to_string());
    };

    let Some(disambiguated_name) = owner_qualified_directory_name(remote_url) else {
        return Err(GitOperationError::invalid_repository(format!(
            "Project directory '{}' already points at a different remote: {}. Choose a different directory name.",
            directory_name, existing_remote
        )));
    };
    if disambiguated_name == directory_name {
        return Err(GitOperationError::invalid_repository(format!(
            "Project directory '{}' already points at a different remote: {}. Choose a different directory name.",
            directory_name, existing_remote
        )));
    }

    let disambiguated_path = workspace_root.join(&disambiguated_name);
    let Some(disambiguated_remote) =
        existing_origin_remote_if_different(&disambiguated_path, remote_url)
    else {
        return Ok(disambiguated_name);
    };

    Err(GitOperationError::invalid_repository(format!(
        "Project directory '{}' already points at a different remote: {}. Choose a different directory name.",
        disambiguated_name, disambiguated_remote
    )))
}

pub(crate) fn safe_project_dir_name(name: &str) -> Result<String, GitOperationError> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name == ".git"
        || name.contains('\0')
        || name.contains('/')
        || name.contains('\\')
        || Path::new(name).components().count() != 1
    {
        return Err(GitOperationError::invalid_directory_name(name));
    }
    Ok(name.to_string())
}

pub(crate) fn safe_import_skill_path(path: &str) -> Result<String, GitOperationError> {
    let path = path.trim();
    let relative = Path::new(path);
    if path.is_empty()
        || path.contains('\0')
        || path.contains('\\')
        || path.split('/').any(|part| part.is_empty() || part == ".")
        || relative.is_absolute()
        || relative.components().count() == 0
        || relative.components().any(|component| {
            matches!(
                component,
                Component::CurDir
                    | Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
        || relative.components().any(|component| {
            component.as_os_str() == ".git" || component.as_os_str() == "node_modules"
        })
    {
        return Err(GitOperationError::invalid_skill_path(path));
    }
    Ok(relative.to_string_lossy().to_string())
}

fn is_git_repository(path: &Path) -> bool {
    path.join(".git").exists()
        && Command::new("git")
            .arg("-C")
            .arg(path)
            .arg("rev-parse")
            .arg("--git-dir")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
}

fn existing_origin_remote_if_different(project_path: &Path, remote_url: &str) -> Option<String> {
    let existing_remote = existing_origin_remote(project_path)?;
    if git_remote_identity(&existing_remote) == git_remote_identity(remote_url) {
        None
    } else {
        Some(existing_remote)
    }
}

fn existing_origin_remote(project_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let remote = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if remote.is_empty() {
        None
    } else {
        Some(remote)
    }
}

fn git_remote_identity(remote_url: &str) -> String {
    if let Some((owner, repo)) = github_owner_repo_from_remote_url(remote_url) {
        return format!(
            "github.com/{}/{}",
            owner.to_ascii_lowercase(),
            repo.to_ascii_lowercase()
        );
    }
    remote_url
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_ascii_lowercase()
}

pub(crate) fn clone_with_retries(
    path: &Path,
    target_path: &Path,
    args: &[String],
    max_attempts: usize,
    timeout: Duration,
    context: &mut crate::TaskContext,
) -> Result<(String, String), (String, String)> {
    let mut combined_stdout = String::new();
    let mut combined_stderr = String::new();

    for attempt in 1..=max_attempts {
        context.stdout(format!("clone attempt {attempt}/{max_attempts}"));
        match git_command_output_with_timeout(path, args, timeout) {
            Ok((stdout, stderr)) => {
                append_log(&mut combined_stdout, &stdout);
                append_log(&mut combined_stderr, &stderr);
                return Ok((combined_stdout, combined_stderr));
            }
            Err((stdout, stderr)) => {
                append_log(&mut combined_stdout, &stdout);
                append_log(&mut combined_stderr, &stderr);
                if let Err(error) = remove_partial_import_target(target_path) {
                    append_log(&mut combined_stderr, &error);
                    context.stderr(error);
                }
                if attempt < max_attempts {
                    context.stderr(format!("clone attempt {attempt} failed; retrying"));
                }
            }
        }
    }

    Err((combined_stdout, combined_stderr))
}

pub(crate) fn clone_import_project(
    workspace_root: &Path,
    target_path: &Path,
    remote_url: &str,
    directory_name: &str,
    skill_path: Option<&str>,
    branch: Option<&str>,
    shallow: bool,
    max_attempts: usize,
    timeout: Duration,
    context: &mut crate::TaskContext,
) -> Result<(String, String), (String, String)> {
    let skill_paths = skill_path
        .map(|skill_path| vec![skill_path.to_string()])
        .unwrap_or_default();
    clone_import_project_with_skill_paths(
        workspace_root,
        target_path,
        remote_url,
        directory_name,
        &skill_paths,
        branch,
        shallow,
        max_attempts,
        timeout,
        context,
    )
}

pub(crate) fn clone_import_project_with_skill_paths(
    workspace_root: &Path,
    target_path: &Path,
    remote_url: &str,
    directory_name: &str,
    skill_paths: &[String],
    branch: Option<&str>,
    shallow: bool,
    max_attempts: usize,
    timeout: Duration,
    context: &mut crate::TaskContext,
) -> Result<(String, String), (String, String)> {
    let staging_path = create_import_staging_path(workspace_root, directory_name)?;
    let mut args = vec!["clone".to_string()];
    if !skill_paths.is_empty() {
        args.push("--sparse".to_string());
    }
    if shallow {
        args.push("--depth".to_string());
        args.push("1".to_string());
    }
    if let Some(branch) = branch.filter(|branch| !branch.is_empty()) {
        args.push("--branch".to_string());
        args.push(branch.to_string());
    }
    args.push(remote_url.to_string());
    args.push(staging_path.display().to_string());
    context.stdout(format!("git {}", args.join(" ")));

    let (mut stdout, mut stderr) = clone_with_retries(
        workspace_root,
        &staging_path,
        &args,
        max_attempts,
        timeout,
        context,
    )?;

    if !skill_paths.is_empty() {
        let mut set_args = vec![
            "sparse-checkout".to_string(),
            "set".to_string(),
            "--cone".to_string(),
        ];
        set_args.extend(skill_paths.iter().cloned());
        context.stdout(format!(
            "git -C {} {}",
            staging_path.display(),
            set_args.join(" ")
        ));
        match git_command_output_with_timeout(&staging_path, &set_args, timeout) {
            Ok((set_stdout, set_stderr)) => {
                append_log(&mut stdout, &set_stdout);
                append_log(&mut stderr, &set_stderr);
            }
            Err((set_stdout, set_stderr)) => {
                append_log(&mut stdout, &set_stdout);
                append_log(&mut stderr, &set_stderr);
                if let Err(error) = remove_partial_import_target(&staging_path) {
                    append_log(&mut stderr, &error);
                    context.stderr(error);
                }
                return Err((stdout, stderr));
            }
        }
        for skill_path in skill_paths {
            if let Err(message) = ensure_sparse_skill_exists(&staging_path, skill_path) {
                append_log(&mut stderr, &message);
                if let Err(error) = remove_partial_import_target(&staging_path) {
                    append_log(&mut stderr, &error);
                    context.stderr(error);
                }
                return Err((stdout, stderr));
            }
        }
    }

    if target_path.exists() {
        let message = format!(
            "Target path '{}' appeared while import was running.",
            target_path.display()
        );
        append_log(&mut stderr, &message);
        if let Err(error) = remove_partial_import_target(&staging_path) {
            append_log(&mut stderr, &error);
            context.stderr(error);
        }
        return Err((stdout, stderr));
    }
    if let Err(error) = fs::rename(&staging_path, target_path) {
        let message = format!(
            "failed to move staged import '{}' to '{}': {error}",
            staging_path.display(),
            target_path.display()
        );
        append_log(&mut stderr, &message);
        if let Err(cleanup_error) = remove_partial_import_target(&staging_path) {
            append_log(&mut stderr, &cleanup_error);
            context.stderr(cleanup_error);
        }
        return Err((stdout, stderr));
    }
    append_log(
        &mut stdout,
        &format!("promoted staged import to {}", target_path.display()),
    );

    Ok((stdout, stderr))
}

fn create_import_staging_path(
    workspace_root: &Path,
    directory_name: &str,
) -> Result<PathBuf, (String, String)> {
    let imports_dir = workspace_root.join(".skilldock").join("imports");
    fs::create_dir_all(&imports_dir).map_err(|error| {
        (
            String::new(),
            format!(
                "failed to create import staging directory '{}': {error}",
                imports_dir.display()
            ),
        )
    })?;
    for attempt in 0..100 {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = imports_dir.join(format!(
            "{}-{}-{}",
            directory_name,
            std::process::id(),
            suffix + attempt
        ));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err((
        String::new(),
        "failed to allocate an import staging directory".to_string(),
    ))
}

fn ensure_sparse_skill_exists(project_path: &Path, skill_path: &str) -> Result<(), String> {
    let skill_md = project_path.join(skill_path).join("SKILL.md");
    if skill_md.is_file() {
        Ok(())
    } else {
        Err(format!(
            "Skill path '{}' does not contain SKILL.md after checkout.",
            skill_path
        ))
    }
}

fn add_skill_to_existing_sparse_checkout(
    project_path: &Path,
    skill_path: &str,
    timeout: Duration,
    context: &mut crate::TaskContext,
) -> Result<(String, String), (String, String)> {
    if !is_sparse_checkout(project_path) {
        return Err((
            String::new(),
            format!(
                "Skill path '{}' does not contain SKILL.md in the existing Git directory.",
                skill_path
            ),
        ));
    }

    let args = vec![
        "sparse-checkout".to_string(),
        "add".to_string(),
        skill_path.to_string(),
    ];
    context.stdout(format!(
        "git -C {} {}",
        project_path.display(),
        args.join(" ")
    ));
    let (mut stdout, mut stderr) = git_command_output_with_timeout(project_path, &args, timeout)?;
    if let Err(message) = ensure_sparse_skill_exists(project_path, skill_path) {
        append_log(&mut stderr, &message);
        return Err((stdout, stderr));
    }
    append_log(
        &mut stdout,
        &format!("added sparse skill path: {}", skill_path),
    );
    Ok((stdout, stderr))
}

fn is_sparse_checkout(project_path: &Path) -> bool {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(["config", "--bool", "core.sparseCheckout"])
        .output();
    output
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim() == "true")
        .unwrap_or(false)
}

pub(crate) fn git_command_output_with_timeout(
    path: &Path,
    args: &[String],
    timeout: Duration,
) -> Result<(String, String), (String, String)> {
    if !path.exists() {
        return Err((
            String::new(),
            format!("path does not exist: {}", path.display()),
        ));
    }

    let mut child = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| (String::new(), error.to_string()))?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|error| (String::new(), error.to_string()))?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                if output.status.success() {
                    return Ok((stdout, stderr));
                }
                return Err((stdout, stderr));
            }
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let output = child
                    .wait_with_output()
                    .map_err(|error| (String::new(), error.to_string()))?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
                append_log(
                    &mut stderr,
                    &format!("git command timed out after {} seconds", timeout.as_secs()),
                );
                return Err((stdout, stderr));
            }
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err((String::new(), error.to_string())),
        }
    }
}

fn git_command_output(path: &Path, args: &[String]) -> Result<(String, String), (String, String)> {
    if !path.exists() {
        return Err((
            String::new(),
            format!("path does not exist: {}", path.display()),
        ));
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .map_err(|error| (String::new(), error.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        Ok((stdout, stderr))
    } else {
        Err((stdout, stderr))
    }
}

fn remove_partial_import_target(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|error| {
            format!(
                "failed to remove partial clone '{}': {error}",
                path.display()
            )
        })
    } else {
        fs::remove_file(path).map_err(|error| {
            format!(
                "failed to remove partial clone '{}': {error}",
                path.display()
            )
        })
    }
}

fn append_log(target: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    target.push_str(text);
    if !target.ends_with('\n') {
        target.push('\n');
    }
}
