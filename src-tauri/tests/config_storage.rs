use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use skills_collection_app_lib::{
    load_user_config_at, load_workspace_config, patch_user_preferences_at, save_user_config_at,
    save_workspace_config, workspace_config_path, AgentProfile, AutomaticCheckSettings,
    ConfigErrorKind, LinkMode, ProjectCategory, ThemePreference, UiPreferences, UserConfig,
    UserPreferencesPatch, WindowSize, WorkspaceConfig, WorkspaceProjectMetadata,
};

fn temp_dir(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "skills_collection_app_{name}_{}_{}",
        std::process::id(),
        unique
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn missing_config_files_load_as_defaults() {
    let workspace_root = temp_dir("missing_workspace_config");
    let user_config_path = temp_dir("missing_user_config").join("config.json");

    let workspace_config = load_workspace_config(&workspace_root).unwrap();
    let user_config = load_user_config_at(&user_config_path).unwrap();

    assert_eq!(workspace_config.schema_version, 1);
    assert!(workspace_config.projects.is_empty());

    assert_eq!(user_config.schema_version, 1);
    assert_eq!(user_config.recent_workspaces, Vec::<String>::new());
    assert_eq!(user_config.agent_profiles.len(), 2);
    assert_eq!(user_config.agent_profiles[0].id, "claude-code");
    assert_eq!(user_config.agent_profiles[0].skills_dir, "~/.claude/skills");
    assert_eq!(user_config.agent_profiles[1].id, "codex");
    assert_eq!(user_config.agent_profiles[1].skills_dir, "~/.codex/skills");
    assert_eq!(user_config.window_size.width, 1200);
    assert_eq!(user_config.automatic_checks.enabled, false);
}

#[test]
fn workspace_config_round_trips_project_metadata_only() {
    let workspace_root = temp_dir("workspace_metadata");
    let config = WorkspaceConfig {
        schema_version: 1,
        projects: vec![WorkspaceProjectMetadata {
            project_id: "superpowers".to_string(),
            display_name: Some("Superpowers".to_string()),
            category: Some(ProjectCategory::Skills),
            favorite: true,
            hidden: false,
            tags: vec!["workflow".to_string(), "skills".to_string()],
            notes: Some("Strong TDD docs".to_string()),
            auto_check: Some(true),
            auto_pull: Some(false),
        }],
    };

    save_workspace_config(&workspace_root, &config).unwrap();

    let raw = std::fs::read_to_string(workspace_config_path(&workspace_root)).unwrap();
    assert!(raw.contains("\"schemaVersion\""));
    assert!(raw.contains("\"projectId\""));
    assert!(!raw.contains("gitStatus"));
    assert!(!raw.contains("remoteUrl"));
    assert!(!raw.contains("\"path\""));

    let loaded = load_workspace_config(&workspace_root).unwrap();
    assert_eq!(loaded, config);
}

#[test]
fn user_config_round_trips_profiles_preferences_window_and_checks() {
    let config_path = temp_dir("user_config_round_trip").join("config.json");
    let config = UserConfig {
        schema_version: 1,
        recent_workspaces: vec!["/collections/skills".to_string()],
        agent_profiles: vec![AgentProfile {
            id: "custom-agent".to_string(),
            name: "Custom Agent".to_string(),
            skills_dir: "/agents/custom/skills".to_string(),
            enabled: true,
            built_in: false,
            link_mode: LinkMode::Symlink,
        }],
        ui_preferences: UiPreferences {
            theme: ThemePreference::Dark,
            ..UiPreferences::default()
        },
        window_size: WindowSize {
            width: 1440,
            height: 960,
        },
        automatic_checks: AutomaticCheckSettings {
            enabled: true,
            interval_minutes: 30,
            pull_after_check: false,
        },
    };

    save_user_config_at(&config_path, &config).unwrap();

    let loaded = load_user_config_at(&config_path).unwrap();
    assert_eq!(loaded, config);
}

#[test]
fn user_preferences_patch_preserves_profiles_and_window_size() {
    let config_path = temp_dir("user_preferences_patch").join("config.json");
    let original = UserConfig {
        schema_version: 1,
        recent_workspaces: vec!["/old".to_string()],
        agent_profiles: vec![AgentProfile {
            id: "custom-agent".to_string(),
            name: "Custom Agent".to_string(),
            skills_dir: "/agents/custom/skills".to_string(),
            enabled: true,
            built_in: false,
            link_mode: LinkMode::Symlink,
        }],
        ui_preferences: UiPreferences::default(),
        window_size: WindowSize {
            width: 1600,
            height: 900,
        },
        automatic_checks: AutomaticCheckSettings::default(),
    };
    save_user_config_at(&config_path, &original).unwrap();

    let patched = patch_user_preferences_at(
        &config_path,
        UserPreferencesPatch {
            recent_workspaces: vec!["/new".to_string()],
            ui_preferences: UiPreferences {
                theme: ThemePreference::Dark,
                project_sort: original.ui_preferences.project_sort.clone(),
                show_hidden_projects: true,
            },
            automatic_checks: AutomaticCheckSettings {
                enabled: true,
                interval_minutes: 15,
                pull_after_check: true,
            },
        },
    )
    .unwrap();

    assert_eq!(patched.recent_workspaces, vec!["/new"]);
    assert_eq!(patched.agent_profiles, original.agent_profiles);
    assert_eq!(patched.window_size, original.window_size);
    assert_eq!(patched.automatic_checks.interval_minutes, 15);
    assert!(patched.ui_preferences.show_hidden_projects);
}

#[test]
fn invalid_config_json_returns_structured_error_with_file_path() {
    let workspace_root = temp_dir("invalid_workspace_config");
    let config_path = workspace_config_path(&workspace_root);
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(&config_path, "{ invalid json").unwrap();

    let error = load_workspace_config(&workspace_root).unwrap_err();

    assert_eq!(error.kind, ConfigErrorKind::InvalidJson);
    assert_eq!(error.path, config_path.display().to_string());
    assert!(!error.message.is_empty());
}
