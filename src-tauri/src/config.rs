use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{AgentProfile, LinkMode, ProjectCategory};

const CONFIG_SCHEMA_VERSION: u32 = 1;
const WORKSPACE_CONFIG_DIR: &str = ".skilldock";
const WORKSPACE_CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConfig {
    pub schema_version: u32,
    pub projects: Vec<WorkspaceProjectMetadata>,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            projects: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceProjectMetadata {
    pub project_id: String,
    pub display_name: Option<String>,
    pub category: Option<ProjectCategory>,
    pub favorite: bool,
    pub hidden: bool,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub auto_check: Option<bool>,
    pub auto_pull: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserConfig {
    pub schema_version: u32,
    pub recent_workspaces: Vec<String>,
    pub agent_profiles: Vec<AgentProfile>,
    pub ui_preferences: UiPreferences,
    pub window_size: WindowSize,
    pub automatic_checks: AutomaticCheckSettings,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            recent_workspaces: Vec::new(),
            agent_profiles: vec![
                AgentProfile {
                    id: "claude-code".to_string(),
                    name: "Claude Code".to_string(),
                    skills_dir: "~/.claude/skills".to_string(),
                    enabled: true,
                    built_in: true,
                    link_mode: LinkMode::Symlink,
                },
                AgentProfile {
                    id: "codex".to_string(),
                    name: "Codex".to_string(),
                    skills_dir: "~/.codex/skills".to_string(),
                    enabled: true,
                    built_in: true,
                    link_mode: LinkMode::Symlink,
                },
            ],
            ui_preferences: UiPreferences::default(),
            window_size: WindowSize::default(),
            automatic_checks: AutomaticCheckSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPreferences {
    pub theme: ThemePreference,
    pub project_sort: ProjectSort,
    pub show_hidden_projects: bool,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            // Ship a dark theme for first-run users. Existing installs keep
            // whatever is in their saved config.json because this default is
            // only consulted when no config exists.
            theme: ThemePreference::Dark,
            project_sort: ProjectSort::Name,
            show_hidden_projects: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSort {
    Name,
    Updated,
    SkillCount,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

impl Default for WindowSize {
    fn default() -> Self {
        Self {
            width: 1440,
            height: 900,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomaticCheckSettings {
    pub enabled: bool,
    pub interval_minutes: u32,
    pub pull_after_check: bool,
}

impl Default for AutomaticCheckSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: 60,
            pull_after_check: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPreferencesPatch {
    pub recent_workspaces: Vec<String>,
    pub ui_preferences: UiPreferences,
    pub automatic_checks: AutomaticCheckSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigError {
    pub kind: ConfigErrorKind,
    pub path: String,
    pub message: String,
}

impl ConfigError {
    fn io(path: &Path, error: io::Error) -> Self {
        Self {
            kind: ConfigErrorKind::Io,
            path: path.display().to_string(),
            message: error.to_string(),
        }
    }

    fn invalid_json(path: &Path, error: serde_json::Error) -> Self {
        Self {
            kind: ConfigErrorKind::InvalidJson,
            path: path.display().to_string(),
            message: error.to_string(),
        }
    }

    fn unsupported_version(path: &Path, schema_version: u32) -> Self {
        Self {
            kind: ConfigErrorKind::UnsupportedVersion,
            path: path.display().to_string(),
            message: format!("unsupported config schema version {schema_version}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigErrorKind {
    Io,
    InvalidJson,
    Serialize,
    UnsupportedVersion,
}

trait VersionedConfig {
    fn schema_version(&self) -> u32;
}

impl VersionedConfig for WorkspaceConfig {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

impl VersionedConfig for UserConfig {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

pub fn workspace_config_path(workspace_root: impl AsRef<Path>) -> PathBuf {
    workspace_root
        .as_ref()
        .join(WORKSPACE_CONFIG_DIR)
        .join(WORKSPACE_CONFIG_FILE)
}

pub fn load_workspace_config(
    workspace_root: impl AsRef<Path>,
) -> Result<WorkspaceConfig, ConfigError> {
    load_config_or_default(&workspace_config_path(workspace_root))
}

pub fn load_user_config_at(config_path: impl AsRef<Path>) -> Result<UserConfig, ConfigError> {
    load_config_or_default(config_path.as_ref())
}

pub fn load_user_config() -> Result<UserConfig, ConfigError> {
    load_user_config_at(default_user_config_path())
}

pub fn save_workspace_config(
    workspace_root: impl AsRef<Path>,
    config: &WorkspaceConfig,
) -> Result<(), ConfigError> {
    save_config_atomically(&workspace_config_path(workspace_root), config)
}

pub fn save_user_config_at(
    config_path: impl AsRef<Path>,
    config: &UserConfig,
) -> Result<(), ConfigError> {
    save_config_atomically(config_path.as_ref(), config)
}

pub fn save_user_config(config: &UserConfig) -> Result<(), ConfigError> {
    save_user_config_at(default_user_config_path(), config)
}

pub fn patch_user_preferences_at(
    config_path: impl AsRef<Path>,
    patch: UserPreferencesPatch,
) -> Result<UserConfig, ConfigError> {
    let mut config = load_user_config_at(config_path.as_ref())?;
    config.recent_workspaces = patch.recent_workspaces;
    config.ui_preferences = patch.ui_preferences;
    config.automatic_checks = patch.automatic_checks;
    save_user_config_at(config_path.as_ref(), &config)?;
    Ok(config)
}

pub fn patch_user_preferences(patch: UserPreferencesPatch) -> Result<UserConfig, ConfigError> {
    patch_user_preferences_at(default_user_config_path(), patch)
}

#[cfg_attr(feature = "desktop", tauri::command(rename_all = "camelCase"))]
pub fn load_workspace_config_command(
    workspace_root: String,
) -> Result<WorkspaceConfig, ConfigError> {
    load_workspace_config(workspace_root)
}

#[cfg_attr(feature = "desktop", tauri::command(rename_all = "camelCase"))]
pub fn save_workspace_config_command(
    workspace_root: String,
    config: WorkspaceConfig,
) -> Result<WorkspaceConfig, ConfigError> {
    save_workspace_config(workspace_root, &config)?;
    Ok(config)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn load_user_config_command() -> Result<UserConfig, ConfigError> {
    load_user_config()
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn save_user_config_command(config: UserConfig) -> Result<UserConfig, ConfigError> {
    save_user_config(&config)?;
    Ok(config)
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn patch_user_preferences_command(
    patch: UserPreferencesPatch,
) -> Result<UserConfig, ConfigError> {
    patch_user_preferences(patch)
}

pub fn default_user_config_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("SkillDock")
                .join("config.json");
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config_home)
                .join("skilldock")
                .join("config.json");
        }

        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".config")
                .join("skilldock")
                .join("config.json");
        }
    }

    PathBuf::from("skilldock-config.json")
}

fn load_config_or_default<T>(path: &Path) -> Result<T, ConfigError>
where
    T: Default + DeserializeOwned + VersionedConfig,
{
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(T::default()),
        Err(error) => return Err(ConfigError::io(path, error)),
    };

    let config: T =
        serde_json::from_str(&contents).map_err(|error| ConfigError::invalid_json(path, error))?;
    if config.schema_version() != CONFIG_SCHEMA_VERSION {
        return Err(ConfigError::unsupported_version(
            path,
            config.schema_version(),
        ));
    }

    Ok(config)
}

fn save_config_atomically<T>(path: &Path, config: &T) -> Result<(), ConfigError>
where
    T: Serialize + VersionedConfig,
{
    if config.schema_version() != CONFIG_SCHEMA_VERSION {
        return Err(ConfigError::unsupported_version(
            path,
            config.schema_version(),
        ));
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| ConfigError::io(parent, error))?;
    }

    let bytes = serde_json::to_vec_pretty(config).map_err(|error| ConfigError {
        kind: ConfigErrorKind::Serialize,
        path: path.display().to_string(),
        message: error.to_string(),
    })?;
    let tmp_path = temporary_path_for(path);

    let write_result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp_path, path)?;
        if let Some(parent) = path.parent() {
            if let Ok(parent_dir) = fs::File::open(parent) {
                let _ = parent_dir.sync_all();
            }
        }
        Ok::<(), io::Error>(())
    })();

    if let Err(error) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(ConfigError::io(path, error));
    }

    Ok(())
}

fn temporary_path_for(path: &Path) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.json");
    path.with_file_name(format!(
        ".{file_name}.tmp.{}.{}",
        std::process::id(),
        unique
    ))
}
