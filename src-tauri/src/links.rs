use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::{
    expand_home, is_writable_dir, load_user_config, run_workspace_task_blocking, scan_workspace_at,
    AgentProfile, BatchLinkExecuteRequest, BatchLinkOperationResult, BatchLinkPreview,
    BatchLinkPreviewRequest, BatchLinkSummary, BatchUnlinkExecuteRequest,
    BatchUnlinkOperationResult, BatchUnlinkPreview, BatchUnlinkPreviewRequest, BatchUnlinkSummary,
    ConfigError, ExecuteLinkSkillRequest, ExecuteUnlinkSkillRequest, LinkPreview,
    LinkPreviewStatus, LinkSkillRequest, Skill, TaskKind, TaskOperationResult, TaskOutcome,
    UnlinkPreview, UnlinkPreviewStatus, UnlinkSkillRequest, Workspace, WorkspaceError,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkOperationError {
    pub kind: LinkOperationErrorKind,
    pub path: Option<String>,
    pub message: String,
}

impl LinkOperationError {
    fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: LinkOperationErrorKind::Validation,
            path: None,
            message: message.into(),
        }
    }

    fn blocked(preview: &LinkPreview) -> Self {
        Self {
            kind: LinkOperationErrorKind::Blocked,
            path: Some(preview.target_path.clone()),
            message: format!(
                "Link preview status '{:?}' blocks execution.",
                preview.status
            ),
        }
    }

    fn unlink_blocked(preview: &UnlinkPreview) -> Self {
        Self {
            kind: LinkOperationErrorKind::Blocked,
            path: Some(preview.target_path.clone()),
            message: format!(
                "Unlink preview status '{:?}' blocks execution.",
                preview.status
            ),
        }
    }

    fn stale_preview() -> Self {
        Self {
            kind: LinkOperationErrorKind::StalePreview,
            path: None,
            message: "Link preview no longer matches the current filesystem state.".to_string(),
        }
    }

    fn workspace(error: WorkspaceError) -> Self {
        Self {
            kind: LinkOperationErrorKind::Workspace,
            path: Some(error.path),
            message: error.message,
        }
    }

    fn config(error: ConfigError) -> Self {
        Self {
            kind: LinkOperationErrorKind::Io,
            path: Some(error.path),
            message: error.message,
        }
    }

    fn io(path: &Path, error: std::io::Error) -> Self {
        Self {
            kind: LinkOperationErrorKind::Io,
            path: Some(path.display().to_string()),
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkOperationErrorKind {
    Validation,
    Blocked,
    StalePreview,
    Workspace,
    Io,
}

pub fn preview_link_skill_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: LinkSkillRequest,
) -> Result<LinkPreview, LinkOperationError> {
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())
        .map_err(LinkOperationError::workspace)?;
    let workspace = scan_workspace_at(&workspace_root, agent_profiles)
        .map_err(LinkOperationError::workspace)?;
    preview_link_with_workspace(&workspace_root, &workspace, request)
}

fn preview_link_with_workspace(
    workspace_root: &Path,
    workspace: &Workspace,
    request: LinkSkillRequest,
) -> Result<LinkPreview, LinkOperationError> {
    let profile_state = workspace
        .agent_profiles
        .iter()
        .find(|state| state.profile.id == request.agent_profile_id)
        .ok_or_else(|| LinkOperationError::validation("Agent profile was not found."))?;
    let skill = workspace
        .skills
        .iter()
        .find(|skill| skill.id == request.skill_id);
    let source_path = match skill {
        Some(skill) => canonical_workspace_skill_path(&workspace_root, skill)?,
        None => missing_source_path(&workspace_root, &request.skill_id)?,
    };
    let link_name = match (request.link_name.as_ref(), skill) {
        (Some(link_name), _) => safe_link_name(link_name)?,
        (None, Some(skill)) => safe_link_name(&skill.default_link_name)?,
        (None, None) => {
            let fallback = Path::new(&request.skill_id)
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| LinkOperationError::validation("Link name is required."))?;
            safe_link_name(fallback)?
        }
    };
    let target_path = PathBuf::from(&profile_state.skills_dir).join(&link_name);

    let (status, message) = if !profile_state.exists {
        (
            LinkPreviewStatus::AgentPathMissing,
            Some("Agent profile skills directory is missing.".to_string()),
        )
    } else if !profile_state.writable {
        (
            LinkPreviewStatus::AgentPathNotWritable,
            Some("Agent profile skills directory is not writable.".to_string()),
        )
    } else if skill.is_none() || !source_path.exists() {
        (
            LinkPreviewStatus::MissingSource,
            Some("Skill source directory is missing.".to_string()),
        )
    } else {
        classify_target(&target_path, &source_path)?
    };

    Ok(LinkPreview {
        skill_id: request.skill_id,
        agent_profile_id: request.agent_profile_id,
        link_name,
        source_path: source_path.display().to_string(),
        target_path: target_path.display().to_string(),
        status,
        message,
    })
}

pub fn link_skill_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: ExecuteLinkSkillRequest,
) -> Result<TaskOperationResult, LinkOperationError> {
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())
        .map_err(LinkOperationError::workspace)?;
    let profile = agent_profiles
        .iter()
        .find(|profile| profile.id == request.preview.agent_profile_id)
        .ok_or_else(|| LinkOperationError::validation("Agent profile was not found."))?;
    let current_preview = refresh_link_preview(&request.preview, profile)?;
    if current_preview != request.preview {
        return Err(LinkOperationError::stale_preview());
    }

    match request.preview.status {
        LinkPreviewStatus::WillLink | LinkPreviewStatus::AlreadyInstalled => {}
        _ => return Err(LinkOperationError::blocked(&request.preview)),
    }

    let preview = request.preview.clone();
    let profiles = agent_profiles.to_vec();
    let result_workspace = Arc::new(Mutex::new(None::<Workspace>));
    let result_workspace_for_task = Arc::clone(&result_workspace);
    let workspace_root_for_task = workspace_root.clone();

    let task = run_workspace_task_blocking(
        workspace_root.display().to_string(),
        TaskKind::LinkSkill,
        format!("Link {} to {}", preview.skill_id, preview.agent_profile_id),
        move |context| {
            let source_path = PathBuf::from(&preview.source_path);
            let target_path = PathBuf::from(&preview.target_path);
            let outcome = match preview.status {
                LinkPreviewStatus::AlreadyInstalled => {
                    context.stdout("already installed");
                    TaskOutcome::skipped("Skill already installed")
                }
                LinkPreviewStatus::WillLink => match create_symlink(&source_path, &target_path) {
                    Ok(()) => {
                        context.stdout(format!(
                            "created symlink: {} -> {}",
                            target_path.display(),
                            source_path.display()
                        ));
                        TaskOutcome::succeeded("Skill linked")
                    }
                    Err(error) => TaskOutcome::failed("Skill link failed", error),
                },
                _ => TaskOutcome::failed("Skill link blocked", "preview status blocks execution"),
            };

            if let Ok(workspace) = scan_workspace_at(&workspace_root_for_task, &profiles) {
                *result_workspace_for_task
                    .lock()
                    .expect("link result workspace lock poisoned") = Some(workspace);
            }

            outcome
        },
    );

    let workspace = result_workspace
        .lock()
        .expect("link result workspace lock poisoned")
        .clone()
        .unwrap_or_else(|| scan_or_empty_workspace(&workspace_root, agent_profiles));

    Ok(TaskOperationResult { task, workspace })
}

pub fn preview_link_skills_batch_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: BatchLinkPreviewRequest,
) -> Result<BatchLinkPreview, LinkOperationError> {
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())
        .map_err(LinkOperationError::workspace)?;
    let workspace = scan_workspace_at(&workspace_root, agent_profiles)
        .map_err(LinkOperationError::workspace)?;
    let mut previews = Vec::with_capacity(request.items.len());
    for item in request.items {
        previews.push(preview_link_with_workspace(&workspace_root, &workspace, item)?);
    }
    Ok(BatchLinkPreview { previews })
}

pub fn link_skills_batch_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: BatchLinkExecuteRequest,
) -> Result<BatchLinkOperationResult, LinkOperationError> {
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())
        .map_err(LinkOperationError::workspace)?;
    let previews = request.previews;
    let profiles = agent_profiles.to_vec();
    let result_workspace = Arc::new(Mutex::new(None::<Workspace>));
    let result_workspace_for_task = Arc::clone(&result_workspace);
    let result_summary = Arc::new(Mutex::new(BatchLinkSummary::default()));
    let result_summary_for_task = Arc::clone(&result_summary);
    let workspace_root_for_task = workspace_root.clone();
    let previews_for_task = previews.clone();

    let task = run_workspace_task_blocking(
        workspace_root.display().to_string(),
        TaskKind::LinkSkillsBatch,
        format!("Link {} skill targets", previews.len()),
        move |context| {
            let mut summary = BatchLinkSummary::default();

            let profiles_by_id: HashMap<&str, &AgentProfile> =
                profiles.iter().map(|profile| (profile.id.as_str(), profile)).collect();
            let mut profile_accessibility_cache: HashMap<String, AgentDirAccessibility> =
                HashMap::new();

            for preview in &previews_for_task {
                let Some(profile) = profiles_by_id.get(preview.agent_profile_id.as_str()) else {
                    summary.failed += 1;
                    context.stderr(format!(
                        "failed: {} -> {} (agent profile not found)",
                        preview.skill_id, preview.agent_profile_id
                    ));
                    continue;
                };

                let accessibility = profile_accessibility_cache
                    .entry(profile.id.clone())
                    .or_insert_with(|| probe_agent_dir_accessibility(profile))
                    .clone();

                let current_preview =
                    match refresh_link_preview_with_accessibility(preview, &accessibility) {
                        Ok(current_preview) => current_preview,
                        Err(error) => {
                            summary.failed += 1;
                            context.stderr(format!(
                                "failed: {} -> {} ({})",
                                preview.skill_id, preview.agent_profile_id, error.message
                            ));
                            continue;
                        }
                    };

                if current_preview != *preview {
                    summary.failed += 1;
                    context.stderr(format!(
                        "failed: {} -> {} (stale preview)",
                        preview.skill_id, preview.agent_profile_id
                    ));
                    continue;
                }

                match preview.status {
                    LinkPreviewStatus::AlreadyInstalled => {
                        summary.already_installed += 1;
                        context.stdout(format!(
                            "already installed: {} -> {} as {}",
                            preview.skill_id, preview.agent_profile_id, preview.link_name
                        ));
                    }
                    LinkPreviewStatus::WillLink => {
                        let source_path = PathBuf::from(&preview.source_path);
                        let target_path = PathBuf::from(&preview.target_path);
                        match create_symlink(&source_path, &target_path) {
                            Ok(()) => {
                                summary.linked += 1;
                                context.stdout(format!(
                                    "linked: {} -> {}",
                                    target_path.display(),
                                    source_path.display()
                                ));
                            }
                            Err(error) => {
                                summary.failed += 1;
                                context.stderr(format!(
                                    "failed: {} -> {} ({})",
                                    preview.skill_id, preview.agent_profile_id, error
                                ));
                            }
                        }
                    }
                    _ => {
                        summary.skipped += 1;
                        context.stdout(format!(
                            "skipped: {} -> {} ({:?})",
                            preview.skill_id, preview.agent_profile_id, preview.status
                        ));
                    }
                }
            }

            if let Ok(workspace) = scan_workspace_at(&workspace_root_for_task, &profiles) {
                *result_workspace_for_task
                    .lock()
                    .expect("batch link result workspace lock poisoned") = Some(workspace);
            }
            *result_summary_for_task
                .lock()
                .expect("batch link result summary lock poisoned") = summary.clone();

            let total =
                summary.linked + summary.already_installed + summary.skipped + summary.failed;
            let summary_line = format!(
                "Batch link complete: {} linked, {} already installed, {} skipped, {} failed",
                summary.linked, summary.already_installed, summary.skipped, summary.failed
            );

            if summary.failed > 0 {
                TaskOutcome::failed(summary_line, "One or more batch link items failed")
            } else if summary.linked == 0 && summary.already_installed == 0 && total > 0 {
                TaskOutcome::skipped(summary_line)
            } else {
                TaskOutcome::succeeded(summary_line)
            }
        },
    );

    let workspace = result_workspace
        .lock()
        .expect("batch link result workspace lock poisoned")
        .clone()
        .unwrap_or_else(|| scan_or_empty_workspace(&workspace_root, agent_profiles));
    let summary = result_summary
        .lock()
        .expect("batch link result summary lock poisoned")
        .clone();

    Ok(BatchLinkOperationResult {
        task,
        workspace,
        summary,
        previews,
    })
}

pub fn preview_unlink_skill_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: UnlinkSkillRequest,
) -> Result<UnlinkPreview, LinkOperationError> {
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())
        .map_err(LinkOperationError::workspace)?;
    let workspace = scan_workspace_at(&workspace_root, agent_profiles)
        .map_err(LinkOperationError::workspace)?;
    preview_unlink_with_workspace(&workspace_root, &workspace, request)
}

fn preview_unlink_with_workspace(
    workspace_root: &Path,
    workspace: &Workspace,
    request: UnlinkSkillRequest,
) -> Result<UnlinkPreview, LinkOperationError> {
    let profile_state = workspace
        .agent_profiles
        .iter()
        .find(|state| state.profile.id == request.agent_profile_id)
        .ok_or_else(|| LinkOperationError::validation("Agent profile was not found."))?;
    let link_name = safe_link_name(&request.link_name)?;
    let target_path = PathBuf::from(&profile_state.skills_dir).join(&link_name);

    let skill_paths = workspace_skill_paths(&workspace)?;
    let (status, source_path, message) = if !profile_state.exists {
        (
            UnlinkPreviewStatus::AgentPathMissing,
            None,
            Some("Agent profile skills directory is missing.".to_string()),
        )
    } else {
        classify_unlink_target(&workspace_root, &target_path, &skill_paths)?
    };

    Ok(UnlinkPreview {
        agent_profile_id: request.agent_profile_id,
        link_name,
        target_path: target_path.display().to_string(),
        source_path: source_path.map(|path| path.display().to_string()),
        status,
        message,
    })
}

pub fn unlink_skill_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: ExecuteUnlinkSkillRequest,
) -> Result<TaskOperationResult, LinkOperationError> {
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())
        .map_err(LinkOperationError::workspace)?;
    let profile = agent_profiles
        .iter()
        .find(|profile| profile.id == request.preview.agent_profile_id)
        .ok_or_else(|| LinkOperationError::validation("Agent profile was not found."))?;
    let current_preview =
        refresh_unlink_preview(&workspace_root, &request.preview, profile)?;
    if current_preview != request.preview {
        return Err(LinkOperationError::stale_preview());
    }
    if request.preview.status != UnlinkPreviewStatus::WillUnlink {
        return Err(LinkOperationError::unlink_blocked(&request.preview));
    }

    let preview = request.preview.clone();
    let profiles = agent_profiles.to_vec();
    let result_workspace = Arc::new(Mutex::new(None::<Workspace>));
    let result_workspace_for_task = Arc::clone(&result_workspace);
    let workspace_root_for_task = workspace_root.clone();

    let task = run_workspace_task_blocking(
        workspace_root.display().to_string(),
        TaskKind::UnlinkSkill,
        format!(
            "Unlink {} from {}",
            preview.link_name, preview.agent_profile_id
        ),
        move |context| {
            let target_path = PathBuf::from(&preview.target_path);
            let source_path = preview.source_path.as_ref().map(PathBuf::from);
            let outcome = match source_path.as_ref() {
                Some(source_path) => match remove_workspace_symlink(&target_path, source_path) {
                    Ok(()) => {
                        context.stdout(format!("unlinked: {}", target_path.display()));
                        TaskOutcome::succeeded("Skill unlinked")
                    }
                    Err(error) => TaskOutcome::failed("Skill unlink failed", error),
                },
                None => TaskOutcome::failed("Skill unlink failed", "source path missing"),
            };

            if let Ok(workspace) = scan_workspace_at(&workspace_root_for_task, &profiles) {
                *result_workspace_for_task
                    .lock()
                    .expect("unlink result workspace lock poisoned") = Some(workspace);
            }

            outcome
        },
    );

    let workspace = result_workspace
        .lock()
        .expect("unlink result workspace lock poisoned")
        .clone()
        .unwrap_or_else(|| scan_or_empty_workspace(&workspace_root, agent_profiles));

    Ok(TaskOperationResult { task, workspace })
}

pub fn preview_unlink_skills_batch_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: BatchUnlinkPreviewRequest,
) -> Result<BatchUnlinkPreview, LinkOperationError> {
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())
        .map_err(LinkOperationError::workspace)?;
    let workspace = scan_workspace_at(&workspace_root, agent_profiles)
        .map_err(LinkOperationError::workspace)?;
    let mut previews = Vec::with_capacity(request.items.len());
    for item in request.items {
        previews.push(preview_unlink_with_workspace(&workspace_root, &workspace, item)?);
    }
    Ok(BatchUnlinkPreview { previews })
}

pub fn unlink_skills_batch_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
    request: BatchUnlinkExecuteRequest,
) -> Result<BatchUnlinkOperationResult, LinkOperationError> {
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())
        .map_err(LinkOperationError::workspace)?;
    let previews = request.previews;
    let profiles = agent_profiles.to_vec();
    let result_workspace = Arc::new(Mutex::new(None::<Workspace>));
    let result_workspace_for_task = Arc::clone(&result_workspace);
    let result_summary = Arc::new(Mutex::new(BatchUnlinkSummary::default()));
    let result_summary_for_task = Arc::clone(&result_summary);
    let workspace_root_for_task = workspace_root.clone();
    let previews_for_task = previews.clone();

    let task = run_workspace_task_blocking(
        workspace_root.display().to_string(),
        TaskKind::UnlinkSkillsBatch,
        format!("Unlink {} skill targets", previews.len()),
        move |context| {
            let mut summary = BatchUnlinkSummary::default();

            let profiles_by_id: HashMap<&str, &AgentProfile> =
                profiles.iter().map(|profile| (profile.id.as_str(), profile)).collect();
            let mut profile_accessibility_cache: HashMap<String, AgentDirAccessibility> =
                HashMap::new();

            for preview in &previews_for_task {
                let Some(profile) = profiles_by_id.get(preview.agent_profile_id.as_str()) else {
                    summary.failed += 1;
                    context.stderr(format!(
                        "failed: {} / {} (agent profile not found)",
                        preview.agent_profile_id, preview.link_name
                    ));
                    continue;
                };

                let accessibility = profile_accessibility_cache
                    .entry(profile.id.clone())
                    .or_insert_with(|| probe_agent_dir_accessibility(profile))
                    .clone();

                let current_preview = match refresh_unlink_preview_with_accessibility(
                    &workspace_root_for_task,
                    preview,
                    &accessibility,
                ) {
                    Ok(current_preview) => current_preview,
                    Err(error) => {
                        summary.failed += 1;
                        context.stderr(format!(
                            "failed: {} / {} ({})",
                            preview.agent_profile_id, preview.link_name, error.message
                        ));
                        continue;
                    }
                };

                if current_preview != *preview {
                    summary.failed += 1;
                    context.stderr(format!(
                        "failed: {} / {} (stale preview)",
                        preview.agent_profile_id, preview.link_name
                    ));
                    continue;
                }

                match preview.status {
                    UnlinkPreviewStatus::WillUnlink => {
                        let Some(source_path) = preview.source_path.as_ref().map(PathBuf::from)
                        else {
                            summary.failed += 1;
                            context.stderr(format!(
                                "failed: {} / {} (source path missing)",
                                preview.agent_profile_id, preview.link_name
                            ));
                            continue;
                        };
                        let target_path = PathBuf::from(&preview.target_path);
                        match remove_workspace_symlink(&target_path, &source_path) {
                            Ok(()) => {
                                summary.unlinked += 1;
                                context.stdout(format!("unlinked: {}", target_path.display()));
                            }
                            Err(error) => {
                                summary.failed += 1;
                                context.stderr(format!(
                                    "failed: {} / {} ({})",
                                    preview.agent_profile_id, preview.link_name, error
                                ));
                            }
                        }
                    }
                    _ => {
                        summary.skipped += 1;
                        context.stdout(format!(
                            "skipped: {} / {} ({:?})",
                            preview.agent_profile_id, preview.link_name, preview.status
                        ));
                    }
                }
            }

            if let Ok(workspace) = scan_workspace_at(&workspace_root_for_task, &profiles) {
                *result_workspace_for_task
                    .lock()
                    .expect("batch unlink result workspace lock poisoned") = Some(workspace);
            }
            *result_summary_for_task
                .lock()
                .expect("batch unlink result summary lock poisoned") = summary.clone();

            let total = summary.unlinked + summary.skipped + summary.failed;
            let summary_line = format!(
                "Batch unlink complete: {} unlinked, {} skipped, {} failed",
                summary.unlinked, summary.skipped, summary.failed
            );

            if summary.failed > 0 {
                TaskOutcome::failed(summary_line, "One or more batch unlink items failed")
            } else if summary.unlinked == 0 && total > 0 {
                TaskOutcome::skipped(summary_line)
            } else {
                TaskOutcome::succeeded(summary_line)
            }
        },
    );

    let workspace = result_workspace
        .lock()
        .expect("batch unlink result workspace lock poisoned")
        .clone()
        .unwrap_or_else(|| scan_or_empty_workspace(&workspace_root, agent_profiles));
    let summary = result_summary
        .lock()
        .expect("batch unlink result summary lock poisoned")
        .clone();

    Ok(BatchUnlinkOperationResult {
        task,
        workspace,
        summary,
        previews,
    })
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn preview_link_skill_command(
    workspace_root: String,
    request: LinkSkillRequest,
) -> Result<LinkPreview, LinkOperationError> {
    let user_config = load_user_config().map_err(LinkOperationError::config)?;
    preview_link_skill_at(workspace_root, &user_config.agent_profiles, request)
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn link_skill_command(
    workspace_root: String,
    request: ExecuteLinkSkillRequest,
) -> Result<TaskOperationResult, LinkOperationError> {
    let user_config = load_user_config().map_err(LinkOperationError::config)?;
    link_skill_at(workspace_root, &user_config.agent_profiles, request)
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn preview_link_skills_batch_command(
    workspace_root: String,
    request: BatchLinkPreviewRequest,
) -> Result<BatchLinkPreview, LinkOperationError> {
    let user_config = load_user_config().map_err(LinkOperationError::config)?;
    preview_link_skills_batch_at(workspace_root, &user_config.agent_profiles, request)
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn link_skills_batch_command(
    workspace_root: String,
    request: BatchLinkExecuteRequest,
) -> Result<BatchLinkOperationResult, LinkOperationError> {
    let user_config = load_user_config().map_err(LinkOperationError::config)?;
    link_skills_batch_at(workspace_root, &user_config.agent_profiles, request)
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn preview_unlink_skill_command(
    workspace_root: String,
    request: UnlinkSkillRequest,
) -> Result<UnlinkPreview, LinkOperationError> {
    let user_config = load_user_config().map_err(LinkOperationError::config)?;
    preview_unlink_skill_at(workspace_root, &user_config.agent_profiles, request)
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn unlink_skill_command(
    workspace_root: String,
    request: ExecuteUnlinkSkillRequest,
) -> Result<TaskOperationResult, LinkOperationError> {
    let user_config = load_user_config().map_err(LinkOperationError::config)?;
    unlink_skill_at(workspace_root, &user_config.agent_profiles, request)
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn preview_unlink_skills_batch_command(
    workspace_root: String,
    request: BatchUnlinkPreviewRequest,
) -> Result<BatchUnlinkPreview, LinkOperationError> {
    let user_config = load_user_config().map_err(LinkOperationError::config)?;
    preview_unlink_skills_batch_at(workspace_root, &user_config.agent_profiles, request)
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn unlink_skills_batch_command(
    workspace_root: String,
    request: BatchUnlinkExecuteRequest,
) -> Result<BatchUnlinkOperationResult, LinkOperationError> {
    let user_config = load_user_config().map_err(LinkOperationError::config)?;
    unlink_skills_batch_at(workspace_root, &user_config.agent_profiles, request)
}

/// Cached accessibility info for an agent profile's skills directory.
/// Used to avoid repeating the (relatively cheap but non-trivial) write probe
/// for every preview in a batch operation.
#[derive(Debug, Clone)]
struct AgentDirAccessibility {
    exists: bool,
    writable: bool,
}

fn probe_agent_dir_accessibility(profile: &AgentProfile) -> AgentDirAccessibility {
    let skills_dir = expand_home(&profile.skills_dir);
    let exists = skills_dir.is_dir();
    let writable = exists && is_writable_dir(&skills_dir);
    AgentDirAccessibility { exists, writable }
}

/// Scan the workspace and return the result, or fall back to an empty
/// `Workspace` anchored at `workspace_root` on failure. Used after link/unlink
/// operations to return a post-operation workspace snapshot without making
/// scan failure a fatal error on an otherwise-successful task.
fn scan_or_empty_workspace(workspace_root: &Path, agent_profiles: &[AgentProfile]) -> Workspace {
    scan_workspace_at(workspace_root, agent_profiles).unwrap_or_else(|_| Workspace {
        root: workspace_root.display().to_string(),
        projects: Vec::new(),
        skills: Vec::new(),
        agent_profiles: Vec::new(),
    })
}

/// Re-derive the current status of a link preview by inspecting the filesystem
/// paths already recorded in the preview. This is a direct replacement for
/// `preview_link_with_workspace` when the caller already has a previously
/// computed preview; it avoids a full workspace rescan.
///
/// The returned preview preserves `skill_id`, `agent_profile_id`, `link_name`,
/// `source_path`, and `target_path` from the input preview, so field-by-field
/// comparison against the caller-supplied preview still detects staleness
/// exactly as the original workspace-scan based flow did (including message
/// differences for conflict cases).
fn refresh_link_preview(
    preview: &LinkPreview,
    profile: &AgentProfile,
) -> Result<LinkPreview, LinkOperationError> {
    let accessibility = probe_agent_dir_accessibility(profile);
    refresh_link_preview_with_accessibility(preview, &accessibility)
}

fn refresh_link_preview_with_accessibility(
    preview: &LinkPreview,
    accessibility: &AgentDirAccessibility,
) -> Result<LinkPreview, LinkOperationError> {
    let source_path = PathBuf::from(&preview.source_path);
    let target_path = PathBuf::from(&preview.target_path);

    let (status, message) = if !accessibility.exists {
        (
            LinkPreviewStatus::AgentPathMissing,
            Some("Agent profile skills directory is missing.".to_string()),
        )
    } else if !accessibility.writable {
        (
            LinkPreviewStatus::AgentPathNotWritable,
            Some("Agent profile skills directory is not writable.".to_string()),
        )
    } else if !source_path.exists() {
        (
            LinkPreviewStatus::MissingSource,
            Some("Skill source directory is missing.".to_string()),
        )
    } else {
        classify_target(&target_path, &source_path)?
    };

    Ok(LinkPreview {
        status,
        message,
        ..preview.clone()
    })
}

/// Re-derive the current status of an unlink preview without a full workspace
/// scan. Unlike link previews, unlink classification needs to know whether
/// the symlink target is a known workspace skill. Instead of rescanning every
/// skill, we verify the target still resolves to the `source_path` recorded
/// in the original preview — this is sufficient to detect the stale cases that
/// matter (target changed, symlink removed, file/dir replaced a symlink, etc.).
fn refresh_unlink_preview(
    workspace_root: &Path,
    preview: &UnlinkPreview,
    profile: &AgentProfile,
) -> Result<UnlinkPreview, LinkOperationError> {
    let accessibility = probe_agent_dir_accessibility(profile);
    refresh_unlink_preview_with_accessibility(workspace_root, preview, &accessibility)
}

fn refresh_unlink_preview_with_accessibility(
    workspace_root: &Path,
    preview: &UnlinkPreview,
    accessibility: &AgentDirAccessibility,
) -> Result<UnlinkPreview, LinkOperationError> {
    let target_path = PathBuf::from(&preview.target_path);

    if !accessibility.exists {
        return Ok(UnlinkPreview {
            source_path: None,
            status: UnlinkPreviewStatus::AgentPathMissing,
            message: Some("Agent profile skills directory is missing.".to_string()),
            ..preview.clone()
        });
    }

    // Rebuild a single-entry skill_paths set from the recorded preview source,
    // so classify_unlink_target can report `WillUnlink` when the symlink still
    // matches that path. We only seed this for previews that were originally
    // `WillUnlink`; for `ExternalSymlink` or `NotWorkspaceSkill` the source
    // path is outside the workspace skill set, and seeding it would
    // incorrectly reclassify the current target as a workspace skill.
    let mut skill_paths = HashSet::new();
    if preview.status == UnlinkPreviewStatus::WillUnlink {
        if let Some(original_source) = preview.source_path.as_deref() {
            skill_paths.insert(PathBuf::from(original_source));
        }
    }

    let (status, source_path, message) =
        classify_unlink_target(workspace_root, &target_path, &skill_paths)?;

    Ok(UnlinkPreview {
        source_path: source_path.map(|path| path.display().to_string()),
        status,
        message,
        ..preview.clone()
    })
}

fn canonical_workspace_skill_path(
    workspace_root: &Path,
    skill: &Skill,
) -> Result<PathBuf, LinkOperationError> {
    let source_path = fs::canonicalize(&skill.absolute_path)
        .map_err(|error| LinkOperationError::io(Path::new(&skill.absolute_path), error))?;
    if !source_path.starts_with(workspace_root) {
        return Err(LinkOperationError::validation(
            "Skill source is outside the selected workspace.",
        ));
    }
    Ok(source_path)
}

fn missing_source_path(
    workspace_root: &Path,
    skill_id: &str,
) -> Result<PathBuf, LinkOperationError> {
    let relative = Path::new(skill_id);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(LinkOperationError::validation(
            "Skill id is not a safe relative workspace path.",
        ));
    }
    Ok(workspace_root.join(relative))
}

fn safe_link_name(name: &str) -> Result<String, LinkOperationError> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || Path::new(name).components().count() != 1
    {
        return Err(LinkOperationError::validation(format!(
            "Link name '{name}' is not a safe single path segment."
        )));
    }
    Ok(name.to_string())
}

fn classify_target(
    target_path: &Path,
    source_path: &Path,
) -> Result<(LinkPreviewStatus, Option<String>), LinkOperationError> {
    let Ok(metadata) = fs::symlink_metadata(target_path) else {
        return Ok((LinkPreviewStatus::WillLink, None));
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        let raw_target = fs::read_link(target_path)
            .map_err(|error| LinkOperationError::io(target_path, error))?;
        let resolved_target = if raw_target.is_absolute() {
            raw_target
        } else {
            target_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(raw_target)
        };
        let Ok(canonical_target) = fs::canonicalize(&resolved_target) else {
            return Ok((
                LinkPreviewStatus::NameConflict,
                Some("Existing symlink target cannot be resolved.".to_string()),
            ));
        };
        if canonical_target == source_path {
            return Ok((LinkPreviewStatus::AlreadyInstalled, None));
        }
        return Ok((
            LinkPreviewStatus::NameConflict,
            Some(format!(
                "Existing symlink points to '{}'. Choose another link name to install safely.",
                canonical_target.display()
            )),
        ));
    }

    if file_type.is_file() {
        return Ok((
            LinkPreviewStatus::BlockedByRealFile,
            Some("Target path is an ordinary file.".to_string()),
        ));
    }
    if file_type.is_dir() {
        return Ok((
            LinkPreviewStatus::BlockedByRealDirectory,
            Some("Target path is an ordinary directory.".to_string()),
        ));
    }
    Ok((
        LinkPreviewStatus::BlockedByRealFile,
        Some("Target path exists and is not a symlink.".to_string()),
    ))
}

fn workspace_skill_paths(workspace: &Workspace) -> Result<HashSet<PathBuf>, LinkOperationError> {
    let mut paths = HashSet::new();
    for skill in &workspace.skills {
        let path = fs::canonicalize(&skill.absolute_path)
            .map_err(|error| LinkOperationError::io(Path::new(&skill.absolute_path), error))?;
        paths.insert(path);
    }
    Ok(paths)
}

fn classify_unlink_target(
    workspace_root: &Path,
    target_path: &Path,
    skill_paths: &HashSet<PathBuf>,
) -> Result<(UnlinkPreviewStatus, Option<PathBuf>, Option<String>), LinkOperationError> {
    let Ok(metadata) = fs::symlink_metadata(target_path) else {
        return Ok((
            UnlinkPreviewStatus::NotFound,
            None,
            Some("Agent skill link was not found.".to_string()),
        ));
    };

    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        let raw_target = fs::read_link(target_path)
            .map_err(|error| LinkOperationError::io(target_path, error))?;
        let resolved_target = if raw_target.is_absolute() {
            raw_target
        } else {
            target_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(raw_target)
        };
        let Ok(canonical_target) = fs::canonicalize(&resolved_target) else {
            return Ok((
                UnlinkPreviewStatus::BrokenSymlink,
                None,
                Some("Existing symlink target cannot be resolved.".to_string()),
            ));
        };

        if skill_paths.contains(&canonical_target) {
            return Ok((
                UnlinkPreviewStatus::WillUnlink,
                Some(canonical_target),
                None,
            ));
        }

        if canonical_target.starts_with(workspace_root) {
            return Ok((
                UnlinkPreviewStatus::NotWorkspaceSkill,
                Some(canonical_target),
                Some(
                    "Symlink points inside the workspace but not to a discovered skill."
                        .to_string(),
                ),
            ));
        }

        return Ok((
            UnlinkPreviewStatus::ExternalSymlink,
            Some(canonical_target),
            Some("Symlink points outside the selected workspace.".to_string()),
        ));
    }

    if file_type.is_dir() {
        return Ok((
            UnlinkPreviewStatus::BlockedByRealDirectory,
            None,
            Some("Target path is an ordinary directory.".to_string()),
        ));
    }

    Ok((
        UnlinkPreviewStatus::BlockedByRealFile,
        None,
        Some("Target path is not a symlink.".to_string()),
    ))
}

fn remove_workspace_symlink(target_path: &Path, expected_source_path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(target_path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_symlink() {
        return Err("target path is no longer a symlink".to_string());
    }
    let actual_source_path = canonical_symlink_target(target_path)?;
    if actual_source_path != expected_source_path {
        return Err(format!(
            "symlink target changed from '{}' to '{}'",
            expected_source_path.display(),
            actual_source_path.display()
        ));
    }
    fs::remove_file(target_path).map_err(|error| error.to_string())
}

fn canonical_symlink_target(target_path: &Path) -> Result<PathBuf, String> {
    let raw_target = fs::read_link(target_path).map_err(|error| error.to_string())?;
    let resolved_target = if raw_target.is_absolute() {
        raw_target
    } else {
        target_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(raw_target)
    };
    fs::canonicalize(resolved_target).map_err(|error| error.to_string())
}

#[cfg(unix)]
fn create_symlink(source_path: &Path, target_path: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(source_path, target_path).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn create_symlink(source_path: &Path, target_path: &Path) -> Result<(), String> {
    std::os::windows::fs::symlink_dir(source_path, target_path).map_err(|error| error.to_string())
}
