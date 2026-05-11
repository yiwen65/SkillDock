use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use skilldock_lib::{
    create_agent_profile_dir_at, default_install_targets, list_agent_profile_states_at,
    save_agent_profiles_at, AgentProfile, AgentProfileErrorKind, LinkMode, UserConfig,
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
    // See tests/git_operations.rs::temp_dir for the macOS symlink rationale.
    std::fs::canonicalize(&dir).unwrap()
}

fn profile(id: &str, name: &str, skills_dir: &Path, enabled: bool) -> AgentProfile {
    AgentProfile {
        id: id.to_string(),
        name: name.to_string(),
        skills_dir: skills_dir.display().to_string(),
        enabled,
        built_in: false,
        link_mode: LinkMode::Symlink,
    }
}

#[test]
fn saves_valid_profiles_lists_states_creates_missing_dirs_only_when_confirmed() {
    let workspace_root = temp_dir("agent_profiles_workspace");
    let config_path = temp_dir("agent_profiles_config").join("config.json");
    let existing_dir = temp_dir("agent_profiles_existing");
    let missing_dir = temp_dir("agent_profiles_missing_parent")
        .join("missing-agent")
        .join("skills");

    let profiles = vec![
        profile("enabled-agent", "Enabled Agent", &existing_dir, true),
        profile("disabled-agent", "Disabled Agent", &missing_dir, false),
    ];

    let saved = save_agent_profiles_at(&config_path, profiles.clone()).unwrap();
    assert_eq!(saved.agent_profiles, profiles);

    let states = list_agent_profile_states_at(&workspace_root, &config_path).unwrap();
    assert_eq!(states.len(), 2);
    assert!(
        states
            .iter()
            .find(|state| state.profile.id == "enabled-agent")
            .unwrap()
            .exists
    );
    let missing = states
        .iter()
        .find(|state| state.profile.id == "disabled-agent")
        .unwrap();
    assert!(!missing.exists);
    assert!(!missing.writable);

    let targets = default_install_targets(&saved.agent_profiles);
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].id, "enabled-agent");

    let error = create_agent_profile_dir_at(&workspace_root, &config_path, "disabled-agent", false)
        .unwrap_err();
    assert_eq!(error.kind, AgentProfileErrorKind::ConfirmationRequired);
    assert!(!missing_dir.exists());

    let workspace =
        create_agent_profile_dir_at(&workspace_root, &config_path, "disabled-agent", true).unwrap();
    let created = workspace
        .agent_profiles
        .iter()
        .find(|state| state.profile.id == "disabled-agent")
        .unwrap();
    assert!(created.exists);
    assert!(created.writable);
    assert!(missing_dir.is_dir());
}

#[test]
fn rejects_invalid_profile_sets_without_overwriting_config() {
    let config_path = temp_dir("agent_profiles_invalid_config").join("config.json");
    let valid_dir = temp_dir("agent_profiles_valid");
    save_agent_profiles_at(
        &config_path,
        vec![profile("valid-agent", "Valid Agent", &valid_dir, true)],
    )
    .unwrap();

    let duplicate_error = save_agent_profiles_at(
        &config_path,
        vec![
            profile("duplicate", "One", &valid_dir, true),
            profile("duplicate", "Two", &valid_dir, true),
        ],
    )
    .unwrap_err();
    assert_eq!(duplicate_error.kind, AgentProfileErrorKind::Validation);

    let blank_error = save_agent_profiles_at(
        &config_path,
        vec![AgentProfile {
            id: " ".to_string(),
            name: "Blank".to_string(),
            skills_dir: valid_dir.display().to_string(),
            enabled: true,
            built_in: false,
            link_mode: LinkMode::Symlink,
        }],
    )
    .unwrap_err();
    assert_eq!(blank_error.kind, AgentProfileErrorKind::Validation);

    let relative_path_error = save_agent_profiles_at(
        &config_path,
        vec![AgentProfile {
            id: "relative-agent".to_string(),
            name: "Relative".to_string(),
            skills_dir: "relative/skills".to_string(),
            enabled: true,
            built_in: false,
            link_mode: LinkMode::Symlink,
        }],
    )
    .unwrap_err();
    assert_eq!(relative_path_error.kind, AgentProfileErrorKind::Validation);

    let invalid_id_error = save_agent_profiles_at(
        &config_path,
        vec![AgentProfile {
            id: "bad id".to_string(),
            name: "Bad Id".to_string(),
            skills_dir: valid_dir.display().to_string(),
            enabled: true,
            built_in: false,
            link_mode: LinkMode::Symlink,
        }],
    )
    .unwrap_err();
    assert_eq!(invalid_id_error.kind, AgentProfileErrorKind::Validation);

    let duplicate_dir_error = save_agent_profiles_at(
        &config_path,
        vec![
            AgentProfile {
                id: "agent-one".to_string(),
                name: "Agent One".to_string(),
                skills_dir: "~//agents//shared/".to_string(),
                enabled: true,
                built_in: false,
                link_mode: LinkMode::Symlink,
            },
            AgentProfile {
                id: "agent-two".to_string(),
                name: "Agent Two".to_string(),
                skills_dir: "~/agents/shared".to_string(),
                enabled: true,
                built_in: false,
                link_mode: LinkMode::Symlink,
            },
        ],
    )
    .unwrap_err();
    assert_eq!(duplicate_dir_error.kind, AgentProfileErrorKind::Validation);

    let trimmed_id_error = save_agent_profiles_at(
        &config_path,
        vec![AgentProfile {
            id: " valid-id ".to_string(),
            name: "Trimmed Id".to_string(),
            skills_dir: valid_dir.display().to_string(),
            enabled: true,
            built_in: false,
            link_mode: LinkMode::Symlink,
        }],
    )
    .unwrap_err();
    assert_eq!(trimmed_id_error.kind, AgentProfileErrorKind::Validation);

    let bare_unc_error = save_agent_profiles_at(
        &config_path,
        vec![AgentProfile {
            id: "bare-unc".to_string(),
            name: "Bare UNC".to_string(),
            skills_dir: "//".to_string(),
            enabled: true,
            built_in: false,
            link_mode: LinkMode::Symlink,
        }],
    )
    .unwrap_err();
    assert_eq!(bare_unc_error.kind, AgentProfileErrorKind::Validation);

    let bare_backslash_unc_error = save_agent_profiles_at(
        &config_path,
        vec![AgentProfile {
            id: "bare-backslash-unc".to_string(),
            name: "Bare Backslash UNC".to_string(),
            skills_dir: "\\\\".to_string(),
            enabled: true,
            built_in: false,
            link_mode: LinkMode::Symlink,
        }],
    )
    .unwrap_err();
    assert_eq!(
        bare_backslash_unc_error.kind,
        AgentProfileErrorKind::Validation
    );

    let loaded = UserConfig::default();
    let persisted = skilldock_lib::load_user_config_at(&config_path).unwrap();
    assert_eq!(persisted.agent_profiles[0].id, "valid-agent");
    assert_ne!(persisted.agent_profiles, loaded.agent_profiles);
}

#[test]
fn accepts_supported_profile_id_and_path_formats() {
    let config_path = temp_dir("agent_profiles_path_formats").join("config.json");

    let saved = save_agent_profiles_at(
        &config_path,
        vec![
            AgentProfile {
                id: "unix.absolute".to_string(),
                name: "Unix Absolute".to_string(),
                skills_dir: "/tmp/agent-skills".to_string(),
                enabled: true,
                built_in: false,
                link_mode: LinkMode::Symlink,
            },
            AgentProfile {
                id: "home_relative".to_string(),
                name: "Home Relative".to_string(),
                skills_dir: "~/agent-skills".to_string(),
                enabled: true,
                built_in: false,
                link_mode: LinkMode::Symlink,
            },
            AgentProfile {
                id: "windows-drive".to_string(),
                name: "Windows Drive".to_string(),
                skills_dir: "C:\\Users\\agent\\skills".to_string(),
                enabled: true,
                built_in: false,
                link_mode: LinkMode::Symlink,
            },
            AgentProfile {
                id: "unc_path-1".to_string(),
                name: "UNC Path".to_string(),
                skills_dir: "\\\\server\\share\\skills".to_string(),
                enabled: true,
                built_in: false,
                link_mode: LinkMode::Symlink,
            },
        ],
    )
    .unwrap();

    assert_eq!(saved.agent_profiles.len(), 4);
}
