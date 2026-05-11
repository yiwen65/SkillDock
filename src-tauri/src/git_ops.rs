use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::{
    is_pull_all_eligible, load_user_config, run_workspace_task_background,
    run_workspace_task_blocking, scan_workspace_at, AgentProfile, ConfigError, GitStatus,
    ImportProjectRequest, Project, ProjectTaskRecord, PullAllProjectsRequest, PullProjectRequest,
    TaskKind, TaskOperationResult, TaskOutcome, TaskStatus, Workspace, WorkspaceError,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProjectPlan {
    pub remote_url: String,
    pub directory_name: String,
    pub target_path: String,
    pub will_clone: bool,
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

    fn io(path: &Path, message: impl Into<String>) -> Self {
        Self {
            kind: GitOperationErrorKind::Io,
            path: Some(path.display().to_string()),
            message: message.into(),
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
    let remote_url = normalize_import_source(&request.source)?;
    let directory_name = match request.directory_name.as_ref() {
        Some(directory_name) => safe_project_dir_name(directory_name)?,
        None => infer_directory_name(&remote_url)?,
    };
    let target_path = workspace_root.join(&directory_name);
    let will_clone = !target_path.exists();

    Ok(ImportProjectPlan {
        remote_url,
        directory_name,
        target_path: target_path.display().to_string(),
        will_clone,
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

    let task = run_workspace_task_blocking(
        workspace_root.display().to_string(),
        TaskKind::ImportProject,
        format!("Import {}", plan.directory_name),
        move |context| {
            let outcome = if target_path.exists() {
                if is_git_repository(&target_path) {
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
                let mut args = vec!["clone".to_string()];
                if request.shallow {
                    args.push("--depth".to_string());
                    args.push("1".to_string());
                }
                args.push(plan.remote_url.clone());
                args.push(plan.directory_name.clone());
                context.stdout(format!("git {}", args.join(" ")));
                match git_command_output(&workspace_root_for_task, &args) {
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

            if matches!(outcome.status(), crate::TaskStatus::Succeeded) {
                if let Ok(workspace) = scan_workspace_at(&workspace_root_for_task, &profiles) {
                    *result_workspace_for_task
                        .lock()
                        .expect("import result workspace lock poisoned") = Some(workspace);
                }
            }

            outcome
        },
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

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn import_project_command(
    workspace_root: String,
    request: ImportProjectRequest,
) -> Result<TaskOperationResult, GitOperationError> {
    let user_config = load_user_config().map_err(GitOperationError::config)?;
    import_project_at(workspace_root, &user_config.agent_profiles, request)
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
    let autostash = request.autostash;
    run_project_git_task_with_mode(
        workspace_root,
        agent_profiles,
        TaskKind::PullProject,
        format!("Pull {}", request.project_id),
        request.project_id,
        ProjectTaskMode::Blocking,
        move |project, context| pull_project(project, autostash, context),
    )
}

pub fn pull_project_background_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: PullProjectRequest,
) -> Result<TaskOperationResult, GitOperationError> {
    let autostash = request.autostash;
    run_project_git_task_with_mode(
        workspace_root,
        agent_profiles,
        TaskKind::PullProject,
        format!("Pull {}", request.project_id),
        request.project_id,
        ProjectTaskMode::Background,
        move |project, context| pull_project(project, autostash, context),
    )
}

pub fn pull_all_projects_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: PullAllProjectsRequest,
) -> Result<TaskOperationResult, GitOperationError> {
    let autostash = request.autostash;
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
            pull_safe_project(project, autostash, context)
        },
    )
}

pub fn pull_all_projects_background_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: PullAllProjectsRequest,
) -> Result<TaskOperationResult, GitOperationError> {
    let autostash = request.autostash;
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
            pull_safe_project(project, autostash, context)
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

fn pull_project(
    project: &Project,
    autostash: bool,
    context: &mut crate::TaskContext,
) -> ProjectTaskOutcome {
    if matches!(project.git_status, GitStatus::Dirty) && !autostash {
        context.stdout(format!(
            "skip: {} has local changes; use autostash or clean/stash first",
            project.id
        ));
        return project_outcome(project, TaskStatus::Skipped, "dirty working tree", None);
    }
    if matches!(project.git_status, GitStatus::Detached) {
        context.stdout(format!("skip: {} is detached", project.id));
        return project_outcome(project, TaskStatus::Skipped, "detached", None);
    }
    if project.upstream.is_none() {
        context.stdout(format!("skip: {} has no upstream", project.id));
        return project_outcome(project, TaskStatus::Skipped, "no upstream", None);
    }

    let mut args = vec![
        "pull".to_string(),
        "--ff-only".to_string(),
        "--prune".to_string(),
    ];
    if autostash {
        args.push("--autostash".to_string());
    }
    context.stdout(format!("git -C {} {}", project.path, args.join(" ")));
    match git_command_output(Path::new(&project.path), &args) {
        Ok((stdout, stderr)) => {
            context.stdout(stdout);
            context.stderr(stderr);
            project_outcome(project, TaskStatus::Succeeded, "pull succeeded", None)
        }
        Err((stdout, stderr)) => {
            context.stdout(stdout);
            context.stderr(&stderr);
            project_outcome(project, TaskStatus::Failed, "pull failed", None).with_error(stderr)
        }
    }
}

fn pull_safe_project(
    project: &Project,
    autostash: bool,
    context: &mut crate::TaskContext,
) -> ProjectTaskOutcome {
    if !project.pull_all_eligible {
        context.stdout(format!(
            "skip: {} is {} and is not safe for pull-all",
            project.id,
            git_status_label(&project.git_status)
        ));
        return project_outcome(project, TaskStatus::Skipped, "not safe for pull-all", None);
    }

    pull_project(project, autostash, context)
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

fn normalize_import_source(source: &str) -> Result<String, GitOperationError> {
    let source = source.trim();
    if source.is_empty() {
        return Err(GitOperationError::invalid_repository(
            "Repository source cannot be empty.",
        ));
    }

    if is_github_shorthand(source) {
        return Ok(format!("https://github.com/{source}.git"));
    }

    Ok(source.to_string())
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

fn safe_project_dir_name(name: &str) -> Result<String, GitOperationError> {
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

#[allow(dead_code)]
fn remove_empty_created_dir(path: &Path) -> Result<(), GitOperationError> {
    if path.exists()
        && fs::read_dir(path)
            .map_err(|error| GitOperationError::io(path, error.to_string()))?
            .next()
            .is_none()
    {
        fs::remove_dir(path).map_err(|error| GitOperationError::io(path, error.to_string()))?;
    }
    Ok(())
}
