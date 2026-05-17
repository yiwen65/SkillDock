use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use skilldock_lib::{
    import_project_at, initialize_catalog_git_sync_at, load_workspace_catalog_summary_at,
    publish_catalog_git_sync_at, pull_catalog_git_sync_at, restore_missing_catalog_repositories_at,
    restore_missing_catalog_repositories_background_at, sync_workspace_catalog_from_projects_at,
    task_queue, ImportProjectRequest, TaskStatus,
};

fn temp_dir(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "skilldock_catalog_{name}_{}_{}",
        std::process::id(),
        unique
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_source_repo(name: &str) -> PathBuf {
    let source = temp_dir(name);
    git(&source, &["init"]);
    git(&source, &["config", "user.email", "test@example.com"]);
    git(&source, &["config", "user.name", "Test User"]);
    std::fs::write(source.join("README.md"), "# Source\n").unwrap();
    git(&source, &["add", "README.md"]);
    git(&source, &["commit", "-m", "initial"]);
    source
}

fn add_skill(repo: &Path, relative_path: &str, name: &str) {
    let skill_dir = repo.join(relative_path);
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {name} skill\n---\n"),
    )
    .unwrap();
}

fn clone_project(remote: &Path, workspace: &Path, name: &str) -> PathBuf {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["clone", remote.to_str().unwrap(), name])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git clone failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    workspace.join(name)
}

fn init_bare_remote(name: &str) -> PathBuf {
    let remote = temp_dir(name);
    git(&remote, &["init", "--bare"]);
    remote
}

fn copy_dir_all(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir_all(&source_path, &target_path);
        } else {
            std::fs::copy(&source_path, &target_path).unwrap();
        }
    }
}

#[test]
fn catalog_sync_from_projects_writes_portable_repository_records() {
    let workspace = temp_dir("sync_from_projects");
    let source = init_source_repo("sync_source");
    clone_project(&source, &workspace, "source-copy");

    let before = load_workspace_catalog_summary_at(&workspace, &[]).unwrap();
    assert_eq!(before.active_count, 0);
    assert_eq!(before.local_only_count, 1);

    let after = sync_workspace_catalog_from_projects_at(&workspace, &[]).unwrap();

    assert_eq!(after.active_count, 1);
    assert_eq!(after.missing_count, 0);
    assert_eq!(after.local_only_count, 0);
    assert_eq!(after.repositories[0].directory_name, "source-copy");
    assert_eq!(
        after.repositories[0].remote_url,
        source.display().to_string()
    );
    assert_eq!(after.repositories[0].skill_path, None);
    assert!(workspace.join(".skilldock/catalog/repos").is_dir());
}

#[test]
fn restore_missing_catalog_repositories_clones_catalog_entries_on_new_workspace() {
    let source = init_source_repo("restore_source");
    let old_workspace = temp_dir("restore_old_workspace");
    clone_project(&source, &old_workspace, "restored-project");
    sync_workspace_catalog_from_projects_at(&old_workspace, &[]).unwrap();

    let new_workspace = temp_dir("restore_new_workspace");
    copy_dir_all(
        &old_workspace.join(".skilldock"),
        &new_workspace.join(".skilldock"),
    );

    let before = load_workspace_catalog_summary_at(&new_workspace, &[]).unwrap();
    assert_eq!(before.active_count, 1);
    assert_eq!(before.missing_count, 1);
    assert_eq!(before.repositories[0].skill_path, None);

    let result = restore_missing_catalog_repositories_at(&new_workspace, &[]).unwrap();

    assert_eq!(result.task.status, TaskStatus::Succeeded);
    assert!(new_workspace.join("restored-project/.git").exists());
    assert!(result
        .workspace
        .projects
        .iter()
        .any(|project| project.id == "restored-project"));
}

#[test]
fn catalog_sync_persists_sparse_skill_path_for_selective_imports() {
    let workspace = temp_dir("sync_sparse_project");
    let source = init_source_repo("sync_sparse_source");
    add_skill(&source, "skills/tdd", "TDD");
    add_skill(&source, "skills/other", "Other");
    git(&source, &["add", "skills"]);
    git(&source, &["commit", "-m", "add skills"]);

    let imported = import_project_at(
        &workspace,
        &[],
        ImportProjectRequest {
            source: source.display().to_string(),
            directory_name: Some("sparse-project".to_string()),
            shallow: false,
            skill_path: Some("skills/tdd".to_string()),
        },
    )
    .unwrap();
    assert_eq!(imported.task.status, TaskStatus::Succeeded);

    let summary = sync_workspace_catalog_from_projects_at(&workspace, &[]).unwrap();

    assert_eq!(summary.active_count, 1);
    assert_eq!(summary.repositories[0].directory_name, "sparse-project");
    assert_eq!(
        summary.repositories[0].skill_path,
        Some("skills/tdd".to_string())
    );
}

#[test]
fn restore_missing_catalog_repositories_recreates_sparse_skill_imports() {
    let source = init_source_repo("restore_sparse_source");
    add_skill(&source, "skills/tdd", "TDD");
    add_skill(&source, "skills/other", "Other");
    git(&source, &["add", "skills"]);
    git(&source, &["commit", "-m", "add skills"]);

    let old_workspace = temp_dir("restore_sparse_old_workspace");
    import_project_at(
        &old_workspace,
        &[],
        ImportProjectRequest {
            source: source.display().to_string(),
            directory_name: Some("restored-sparse".to_string()),
            shallow: false,
            skill_path: Some("skills/tdd".to_string()),
        },
    )
    .unwrap();
    sync_workspace_catalog_from_projects_at(&old_workspace, &[]).unwrap();

    let new_workspace = temp_dir("restore_sparse_new_workspace");
    copy_dir_all(
        &old_workspace.join(".skilldock"),
        &new_workspace.join(".skilldock"),
    );

    let result = restore_missing_catalog_repositories_at(&new_workspace, &[]).unwrap();

    assert_eq!(result.task.status, TaskStatus::Succeeded);
    assert!(new_workspace
        .join("restored-sparse/skills/tdd/SKILL.md")
        .is_file());
    assert!(!new_workspace
        .join("restored-sparse/skills/other/SKILL.md")
        .exists());
    assert!(result
        .workspace
        .skills
        .iter()
        .any(|skill| skill.id == "restored-sparse/skills/tdd"));
    assert!(!result
        .workspace
        .skills
        .iter()
        .any(|skill| skill.id == "restored-sparse/skills/other"));
}

#[test]
fn catalog_git_sync_round_trips_repository_list_between_devices() {
    let catalog_remote = init_bare_remote("catalog_remote");
    let source = init_source_repo("catalog_round_trip_source");

    let device_one = temp_dir("catalog_device_one");
    clone_project(&source, &device_one, "shared-project");
    sync_workspace_catalog_from_projects_at(&device_one, &[]).unwrap();
    initialize_catalog_git_sync_at(&device_one, Some(catalog_remote.display().to_string()))
        .unwrap();
    git(&device_one, &["config", "user.email", "test@example.com"]);
    git(&device_one, &["config", "user.name", "Test User"]);
    let published = publish_catalog_git_sync_at(&device_one).unwrap();
    assert_eq!(published.status, TaskStatus::Succeeded);

    let device_two = temp_dir("catalog_device_two");
    initialize_catalog_git_sync_at(&device_two, Some(catalog_remote.display().to_string()))
        .unwrap();
    let pulled = pull_catalog_git_sync_at(&device_two).unwrap();
    assert_eq!(pulled.status, TaskStatus::Succeeded);

    let summary = load_workspace_catalog_summary_at(&device_two, &[]).unwrap();
    assert_eq!(summary.active_count, 1);
    assert_eq!(summary.missing_count, 1);

    let restored = restore_missing_catalog_repositories_at(&device_two, &[]).unwrap();
    assert_eq!(restored.task.status, TaskStatus::Succeeded);
    assert!(device_two.join("shared-project/.git").exists());
}

#[test]
fn restore_missing_catalog_repositories_command_queues_long_clone_work() {
    let source = init_source_repo("restore_background_source");
    let old_workspace = temp_dir("restore_background_old_workspace");
    clone_project(&source, &old_workspace, "background-project");
    sync_workspace_catalog_from_projects_at(&old_workspace, &[]).unwrap();

    let new_workspace = temp_dir("restore_background_new_workspace");
    copy_dir_all(
        &old_workspace.join(".skilldock"),
        &new_workspace.join(".skilldock"),
    );

    let result = restore_missing_catalog_repositories_background_at(&new_workspace, &[]).unwrap();

    assert_eq!(result.task.status, TaskStatus::Queued);
    assert_eq!(result.workspace.projects.len(), 0);
    for _ in 0..50 {
        let status = task_queue().get_task_status(&result.task.id).unwrap();
        if matches!(
            status.status,
            TaskStatus::Succeeded
                | TaskStatus::Skipped
                | TaskStatus::Failed
                | TaskStatus::Cancelled
        ) {
            assert_eq!(status.status, TaskStatus::Succeeded);
            assert!(new_workspace.join("background-project/.git").exists());
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("background catalog restore did not finish");
}
