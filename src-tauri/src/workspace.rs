use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    load_user_config_at, load_workspace_config, save_user_config_at, save_workspace_config,
    ConfigError, SkillMarkdownPreview, UserConfig, Workspace, WorkspaceConfig,
};

pub const SKILL_MARKDOWN_PREVIEW_MAX_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceError {
    pub kind: WorkspaceErrorKind,
    pub path: String,
    pub message: String,
}

impl WorkspaceError {
    fn path_missing(path: &Path) -> Self {
        Self {
            kind: WorkspaceErrorKind::PathMissing,
            path: path.display().to_string(),
            message: "Workspace path does not exist.".to_string(),
        }
    }

    fn not_directory(path: &Path) -> Self {
        Self {
            kind: WorkspaceErrorKind::NotDirectory,
            path: path.display().to_string(),
            message: "Workspace path is not a directory.".to_string(),
        }
    }

    pub(crate) fn io(path: &Path, message: impl Into<String>) -> Self {
        Self {
            kind: WorkspaceErrorKind::Io,
            path: path.display().to_string(),
            message: message.into(),
        }
    }

    pub(crate) fn config(error: ConfigError) -> Self {
        Self {
            kind: WorkspaceErrorKind::Config,
            path: error.path,
            message: error.message,
        }
    }

    pub(crate) fn outside_workspace(path: &Path) -> Self {
        Self {
            kind: WorkspaceErrorKind::OutsideWorkspace,
            path: path.display().to_string(),
            message: "Path is outside the selected workspace.".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceErrorKind {
    PathMissing,
    NotDirectory,
    Io,
    Config,
    OutsideWorkspace,
}

pub fn select_workspace_at(
    workspace_root: impl AsRef<Path>,
    user_config_path: impl AsRef<Path>,
) -> Result<Workspace, WorkspaceError> {
    let canonical_root = validate_workspace_root(workspace_root.as_ref())?;
    ensure_workspace_config(&canonical_root)?;

    let mut user_config = load_user_config_at(user_config_path.as_ref())
        .map_err(WorkspaceError::config)?;
    remember_recent_workspace(&mut user_config, &canonical_root);
    save_user_config_at(user_config_path.as_ref(), &user_config)
        .map_err(WorkspaceError::config)?;

    crate::scan_workspace_at(&canonical_root, &user_config.agent_profiles)
}

pub fn restore_recent_workspace_at(
    user_config_path: impl AsRef<Path>,
) -> Result<Option<Workspace>, WorkspaceError> {
    let user_config = load_user_config_at(user_config_path.as_ref())
        .map_err(WorkspaceError::config)?;

    for workspace_root in user_config.recent_workspaces {
        let path = PathBuf::from(&workspace_root);
        if path.is_dir() {
            return select_workspace_at(path, user_config_path).map(Some);
        }
    }

    Ok(None)
}

pub fn read_skill_markdown_preview_at(
    workspace_root: impl AsRef<Path>,
    skill_id: &str,
    max_bytes: usize,
) -> Result<SkillMarkdownPreview, WorkspaceError> {
    let workspace_root = validate_workspace_root(workspace_root.as_ref())?;
    let skill_relative = safe_relative_path(skill_id)?;
    let skill_dir = workspace_root.join(&skill_relative);
    let canonical_skill_dir = fs::canonicalize(&skill_dir)
        .map_err(|error| WorkspaceError::io(&skill_dir, error.to_string()))?;
    if !canonical_skill_dir.starts_with(&workspace_root) {
        return Err(WorkspaceError::outside_workspace(&canonical_skill_dir));
    }

    let skill_md = canonical_skill_dir.join("SKILL.md");
    let mut file = fs::File::open(&skill_md)
        .map_err(|error| WorkspaceError::io(&skill_md, error.to_string()))?;
    let effective_max_bytes = max_bytes.min(SKILL_MARKDOWN_PREVIEW_MAX_BYTES);
    let mut buffer = vec![0; effective_max_bytes + 1];
    let read = file
        .read(&mut buffer)
        .map_err(|error| WorkspaceError::io(&skill_md, error.to_string()))?;
    let truncated = read > effective_max_bytes;
    buffer.truncate(read.min(effective_max_bytes));

    Ok(SkillMarkdownPreview {
        skill_id: skill_id.to_string(),
        markdown: String::from_utf8_lossy(&buffer).to_string(),
        truncated,
    })
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn read_skill_markdown_preview_command(
    workspace_root: String,
    skill_id: String,
    max_bytes: usize,
) -> Result<SkillMarkdownPreview, WorkspaceError> {
    read_skill_markdown_preview_at(workspace_root, &skill_id, max_bytes)
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn open_workspace_path_command(
    workspace_root: String,
    path: String,
) -> Result<(), WorkspaceError> {
    let canonical_path = resolve_workspace_path_at(workspace_root, path)?;
    open_path(&canonical_path)
}

pub fn resolve_workspace_path_at(
    workspace_root: impl AsRef<Path>,
    path: impl AsRef<Path>,
) -> Result<PathBuf, WorkspaceError> {
    let workspace_root = validate_workspace_root(workspace_root.as_ref())?;
    let requested_path = path.as_ref();
    let requested_path = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        workspace_root.join(requested_path)
    };
    let canonical_path = fs::canonicalize(&requested_path)
        .map_err(|error| WorkspaceError::io(&requested_path, error.to_string()))?;
    if !canonical_path.starts_with(&workspace_root) {
        return Err(WorkspaceError::outside_workspace(&canonical_path));
    }
    Ok(canonical_path)
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn select_workspace_command(workspace_root: String) -> Result<Workspace, WorkspaceError> {
    select_workspace_at(workspace_root, crate::default_user_config_path())
}

#[cfg_attr(feature = "desktop", tauri::command(async))]
pub fn restore_recent_workspace_command() -> Result<Option<Workspace>, WorkspaceError> {
    restore_recent_workspace_at(crate::default_user_config_path())
}

pub(crate) fn validate_workspace_root(path: &Path) -> Result<PathBuf, WorkspaceError> {
    if !path.exists() {
        return Err(WorkspaceError::path_missing(path));
    }

    if !path.is_dir() {
        return Err(WorkspaceError::not_directory(path));
    }

    fs::canonicalize(path).map_err(|error| WorkspaceError::io(path, error.to_string()))
}

fn safe_relative_path(path: &str) -> Result<PathBuf, WorkspaceError> {
    let relative = Path::new(path);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(WorkspaceError::outside_workspace(relative));
    }
    Ok(relative.to_path_buf())
}

fn open_path(path: &Path) -> Result<(), WorkspaceError> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(path);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", "", &path.display().to_string()]);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = if is_wsl() {
        match wsl_windows_path(path) {
            Ok(windows_path) => {
                let mut command = std::process::Command::new("cmd.exe");
                command.args(["/C", "start", "", &windows_path]);
                command
            }
            Err(_) => {
                let mut command = std::process::Command::new("xdg-open");
                command.arg(path);
                command
            }
        }
    } else {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| WorkspaceError::io(path, error.to_string()))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn is_wsl() -> bool {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|value| {
            let lower = value.to_ascii_lowercase();
            lower.contains("microsoft") || lower.contains("wsl")
        })
        .unwrap_or(false)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn wsl_windows_path(path: &Path) -> Result<String, WorkspaceError> {
    let output = std::process::Command::new("wslpath")
        .arg("-w")
        .arg(path)
        .output()
        .map_err(|error| WorkspaceError::io(path, error.to_string()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(WorkspaceError::io(
            path,
            if stderr.is_empty() {
                "wslpath failed".to_string()
            } else {
                stderr
            },
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn ensure_workspace_config(workspace_root: &Path) -> Result<(), WorkspaceError> {
    let config = load_workspace_config(workspace_root)
        .map_err(WorkspaceError::config)?;
    save_workspace_config(workspace_root, &WorkspaceConfig { ..config })
        .map_err(WorkspaceError::config)
}

fn remember_recent_workspace(user_config: &mut UserConfig, workspace_root: &Path) {
    let root = workspace_root.display().to_string();
    user_config
        .recent_workspaces
        .retain(|recent| recent != &root);
    user_config.recent_workspaces.insert(0, root);
}
