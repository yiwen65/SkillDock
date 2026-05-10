use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use skills_collection_app_lib::{
    check_all_project_updates_at, check_project_updates_at, import_project_at, plan_import_project,
    pull_all_projects_at, pull_project_at, AgentProfile, GitOperationErrorKind, GitStatus,
    ImportProjectRequest, PullAllProjectsRequest, PullProjectRequest, TaskStatus,
};

fn temp_dir(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "skills_collection_app_git_{name}_{}_{}",
        std::process::id(),
        unique
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

fn init_bare_remote_with_seed(name: &str) -> (PathBuf, PathBuf) {
    let remote = temp_dir(&format!("{name}_remote"));
    git(&remote, &["init", "--bare"]);
    let seed = temp_dir(&format!("{name}_seed"));
    git(&seed, &["init"]);
    git(&seed, &["config", "user.email", "test@example.com"]);
    git(&seed, &["config", "user.name", "Test User"]);
    std::fs::write(seed.join("README.md"), "# Remote\n").unwrap();
    git(&seed, &["add", "README.md"]);
    git(&seed, &["commit", "-m", "initial"]);
    git(
        &seed,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&seed, &["push", "-u", "origin", "HEAD"]);
    (remote, seed)
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

fn commit_and_push(seed: &Path, file_name: &str, contents: &str) {
    std::fs::write(seed.join(file_name), contents).unwrap();
    git(seed, &["add", file_name]);
    git(seed, &["commit", "-m", "update"]);
    git(seed, &["push"]);
}

#[test]
fn import_plan_normalizes_github_shorthand_and_rejects_unsafe_directory_names() {
    let workspace = temp_dir("import_plan");
    let plan = plan_import_project(
        &workspace,
        &ImportProjectRequest {
            source: "owner/repo".to_string(),
            directory_name: None,
            shallow: false,
        },
    )
    .unwrap();

    assert_eq!(plan.remote_url, "https://github.com/owner/repo.git");
    assert_eq!(plan.directory_name, "repo");
    assert_eq!(
        plan.target_path,
        workspace.join("repo").display().to_string()
    );

    for unsafe_name in ["../repo", "nested/repo", ".git", "", "repo\u{0}name"] {
        let error = plan_import_project(
            &workspace,
            &ImportProjectRequest {
                source: "owner/repo".to_string(),
                directory_name: Some(unsafe_name.to_string()),
                shallow: false,
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, GitOperationErrorKind::InvalidDirectoryName);
    }

    for source in [
        "https://example.com/team/repo.git",
        "ssh://git@example.com/team/repo.git",
        "git@example.com:team/repo.git",
        "file:///tmp/repo.git",
    ] {
        let plan = plan_import_project(
            &workspace,
            &ImportProjectRequest {
                source: source.to_string(),
                directory_name: Some("safe-name".to_string()),
                shallow: false,
            },
        )
        .unwrap();
        assert_eq!(plan.remote_url, source);
    }
}

#[test]
fn import_project_clones_local_repo_and_returns_refreshed_workspace() {
    let workspace = temp_dir("import_clone");
    let source = init_source_repo("source_repo");

    let result = import_project_at(
        &workspace,
        &[],
        ImportProjectRequest {
            source: source.display().to_string(),
            directory_name: Some("imported".to_string()),
            shallow: true,
        },
    )
    .unwrap();

    assert_eq!(result.task.status, TaskStatus::Succeeded);
    assert!(result.task.stdout.contains("git clone"));
    assert!(workspace.join("imported").join(".git").exists());
    assert!(result
        .workspace
        .projects
        .iter()
        .any(|project| project.id == "imported"));
}

#[test]
fn import_project_adopts_existing_git_directory_and_blocks_plain_directory() {
    let workspace = temp_dir("import_existing");
    let existing_git = workspace.join("existing");
    std::fs::create_dir_all(&existing_git).unwrap();
    git(&existing_git, &["init"]);

    let adopted = import_project_at(
        &workspace,
        &Vec::<AgentProfile>::new(),
        ImportProjectRequest {
            source: "https://example.com/existing.git".to_string(),
            directory_name: Some("existing".to_string()),
            shallow: false,
        },
    )
    .unwrap();
    assert_eq!(adopted.task.status, TaskStatus::Succeeded);
    assert!(adopted.task.stdout.contains("adopt"));

    std::fs::create_dir_all(workspace.join("plain")).unwrap();
    let blocked = import_project_at(
        &workspace,
        &[],
        ImportProjectRequest {
            source: "https://example.com/plain.git".to_string(),
            directory_name: Some("plain".to_string()),
            shallow: false,
        },
    )
    .unwrap();
    assert_eq!(blocked.task.status, TaskStatus::Failed);
    assert!(blocked.task.error.unwrap().contains("non-Git directory"));
}

#[test]
fn check_project_updates_fetches_prune_and_marks_project_behind() {
    let workspace = temp_dir("check_updates");
    let (remote, seed) = init_bare_remote_with_seed("check_updates");
    clone_project(&remote, &workspace, "project-one");
    commit_and_push(&seed, "CHANGELOG.md", "new release\n");

    let result = check_project_updates_at(&workspace, &[], "project-one").unwrap();

    assert_eq!(result.task.status, TaskStatus::Succeeded);
    assert!(result.task.stdout.contains("fetch --prune"));
    let project = result
        .workspace
        .projects
        .iter()
        .find(|project| project.id == "project-one")
        .unwrap();
    assert_eq!(project.git_status, GitStatus::Behind);
    assert_eq!(project.ahead_count, 0);
    assert_eq!(project.behind_count, 1);
}

#[test]
fn check_project_updates_keeps_dirty_status_with_divergence_counts() {
    let workspace = temp_dir("check_dirty_behind");
    let (remote, seed) = init_bare_remote_with_seed("check_dirty_behind");
    let project_path = clone_project(&remote, &workspace, "project-one");
    commit_and_push(&seed, "CHANGELOG.md", "new release\n");
    std::fs::write(project_path.join("local.txt"), "local changes\n").unwrap();

    let result = check_project_updates_at(&workspace, &[], "project-one").unwrap();

    assert_eq!(result.task.status, TaskStatus::Succeeded);
    let project = result
        .workspace
        .projects
        .iter()
        .find(|project| project.id == "project-one")
        .unwrap();
    assert_eq!(project.git_status, GitStatus::Dirty);
    assert_eq!(project.ahead_count, 0);
    assert_eq!(project.behind_count, 1);
}

#[test]
fn check_all_project_updates_summarizes_success_skips_and_failures() {
    let workspace = temp_dir("check_all_updates");
    let (remote, _seed) = init_bare_remote_with_seed("check_all_updates");
    clone_project(&remote, &workspace, "ok-project");

    let no_upstream = workspace.join("no-upstream");
    std::fs::create_dir_all(&no_upstream).unwrap();
    git(&no_upstream, &["init"]);
    git(&no_upstream, &["config", "user.email", "test@example.com"]);
    git(&no_upstream, &["config", "user.name", "Test User"]);
    std::fs::write(no_upstream.join("README.md"), "# Local\n").unwrap();
    git(&no_upstream, &["add", "README.md"]);
    git(&no_upstream, &["commit", "-m", "initial"]);

    let failing = clone_project(&remote, &workspace, "failing-project");
    git(
        &failing,
        &[
            "remote",
            "set-url",
            "origin",
            "/tmp/does-not-exist-for-fetch",
        ],
    );

    let result = check_all_project_updates_at(&workspace, &[]).unwrap();

    assert_eq!(result.task.status, TaskStatus::Failed);
    assert!(result
        .task
        .stdout
        .contains("summary: ok=1 skipped=1 failed=1"));
    let failing_project = result
        .workspace
        .projects
        .iter()
        .find(|project| project.id == "failing-project")
        .unwrap();
    assert_eq!(failing_project.git_status, GitStatus::FetchFailed);
    assert!(!failing_project.pull_all_eligible);
    let failing_outcome = result
        .task
        .project_outcomes
        .iter()
        .find(|outcome| outcome.project_id == "failing-project")
        .unwrap();
    assert_eq!(failing_outcome.status, TaskStatus::Failed);
    assert!(failing_outcome.error.is_some());
}

#[test]
fn pull_project_skips_dirty_worktree_by_default() {
    let workspace = temp_dir("pull_dirty");
    let (remote, _seed) = init_bare_remote_with_seed("pull_dirty");
    let project = clone_project(&remote, &workspace, "project-one");
    std::fs::write(project.join("local.txt"), "local changes\n").unwrap();

    let result = pull_project_at(
        &workspace,
        &[],
        PullProjectRequest {
            project_id: "project-one".to_string(),
            autostash: false,
        },
    )
    .unwrap();

    assert_eq!(result.task.status, TaskStatus::Skipped);
    assert!(result.task.stdout.contains("local changes"));
    assert!(!result.task.stdout.contains("--autostash"));
}

#[test]
fn pull_project_uses_fast_forward_prune_and_refreshes_workspace() {
    let workspace = temp_dir("pull_fast_forward");
    let (remote, seed) = init_bare_remote_with_seed("pull_fast_forward");
    let project = clone_project(&remote, &workspace, "project-one");
    commit_and_push(&seed, "CHANGELOG.md", "new release\n");

    let result = pull_project_at(
        &workspace,
        &[],
        PullProjectRequest {
            project_id: "project-one".to_string(),
            autostash: false,
        },
    )
    .unwrap();

    assert_eq!(result.task.status, TaskStatus::Succeeded);
    assert!(result.task.stdout.contains("pull --ff-only --prune"));
    assert!(project.join("CHANGELOG.md").exists());
    let scanned = result
        .workspace
        .projects
        .iter()
        .find(|project| project.id == "project-one")
        .unwrap();
    assert_eq!(scanned.git_status, GitStatus::UpToDate);
}

#[test]
fn pull_all_projects_continues_after_skipped_projects_and_autostash_is_explicit() {
    let workspace = temp_dir("pull_all");
    let (remote, seed) = init_bare_remote_with_seed("pull_all");
    clone_project(&remote, &workspace, "clean-project");
    let dirty = clone_project(&remote, &workspace, "dirty-project");
    std::fs::write(dirty.join("local.txt"), "local changes\n").unwrap();
    commit_and_push(&seed, "CHANGELOG.md", "new release\n");

    let skipped = pull_all_projects_at(
        &workspace,
        &[],
        PullAllProjectsRequest {
            autostash: false,
            safe_project_ids: None,
        },
    )
    .unwrap();

    assert_eq!(skipped.task.status, TaskStatus::Succeeded);
    assert!(skipped
        .task
        .stdout
        .contains("summary: ok=1 skipped=1 failed=0"));

    let autostashed = pull_project_at(
        &workspace,
        &[],
        PullProjectRequest {
            project_id: "dirty-project".to_string(),
            autostash: true,
        },
    )
    .unwrap();
    assert!(autostashed.task.stdout.contains("--autostash"));
}

#[test]
fn pull_all_projects_only_attempts_clean_safe_projects() {
    let workspace = temp_dir("pull_all_safe");
    let (behind_remote, behind_seed) = init_bare_remote_with_seed("pull_all_safe_behind");
    let behind = clone_project(&behind_remote, &workspace, "behind-project");

    let (ahead_remote, _ahead_seed) = init_bare_remote_with_seed("pull_all_safe_ahead");
    let ahead = clone_project(&ahead_remote, &workspace, "ahead-project");
    git(&ahead, &["config", "user.email", "test@example.com"]);
    git(&ahead, &["config", "user.name", "Test User"]);
    std::fs::write(ahead.join("local-only.txt"), "local commit\n").unwrap();
    git(&ahead, &["add", "local-only.txt"]);
    git(&ahead, &["commit", "-m", "local only"]);

    commit_and_push(&behind_seed, "CHANGELOG.md", "new release\n");
    let checked = check_all_project_updates_at(&workspace, &[]).unwrap();
    assert_eq!(checked.task.status, TaskStatus::Succeeded);

    let result = pull_all_projects_at(
        &workspace,
        &[],
        PullAllProjectsRequest {
            autostash: false,
            safe_project_ids: None,
        },
    )
    .unwrap();

    assert_eq!(result.task.status, TaskStatus::Succeeded);
    assert!(result
        .task
        .stdout
        .contains("summary: ok=1 skipped=1 failed=0"));
    assert!(result.task.stdout.contains("skip: ahead-project is ahead"));
    assert!(behind.join("CHANGELOG.md").exists());
}

#[test]
fn pull_all_projects_respects_current_ui_safe_project_ids() {
    let workspace = temp_dir("pull_all_ui_safe_ids");
    let (included_remote, included_seed) = init_bare_remote_with_seed("pull_all_ui_safe_included");
    let included = clone_project(&included_remote, &workspace, "included-project");
    let (excluded_remote, excluded_seed) = init_bare_remote_with_seed("pull_all_ui_safe_excluded");
    let excluded = clone_project(&excluded_remote, &workspace, "excluded-project");

    commit_and_push(&included_seed, "CHANGELOG.md", "included update\n");
    commit_and_push(&excluded_seed, "CHANGELOG.md", "excluded update\n");
    check_all_project_updates_at(&workspace, &[]).unwrap();

    let result = pull_all_projects_at(
        &workspace,
        &[],
        PullAllProjectsRequest {
            autostash: false,
            safe_project_ids: Some(vec!["included-project".to_string()]),
        },
    )
    .unwrap();

    assert_eq!(result.task.status, TaskStatus::Succeeded);
    assert!(result
        .task
        .stdout
        .contains("summary: ok=1 skipped=1 failed=0"));
    assert!(result
        .task
        .stdout
        .contains("skip: excluded-project was not marked safe"));
    assert!(included.join("CHANGELOG.md").exists());
    assert!(!excluded.join("CHANGELOG.md").exists());
}
