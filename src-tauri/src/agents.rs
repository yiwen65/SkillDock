use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    default_user_config_path, load_user_config_at, save_user_config_at, scan_workspace_at,
    AgentProfile, AgentProfileState, ConfigError, UserConfig, Workspace, WorkspaceError,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileError {
    pub kind: AgentProfileErrorKind,
    pub path: Option<String>,
    pub message: String,
}

impl AgentProfileError {
    fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: AgentProfileErrorKind::Validation,
            path: None,
            message: message.into(),
        }
    }

    fn confirmation_required(path: &Path) -> Self {
        Self {
            kind: AgentProfileErrorKind::ConfirmationRequired,
            path: Some(path.display().to_string()),
            message: "Creating this profile directory requires explicit confirmation.".to_string(),
        }
    }

    fn profile_not_found(profile_id: &str) -> Self {
        Self {
            kind: AgentProfileErrorKind::ProfileNotFound,
            path: None,
            message: format!("Agent profile '{profile_id}' was not found."),
        }
    }

    fn config(error: ConfigError) -> Self {
        Self {
            kind: AgentProfileErrorKind::Config,
            path: Some(error.path),
            message: error.message,
        }
    }

    fn workspace(error: WorkspaceError) -> Self {
        Self {
            kind: AgentProfileErrorKind::Workspace,
            path: Some(error.path),
            message: error.message,
        }
    }

    fn io(path: &Path, error: std::io::Error) -> Self {
        Self {
            kind: AgentProfileErrorKind::Io,
            path: Some(path.display().to_string()),
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProfileErrorKind {
    Validation,
    ConfirmationRequired,
    ProfileNotFound,
    Config,
    Workspace,
    Io,
}

pub fn list_agent_profile_states_at(
    workspace_root: impl AsRef<Path>,
    user_config_path: impl AsRef<Path>,
) -> Result<Vec<AgentProfileState>, AgentProfileError> {
    let config = load_user_config_at(user_config_path).map_err(AgentProfileError::config)?;
    let workspace = scan_workspace_at(workspace_root, &config.agent_profiles)
        .map_err(AgentProfileError::workspace)?;
    Ok(workspace.agent_profiles)
}

pub fn save_agent_profiles_at(
    user_config_path: impl AsRef<Path>,
    profiles: Vec<AgentProfile>,
) -> Result<UserConfig, AgentProfileError> {
    validate_agent_profiles(&profiles)?;

    let mut config =
        load_user_config_at(user_config_path.as_ref()).map_err(AgentProfileError::config)?;
    config.agent_profiles = profiles;
    save_user_config_at(user_config_path.as_ref(), &config).map_err(AgentProfileError::config)?;
    Ok(config)
}

pub fn create_agent_profile_dir_at(
    workspace_root: impl AsRef<Path>,
    user_config_path: impl AsRef<Path>,
    profile_id: &str,
    confirmed: bool,
) -> Result<Workspace, AgentProfileError> {
    let config =
        load_user_config_at(user_config_path.as_ref()).map_err(AgentProfileError::config)?;
    validate_agent_profiles(&config.agent_profiles)?;
    let profile = config
        .agent_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| AgentProfileError::profile_not_found(profile_id))?;
    let skills_dir = expand_home(&profile.skills_dir);

    if !confirmed {
        return Err(AgentProfileError::confirmation_required(&skills_dir));
    }

    fs::create_dir_all(&skills_dir).map_err(|error| AgentProfileError::io(&skills_dir, error))?;
    scan_workspace_at(workspace_root, &config.agent_profiles).map_err(AgentProfileError::workspace)
}

pub fn default_install_targets(agent_profiles: &[AgentProfile]) -> Vec<AgentProfile> {
    agent_profiles
        .iter()
        .filter(|profile| profile.enabled)
        .cloned()
        .collect()
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn list_agent_profile_states_command(
    workspace_root: String,
) -> Result<Vec<AgentProfileState>, AgentProfileError> {
    list_agent_profile_states_at(workspace_root, default_user_config_path())
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn save_agent_profiles_command(
    profiles: Vec<AgentProfile>,
) -> Result<UserConfig, AgentProfileError> {
    save_agent_profiles_at(default_user_config_path(), profiles)
}

#[cfg_attr(feature = "desktop", tauri::command(async, rename_all = "camelCase"))]
pub fn create_agent_profile_dir_command(
    workspace_root: String,
    profile_id: String,
    confirmed: bool,
) -> Result<Workspace, AgentProfileError> {
    create_agent_profile_dir_at(
        workspace_root,
        default_user_config_path(),
        &profile_id,
        confirmed,
    )
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn default_install_targets_command() -> Result<Vec<AgentProfile>, AgentProfileError> {
    let config =
        load_user_config_at(default_user_config_path()).map_err(AgentProfileError::config)?;
    Ok(default_install_targets(&config.agent_profiles))
}

fn validate_agent_profiles(profiles: &[AgentProfile]) -> Result<(), AgentProfileError> {
    let mut ids = HashSet::new();
    let mut skills_dirs = HashSet::new();
    for profile in profiles {
        let id = profile.id.trim();
        if id.is_empty() {
            return Err(AgentProfileError::validation(
                "Agent profile id is required.",
            ));
        }
        if profile.id != id {
            return Err(AgentProfileError::validation(format!(
                "Agent profile id '{id}' must not contain leading or trailing whitespace."
            )));
        }
        if !is_valid_profile_id(id) {
            return Err(AgentProfileError::validation(format!(
                "Agent profile id '{id}' may only use letters, numbers, dots, underscores and hyphens."
            )));
        }
        if profile.name.trim().is_empty() {
            return Err(AgentProfileError::validation(format!(
                "Agent profile '{id}' requires a name."
            )));
        }
        let skills_dir = profile.skills_dir.trim();
        if skills_dir.is_empty() {
            return Err(AgentProfileError::validation(format!(
                "Agent profile '{id}' requires a skills directory."
            )));
        }
        if !is_valid_skills_dir(skills_dir) {
            return Err(AgentProfileError::validation(format!(
                "Agent profile '{id}' requires an absolute or home-relative skills directory."
            )));
        }
        if !ids.insert(id.to_string()) {
            return Err(AgentProfileError::validation(format!(
                "Agent profile id '{id}' is duplicated."
            )));
        }
        let normalized_skills_dir = normalize_skills_dir_for_compare(skills_dir);
        if !skills_dirs.insert(normalized_skills_dir) {
            return Err(AgentProfileError::validation(format!(
                "Agent profile skills directory '{skills_dir}' is duplicated."
            )));
        }
    }

    Ok(())
}

fn is_valid_profile_id(id: &str) -> bool {
    id.chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
}

fn is_valid_skills_dir(path: &str) -> bool {
    if path.contains('\0') {
        return false;
    }

    if path.starts_with("\\\\") || path.starts_with("//") {
        return is_valid_unc_path(path);
    }

    path == "~"
        || path.starts_with("~/")
        || path.starts_with("~\\")
        || Path::new(path).is_absolute()
        || is_windows_absolute_path(path)
}

fn is_windows_absolute_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
    {
        return true;
    }

    false
}

fn is_valid_unc_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let Some(rest) = normalized.strip_prefix("//") else {
        return false;
    };
    let mut parts = rest.split('/').filter(|part| !part.is_empty());
    parts.next().is_some() && parts.next().is_some()
}

fn normalize_skills_dir_for_compare(path: &str) -> String {
    let replaced = path.trim().replace('\\', "/");
    let mut normalized = String::with_capacity(replaced.len());
    let mut previous_was_slash = false;
    let preserve_unc_prefix = replaced.starts_with("//");

    for (index, character) in replaced.chars().enumerate() {
        if character == '/' {
            if preserve_unc_prefix && index < 2 {
                normalized.push(character);
            } else if !previous_was_slash {
                normalized.push(character);
            }
            previous_was_slash = true;
        } else {
            normalized.push(character);
            previous_was_slash = false;
        }
    }

    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }

    if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
        normalized.replace_range(0..1, &normalized[0..1].to_ascii_lowercase());
    }

    normalized
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(path));
    }

    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }

    PathBuf::from(path)
}
