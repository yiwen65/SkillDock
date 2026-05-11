use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use skilldock_lib::{
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
        "skilldock_{name}_{}_{}",
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
    assert_eq!(user_config.window_size.width, 1440);
    assert_eq!(user_config.window_size.height, 900);
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
fn user_preferences_patch_preserves_profiles_window_size_and_recent_workspaces() {
    let config_path = temp_dir("user_preferences_patch").join("config.json");
    let original = UserConfig {
        schema_version: 1,
        recent_workspaces: vec!["/keep-me".to_string(), "/also-keep".to_string()],
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

    // The patch is intentionally narrow: it owns UI preferences and automatic
    // checks, and must not touch state the backend maintains authoritatively
    // (recent_workspaces, agent_profiles, window_size).
    assert_eq!(patched.recent_workspaces, original.recent_workspaces);
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

#[test]
fn user_config_tolerates_missing_optional_fields() {
    let config_path = temp_dir("user_config_missing_fields").join("config.json");
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    // Only the required schemaVersion — every other field must hydrate from the
    // Default impl so a hand-edited or older on-disk config still loads cleanly.
    std::fs::write(&config_path, "{\"schemaVersion\":1}").unwrap();

    let loaded = load_user_config_at(&config_path).unwrap();

    assert_eq!(loaded.schema_version, 1);
    assert!(loaded.recent_workspaces.is_empty());
    // Missing agentProfiles rehydrates to the built-ins so first-run behavior
    // on an older config is preserved.
    assert_eq!(loaded.agent_profiles.len(), 2);
    assert_eq!(loaded.agent_profiles[0].id, "claude-code");
    assert_eq!(loaded.agent_profiles[1].id, "codex");
    assert_eq!(loaded.ui_preferences, UiPreferences::default());
    assert_eq!(loaded.window_size, WindowSize::default());
    assert_eq!(loaded.automatic_checks, AutomaticCheckSettings::default());
}

#[test]
fn user_config_ignores_unknown_fields_for_forward_compat() {
    let config_path = temp_dir("user_config_unknown_fields").join("config.json");
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    // Simulate a config produced by a newer SkillDock version with an
    // unknown top-level field and an unknown nested field inside uiPreferences.
    let json = r#"{
        "schemaVersion": 1,
        "recentWorkspaces": ["/tmp/ws"],
        "uiPreferences": {
            "theme": "dark",
            "projectSort": "name",
            "showHiddenProjects": true,
            "futureUiFlag": 42
        },
        "agentProfiles": [],
        "someUnknownFutureField": {"nested": [1, 2, 3]}
    }"#;
    std::fs::write(&config_path, json).unwrap();

    let loaded = load_user_config_at(&config_path).unwrap();
    assert_eq!(loaded.recent_workspaces, vec!["/tmp/ws".to_string()]);
    assert_eq!(loaded.ui_preferences.theme, ThemePreference::Dark);
    assert!(loaded.ui_preferences.show_hidden_projects);
    // An explicitly empty agentProfiles list is preserved (user intent),
    // distinct from a missing field rehydrating to built-ins.
    assert!(loaded.agent_profiles.is_empty());
    assert_eq!(loaded.window_size, WindowSize::default());
}

#[test]
fn workspace_project_metadata_tolerates_missing_optional_fields() {
    let workspace_root = temp_dir("workspace_metadata_missing_fields");
    let config_path = workspace_config_path(&workspace_root);
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        r#"{
            "schemaVersion": 1,
            "projects": [{"projectId": "only-required"}]
        }"#,
    )
    .unwrap();

    let loaded = load_workspace_config(&workspace_root).unwrap();
    assert_eq!(loaded.projects.len(), 1);
    let project = &loaded.projects[0];
    assert_eq!(project.project_id, "only-required");
    assert!(!project.favorite);
    assert!(!project.hidden);
    assert!(project.tags.is_empty());
    assert!(project.display_name.is_none());
    assert!(project.category.is_none());
    assert!(project.notes.is_none());
    assert!(project.auto_check.is_none());
    assert!(project.auto_pull.is_none());
}

#[test]
fn update_window_size_writes_new_value_and_preserves_other_fields() {
    use skilldock_lib::update_window_size_at;

    let config_path = temp_dir("update_window_size_writes").join("config.json");
    let original = UserConfig {
        recent_workspaces: vec!["/tmp/keep-me".to_string()],
        ui_preferences: UiPreferences {
            theme: ThemePreference::Dark,
            ..UiPreferences::default()
        },
        window_size: WindowSize {
            width: 1000,
            height: 700,
        },
        ..UserConfig::default()
    };
    save_user_config_at(&config_path, &original).unwrap();

    let persisted = update_window_size_at(&config_path, 1600, 950).unwrap().unwrap();
    assert_eq!(persisted, WindowSize { width: 1600, height: 950 });

    let loaded = load_user_config_at(&config_path).unwrap();
    assert_eq!(loaded.window_size, WindowSize { width: 1600, height: 950 });
    assert_eq!(loaded.recent_workspaces, original.recent_workspaces);
    assert_eq!(loaded.ui_preferences.theme, ThemePreference::Dark);
    assert_eq!(loaded.agent_profiles, original.agent_profiles);
}

#[test]
fn update_window_size_ignores_minimised_or_bogus_measurements() {
    use skilldock_lib::update_window_size_at;

    let config_path = temp_dir("update_window_size_ignores").join("config.json");
    let original = UserConfig {
        window_size: WindowSize {
            width: 1280,
            height: 820,
        },
        ..UserConfig::default()
    };
    save_user_config_at(&config_path, &original).unwrap();

    // A zero or sub-threshold reading must not overwrite a good saved size:
    // some desktop environments emit such values while the window is being
    // destroyed.
    assert!(update_window_size_at(&config_path, 0, 0).unwrap().is_none());
    assert!(update_window_size_at(&config_path, 50, 50).unwrap().is_none());

    let loaded = load_user_config_at(&config_path).unwrap();
    assert_eq!(loaded.window_size, original.window_size);
}

#[test]
fn update_window_size_is_idempotent_for_unchanged_values() {
    use skilldock_lib::update_window_size_at;

    let config_path = temp_dir("update_window_size_idempotent").join("config.json");
    let original = UserConfig {
        window_size: WindowSize {
            width: 1440,
            height: 900,
        },
        ..UserConfig::default()
    };
    save_user_config_at(&config_path, &original).unwrap();
    let mtime_before = std::fs::metadata(&config_path).unwrap().modified().unwrap();
    // Ensure any subsequent modification would bump the mtime by at least
    // filesystem granularity.
    std::thread::sleep(std::time::Duration::from_millis(20));

    let persisted = update_window_size_at(&config_path, 1440, 900).unwrap().unwrap();
    assert_eq!(persisted, original.window_size);

    let mtime_after = std::fs::metadata(&config_path).unwrap().modified().unwrap();
    assert_eq!(mtime_before, mtime_after, "config file should not be rewritten when size is unchanged");
}
