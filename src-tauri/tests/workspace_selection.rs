use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use skilldock_lib::{
    load_user_config_at, restore_recent_workspace_at, select_workspace_at, workspace_config_path,
    WorkspaceErrorKind, MAX_RECENT_WORKSPACES,
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

#[test]
fn recent_workspaces_cap_evicts_oldest_and_dedupes_reselection() {
    let user_config_path = temp_dir("recent_cap_user_config").join("config.json");
    let parent = temp_dir("recent_cap_workspaces");
    let workspace_count = MAX_RECENT_WORKSPACES + 5;
    let mut canonical_roots = Vec::with_capacity(workspace_count);

    for index in 0..workspace_count {
        let workspace = parent.join(format!("ws-{index:03}"));
        std::fs::create_dir_all(&workspace).unwrap();
        select_workspace_at(&workspace, &user_config_path).unwrap();
        canonical_roots.push(workspace.canonicalize().unwrap().display().to_string());
    }

    let config = load_user_config_at(&user_config_path).unwrap();
    assert_eq!(config.recent_workspaces.len(), MAX_RECENT_WORKSPACES);
    // Most recent selection should be at index 0.
    assert_eq!(
        config.recent_workspaces.first().unwrap(),
        canonical_roots.last().unwrap()
    );
    // Oldest selections beyond the cap must be evicted.
    for stale in canonical_roots.iter().take(workspace_count - MAX_RECENT_WORKSPACES) {
        assert!(!config.recent_workspaces.contains(stale));
    }

    // Re-selecting an existing recent root must move it to the front without
    // growing the list or leaving a duplicate.
    let to_reselect = canonical_roots[workspace_count - 3].clone();
    select_workspace_at(PathBuf::from(&to_reselect), &user_config_path).unwrap();
    let config = load_user_config_at(&user_config_path).unwrap();
    assert_eq!(config.recent_workspaces.len(), MAX_RECENT_WORKSPACES);
    assert_eq!(config.recent_workspaces.first().unwrap(), &to_reselect);
    assert_eq!(
        config
            .recent_workspaces
            .iter()
            .filter(|root| *root == &to_reselect)
            .count(),
        1
    );
}
