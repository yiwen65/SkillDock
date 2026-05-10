use skills_collection_app_lib::health_check;

#[test]
fn health_check_returns_placeholder_workspace() {
    let workspace = health_check();

    assert_eq!(workspace.root, "");
    assert!(workspace.projects.is_empty());
    assert!(workspace.skills.is_empty());
}
