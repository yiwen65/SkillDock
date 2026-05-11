use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use skilldock_lib::{
    restore_recent_workspace_at, select_workspace_at, workspace_config_path, WorkspaceErrorKind,
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

#[test]
fn selecting_empty_workspace_initializes_config_and_remembers_recent_path() {
    let workspace_root = temp_dir("empty_workspace");
    let user_config_path = temp_dir("user_config").join("config.json");

    let workspace = select_workspace_at(&workspace_root, &user_config_path).unwrap();

    assert_eq!(
        workspace.root,
        workspace_root.canonicalize().unwrap().display().to_string()
    );
    assert!(workspace.projects.is_empty());
    assert!(workspace.skills.is_empty());
    assert!(workspace_config_path(&workspace_root).exists());

    let restored = restore_recent_workspace_at(&user_config_path)
        .unwrap()
        .unwrap();
    assert_eq!(restored.root, workspace.root);
}

#[test]
fn selecting_invalid_path_returns_clear_structured_error() {
    let workspace_root = temp_dir("missing_parent").join("missing");
    let user_config_path = temp_dir("user_config").join("config.json");

    let error = select_workspace_at(&workspace_root, &user_config_path).unwrap_err();

    assert_eq!(error.kind, WorkspaceErrorKind::PathMissing);
    assert_eq!(error.path, workspace_root.display().to_string());
    assert!(!error.message.is_empty());
}
