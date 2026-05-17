use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use skilldock_lib::{
    read_skill_markdown_preview_at, resolve_workspace_path_at, save_workspace_config,
    scan_workspace_at, AgentDirectoryEntryKind, AgentProfile, InstalledAgentSkillStatus, LinkMode,
    ProjectCategory, WorkspaceConfig, WorkspaceProjectMetadata, SKILL_MARKDOWN_PREVIEW_MAX_BYTES,
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
fn read_skill_markdown_preview_is_bounded_and_workspace_scoped() {
    let workspace_root = temp_dir("skill_preview");
    let skill_dir = workspace_root.join("repo-one").join("skill-a");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "abcdef").unwrap();

    let preview = read_skill_markdown_preview_at(&workspace_root, "repo-one/skill-a", 4).unwrap();
    assert_eq!(preview.skill_id, "repo-one/skill-a");
    assert_eq!(preview.markdown, "abcd");
    assert!(preview.truncated);

    let outside = read_skill_markdown_preview_at(&workspace_root, "../skill-a", 4).unwrap_err();
    assert_eq!(
        outside.kind,
        skilldock_lib::WorkspaceErrorKind::OutsideWorkspace
    );
}

#[test]
fn read_skill_markdown_preview_clamps_oversized_requests_and_rejects_absolute_paths() {
    let workspace_root = temp_dir("skill_preview_limits");
    let skill_dir = workspace_root.join("repo-one").join("skill-a");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "a".repeat(SKILL_MARKDOWN_PREVIEW_MAX_BYTES + 8),
    )
    .unwrap();

    let preview =
        read_skill_markdown_preview_at(&workspace_root, "repo-one/skill-a", usize::MAX).unwrap();
    assert_eq!(preview.markdown.len(), SKILL_MARKDOWN_PREVIEW_MAX_BYTES);
    assert!(preview.truncated);

    let absolute = read_skill_markdown_preview_at(&workspace_root, skill_dir.to_str().unwrap(), 4)
        .unwrap_err();
    assert_eq!(
        absolute.kind,
        skilldock_lib::WorkspaceErrorKind::OutsideWorkspace
    );
}

#[cfg(unix)]
#[test]
fn read_skill_markdown_preview_rejects_symlink_escape() {
    let workspace_root = temp_dir("skill_preview_symlink");
    let external_dir = temp_dir("skill_preview_external");
    let external_skill = external_dir.join("skill-a");
    std::fs::create_dir_all(&external_skill).unwrap();
    std::fs::write(external_skill.join("SKILL.md"), "outside").unwrap();
    std::fs::create_dir_all(workspace_root.join("repo-one")).unwrap();
    std::os::unix::fs::symlink(
        &external_skill,
        workspace_root.join("repo-one").join("skill-a"),
    )
    .unwrap();

    let error = read_skill_markdown_preview_at(&workspace_root, "repo-one/skill-a", 4).unwrap_err();
    assert_eq!(
        error.kind,
        skilldock_lib::WorkspaceErrorKind::OutsideWorkspace
    );
}

#[test]
fn resolve_workspace_path_accepts_relative_and_absolute_paths_inside_workspace() {
    let workspace_root = temp_dir("open_path_inside");
    let project_dir = workspace_root.join("repo-one");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("README.md"), "readme").unwrap();

    let relative = resolve_workspace_path_at(&workspace_root, "repo-one/README.md").unwrap();
    assert_eq!(
        relative,
        std::fs::canonicalize(project_dir.join("README.md")).unwrap()
    );

    let absolute = resolve_workspace_path_at(
        &workspace_root,
        project_dir.join("README.md").to_str().unwrap(),
    )
    .unwrap();
    assert_eq!(absolute, relative);
}

#[test]
fn resolve_workspace_path_rejects_parent_escape() {
    let workspace_root = temp_dir("open_path_parent_escape");
    let outside_dir = temp_dir("open_path_external");
    std::fs::write(outside_dir.join("README.md"), "outside").unwrap();
    let requested = format!(
        "../{}/README.md",
        outside_dir.file_name().unwrap().to_string_lossy()
    );

    let error = resolve_workspace_path_at(&workspace_root, &requested).unwrap_err();
    assert_eq!(
        error.kind,
        skilldock_lib::WorkspaceErrorKind::OutsideWorkspace
    );
}

#[cfg(unix)]
#[test]
fn resolve_workspace_path_rejects_symlink_escape() {
    let workspace_root = temp_dir("open_path_symlink");
    let outside_dir = temp_dir("open_path_symlink_external");
    std::fs::write(outside_dir.join("README.md"), "outside").unwrap();
    std::os::unix::fs::symlink(&outside_dir, workspace_root.join("outside-link")).unwrap();

    let error = resolve_workspace_path_at(&workspace_root, "outside-link/README.md").unwrap_err();
    assert_eq!(
        error.kind,
        skilldock_lib::WorkspaceErrorKind::OutsideWorkspace
    );
}

#[cfg(unix)]
#[test]
fn scan_workspace_maps_agent_symlinks_to_installed_skills_and_reports_conflicts() {
    let workspace_root = temp_dir("scan_agents");
    let skill_dir = workspace_root.join("repo-one").join("skill-a");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: Skill A\ndescription: A skill.\n---\n",
    )
    .unwrap();
    let workspace_non_skill = workspace_root.join("repo-one").join("not-a-skill");
    std::fs::create_dir_all(&workspace_non_skill).unwrap();

    let external_dir = temp_dir("external_skill");
    let profile_dir = temp_dir("agent_profile");
    std::os::unix::fs::symlink(&skill_dir, profile_dir.join("valid-link")).unwrap();
    std::os::unix::fs::symlink(
        profile_dir.join("missing-target"),
        profile_dir.join("broken-link"),
    )
    .unwrap();
    std::os::unix::fs::symlink(&external_dir, profile_dir.join("external-link")).unwrap();
    std::os::unix::fs::symlink(&workspace_non_skill, profile_dir.join("conflict-link")).unwrap();
    std::fs::write(profile_dir.join("ordinary-file"), "not removable\n").unwrap();

    let profiles = vec![
        AgentProfile {
            id: "codex".to_string(),
            name: "Codex".to_string(),
            skills_dir: profile_dir.display().to_string(),
            enabled: true,
            built_in: true,
            link_mode: LinkMode::Symlink,
        },
        AgentProfile {
            id: "missing".to_string(),
            name: "Missing".to_string(),
            skills_dir: profile_dir.join("missing").display().to_string(),
            enabled: true,
            built_in: false,
            link_mode: LinkMode::Symlink,
        },
    ];

    let workspace = scan_workspace_at(&workspace_root, &profiles).unwrap();

    let skill = workspace
        .skills
        .iter()
        .find(|skill| skill.relative_path == "repo-one/skill-a")
        .unwrap();
    assert_eq!(skill.installed_agents.len(), 1);
    assert_eq!(skill.installed_agents[0].agent_profile_id, "codex");
    assert_eq!(skill.installed_agents[0].link_name, "valid-link");
    assert_eq!(
        skill.installed_agents[0].status,
        InstalledAgentSkillStatus::Valid
    );

    let state = workspace
        .agent_profiles
        .iter()
        .find(|state| state.profile.id == "codex")
        .unwrap();
    assert!(state.exists);
    assert!(state.writable);
    assert_eq!(state.symlink_count, 4);
    assert_eq!(state.workspace_link_count, 1);

    let statuses = state
        .entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.status.clone(), entry.removable))
        .collect::<Vec<_>>();
    assert!(statuses.contains(&("valid-link", InstalledAgentSkillStatus::Valid, true)));
    assert!(statuses.contains(&("broken-link", InstalledAgentSkillStatus::Broken, false)));
    assert!(statuses.contains(&("external-link", InstalledAgentSkillStatus::External, false)));
    assert!(statuses.contains(&("conflict-link", InstalledAgentSkillStatus::Conflict, false)));

    let ordinary_file = state
        .entries
        .iter()
        .find(|entry| entry.name == "ordinary-file")
        .unwrap();
    assert_eq!(ordinary_file.kind, AgentDirectoryEntryKind::File);
    assert_eq!(ordinary_file.status, InstalledAgentSkillStatus::Conflict);
    assert!(!ordinary_file.removable);

    let missing = workspace
        .agent_profiles
        .iter()
        .find(|state| state.profile.id == "missing")
        .unwrap();
    assert!(!missing.exists);
    assert!(!missing.writable);
}

#[test]
fn scan_workspace_profiles_probe_writable_directories_without_leaving_temp_files() {
    let workspace_root = temp_dir("scan_writable_probe_cleanup");
    let profile_dir = temp_dir("agent_profile_writable_probe");
    let profiles = vec![AgentProfile {
        id: "probe".to_string(),
        name: "Probe".to_string(),
        skills_dir: profile_dir.display().to_string(),
        enabled: true,
        built_in: false,
        link_mode: LinkMode::Symlink,
    }];

    let workspace = scan_workspace_at(&workspace_root, &profiles).unwrap();

    let state = workspace
        .agent_profiles
        .iter()
        .find(|state| state.profile.id == "probe")
        .unwrap();
    assert!(state.exists);
    assert!(state.writable);
    assert!(std::fs::read_dir(&profile_dir).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".skilldock-writable-probe-")
    }));
}

#[cfg(unix)]
#[test]
fn scan_workspace_reports_existing_profile_directory_not_writable_when_probe_cannot_create_file() {
    use std::os::unix::fs::PermissionsExt;

    let workspace_root = temp_dir("scan_not_writable_probe");
    let profile_dir = temp_dir("agent_profile_not_writable_probe");
    let original_permissions = std::fs::metadata(&profile_dir).unwrap().permissions();
    let mut readonly_permissions = original_permissions.clone();
    readonly_permissions.set_mode(0o555);
    std::fs::set_permissions(&profile_dir, readonly_permissions).unwrap();

    let manual_probe = profile_dir.join("manual-write-probe");
    if std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manual_probe)
        .is_ok()
    {
        let _ = std::fs::remove_file(&manual_probe);
        std::fs::set_permissions(&profile_dir, original_permissions).unwrap();
        return;
    }

    let profiles = vec![AgentProfile {
        id: "not-writable".to_string(),
        name: "Not Writable".to_string(),
        skills_dir: profile_dir.display().to_string(),
        enabled: true,
        built_in: false,
        link_mode: LinkMode::Symlink,
    }];

    let workspace = scan_workspace_at(&workspace_root, &profiles).unwrap();
    std::fs::set_permissions(&profile_dir, original_permissions).unwrap();

    let state = workspace
        .agent_profiles
        .iter()
        .find(|state| state.profile.id == "not-writable")
        .unwrap();
    assert!(state.exists);
    assert!(!state.writable);
}

#[test]
fn scan_workspace_finds_skills_with_frontmatter_fallbacks_and_resource_flags() {
    let workspace_root = temp_dir("scan_skills");
    let skill_dir = workspace_root.join("repo-one").join("skills").join("tdd");
    std::fs::create_dir_all(skill_dir.join("assets")).unwrap();
    std::fs::create_dir_all(skill_dir.join("scripts")).unwrap();
    std::fs::create_dir_all(skill_dir.join("references")).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: Test Driven Development\ndescription: Build behavior through tests.\n---\n\n# Body\n",
    )
    .unwrap();

    let fallback_dir = workspace_root.join("repo-two").join("fallback-skill");
    std::fs::create_dir_all(&fallback_dir).unwrap();
    std::fs::write(
        fallback_dir.join("SKILL.md"),
        "# Fallback Heading\n\nFallback paragraph.\n",
    )
    .unwrap();

    let ignored_git_dir = workspace_root.join("repo-one").join(".git").join("ignored");
    std::fs::create_dir_all(&ignored_git_dir).unwrap();
    std::fs::write(ignored_git_dir.join("SKILL.md"), "# Ignored\n").unwrap();
    let ignored_node_dir = workspace_root
        .join("repo-one")
        .join("node_modules")
        .join("ignored");
    std::fs::create_dir_all(&ignored_node_dir).unwrap();
    std::fs::write(ignored_node_dir.join("SKILL.md"), "# Ignored\n").unwrap();
    let ignored_staging_dir = workspace_root
        .join(".skilldock")
        .join("imports")
        .join("staged-repo")
        .join("skills")
        .join("ignored");
    std::fs::create_dir_all(&ignored_staging_dir).unwrap();
    std::fs::write(ignored_staging_dir.join("SKILL.md"), "# Ignored\n").unwrap();

    let workspace = scan_workspace_at(&workspace_root, &[]).unwrap();

    assert_eq!(workspace.skills.len(), 2);
    let parsed = workspace
        .skills
        .iter()
        .find(|skill| skill.relative_path == "repo-one/skills/tdd")
        .unwrap();
    assert_eq!(parsed.name, "Test Driven Development");
    assert_eq!(
        parsed.description.as_deref(),
        Some("Build behavior through tests.")
    );
    assert_eq!(parsed.source_project_id, "repo-one");
    assert_eq!(parsed.default_link_name, "repo-one-test-driven-development");
    assert!(parsed.has_assets);
    assert!(parsed.has_scripts);
    assert!(parsed.has_references);
    assert!(parsed.last_modified.is_some());

    let fallback = workspace
        .skills
        .iter()
        .find(|skill| skill.relative_path == "repo-two/fallback-skill")
        .unwrap();
    assert_eq!(fallback.name, "fallback-skill");
    assert_eq!(fallback.description.as_deref(), Some("Fallback Heading"));
    assert_eq!(fallback.default_link_name, "repo-two-fallback-skill");
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

fn init_git_project(workspace: &Path, name: &str) -> PathBuf {
    let project = workspace.join(name);
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "Test User"]);
    std::fs::write(
        project.join("README.md"),
        "# Example\n\nA collected project.\n",
    )
    .unwrap();
    std::fs::write(project.join("LICENSE"), "MIT\n").unwrap();
    std::fs::write(project.join("notes.txt"), "dirty\n").unwrap();
    git(&project, &["add", "README.md", "LICENSE"]);
    git(&project, &["commit", "-m", "initial"]);
    git(
        &project,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/project.git",
        ],
    );
    project
}

fn init_clean_local_project(workspace: &Path, name: &str) -> PathBuf {
    let project = workspace.join(name);
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "Test User"]);
    std::fs::write(project.join("README.md"), "# Local\n").unwrap();
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-m", "initial"]);
    project
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

fn commit_file(project: &Path, file_name: &str, contents: &str, message: &str) {
    std::fs::write(project.join(file_name), contents).unwrap();
    git(project, &["add", file_name]);
    git(project, &["commit", "-m", message]);
}

fn commit_and_push(seed: &Path, file_name: &str, contents: &str) {
    commit_file(seed, file_name, contents, "update");
    git(seed, &["push"]);
}

#[test]
fn scan_workspace_discovers_top_level_git_projects_with_local_status_and_metadata() {
    let workspace_root = temp_dir("scan_projects");
    let project = init_git_project(&workspace_root, "project-one");
    std::fs::create_dir_all(workspace_root.join("plain-directory")).unwrap();

    save_workspace_config(
        &workspace_root,
        &WorkspaceConfig {
            schema_version: 1,
            projects: vec![WorkspaceProjectMetadata {
                project_id: "project-one".to_string(),
                display_name: Some("Project One".to_string()),
                category: Some(ProjectCategory::Tools),
                favorite: true,
                hidden: true,
                tags: vec!["featured".to_string()],
                notes: Some("Pinned".to_string()),
                auto_check: None,
                auto_pull: None,
            }],
        },
    )
    .unwrap();

    let workspace = scan_workspace_at(&workspace_root, &[]).unwrap();

    assert_eq!(workspace.projects.len(), 1);
    let scanned = &workspace.projects[0];
    assert_eq!(scanned.id, "project-one");
    assert_eq!(scanned.name, "Project One");
    assert_eq!(
        scanned.path,
        project.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(
        scanned.remote_url.as_deref(),
        Some("https://github.com/example/project.git")
    );
    assert_eq!(
        scanned.provider,
        skilldock_lib::GitProvider::Github
    );
    assert!(scanned.branch.is_some());
    assert_eq!(scanned.upstream, None);
    assert_eq!(
        scanned.git_status,
        skilldock_lib::GitStatus::Dirty
    );
    assert_eq!(scanned.category, ProjectCategory::Tools);
    assert_eq!(scanned.readme_file.as_deref(), Some("README.md"));
    assert_eq!(scanned.license_file.as_deref(), Some("LICENSE"));
    assert_eq!(scanned.tags, vec!["featured"]);
    assert_eq!(scanned.notes.as_deref(), Some("Pinned"));
    assert!(scanned.favorite);
    assert!(scanned.hidden);
}

#[test]
fn scan_workspace_maps_git_status_fixtures_without_network_dependencies() {
    let workspace_root = temp_dir("scan_git_status_matrix");

    let no_upstream = init_clean_local_project(&workspace_root, "no-upstream");
    let dirty = init_clean_local_project(&workspace_root, "dirty");
    std::fs::write(dirty.join("local.txt"), "uncommitted\n").unwrap();
    let detached = init_clean_local_project(&workspace_root, "detached");
    git(&detached, &["checkout", "--detach", "HEAD"]);

    let (behind_remote, behind_seed) = init_bare_remote_with_seed("scan_status_behind");
    let behind = clone_project(&behind_remote, &workspace_root, "behind");
    commit_and_push(&behind_seed, "CHANGELOG.md", "remote change\n");
    git(&behind, &["fetch", "--prune"]);

    let (ahead_remote, _ahead_seed) = init_bare_remote_with_seed("scan_status_ahead");
    let ahead = clone_project(&ahead_remote, &workspace_root, "ahead");
    git(&ahead, &["config", "user.email", "test@example.com"]);
    git(&ahead, &["config", "user.name", "Test User"]);
    commit_file(&ahead, "local-only.txt", "local change\n", "local only");

    let workspace = scan_workspace_at(&workspace_root, &[]).unwrap();
    let project = |id: &str| {
        workspace
            .projects
            .iter()
            .find(|project| project.id == id)
            .unwrap()
    };

    assert_eq!(
        project("no-upstream").path,
        no_upstream.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(
        project("no-upstream").git_status,
        skilldock_lib::GitStatus::NoUpstream
    );
    assert_eq!(project("no-upstream").ahead_count, 0);
    assert_eq!(project("no-upstream").behind_count, 0);

    assert_eq!(
        project("dirty").git_status,
        skilldock_lib::GitStatus::Dirty
    );
    assert_eq!(project("dirty").ahead_count, 0);
    assert_eq!(project("dirty").behind_count, 0);

    assert_eq!(
        project("detached").git_status,
        skilldock_lib::GitStatus::Detached
    );
    assert_eq!(project("detached").branch, None);

    assert_eq!(
        project("behind").git_status,
        skilldock_lib::GitStatus::Behind
    );
    assert_eq!(project("behind").ahead_count, 0);
    assert_eq!(project("behind").behind_count, 1);
    assert!(project("behind").pull_all_eligible);

    assert_eq!(
        project("ahead").git_status,
        skilldock_lib::GitStatus::Ahead
    );
    assert_eq!(project("ahead").ahead_count, 1);
    assert_eq!(project("ahead").behind_count, 0);
    assert!(!project("ahead").pull_all_eligible);
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn spawn_first_available_opener_falls_back_past_missing_programs() {
    // A program name that is essentially guaranteed not to exist on PATH,
    // followed by `true`, a no-op binary that is universally available on
    // POSIX systems. The fallback chain must skip the missing program and
    // spawn `true` successfully.
    let path = temp_dir("opener_fallback_ok");
    let candidates: &[(&str, &[&str])] = &[
        ("skilldock-definitely-not-a-real-opener", &[]),
        ("true", &[]),
    ];
    skilldock_lib::spawn_first_available_opener(&path, candidates).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn spawn_first_available_opener_falls_back_past_nonzero_exit() {
    // Simulates the real-world symptom where `xdg-open` spawns cleanly but
    // then exits non-zero because no desktop environment / handler is
    // registered. The fallback must treat that as a failure and try the
    // next candidate instead of reporting false success.
    let path = temp_dir("opener_nonzero_fallback");
    let candidates: &[(&str, &[&str])] = &[("false", &[]), ("true", &[])];
    skilldock_lib::spawn_first_available_opener(&path, candidates).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn spawn_first_available_opener_reports_last_error_when_all_fail() {
    let path = temp_dir("opener_fallback_err");
    let candidates: &[(&str, &[&str])] = &[
        ("skilldock-missing-opener-alpha", &[]),
        ("skilldock-missing-opener-beta", &[]),
    ];
    let error = skilldock_lib::spawn_first_available_opener(&path, candidates).unwrap_err();
    assert_eq!(error.kind, skilldock_lib::WorkspaceErrorKind::Io);
    // The aggregated error message should mention each attempted program
    // so users can diagnose a broken desktop environment.
    assert!(
        error.message.contains("skilldock-missing-opener-alpha"),
        "error message should list first candidate, got: {}",
        error.message
    );
    assert!(
        error.message.contains("skilldock-missing-opener-beta"),
        "error message should list last candidate, got: {}",
        error.message
    );
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn spawn_first_available_opener_aggregates_nonzero_exit_in_error() {
    // When every candidate runs but each exits non-zero (as `xdg-open`
    // does on a headless system), the aggregated error should still name
    // every program that was tried so the user knows the full story.
    let path = temp_dir("opener_all_nonzero");
    let candidates: &[(&str, &[&str])] = &[("false", &[]), ("false", &["--unused"])];
    let error = skilldock_lib::spawn_first_available_opener(&path, candidates).unwrap_err();
    assert_eq!(error.kind, skilldock_lib::WorkspaceErrorKind::Io);
    assert!(
        error.message.contains("false"),
        "error should name the failing program, got: {}",
        error.message
    );
    // A non-zero exit must not be mis-reported as "failed to spawn".
    assert!(
        !error.message.contains("failed to spawn"),
        "non-zero exit should not be phrased as a spawn failure, got: {}",
        error.message
    );
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn linux_path_openers_list_includes_xdg_open_as_primary_and_gui_fallbacks() {
    // Lock in the prioritisation so a future refactor doesn't accidentally
    // demote the XDG-standard entry point below desktop-specific ones, and
    // so the direct file-manager tier (needed on minimal installs where no
    // inode/directory MIME handler is registered) is preserved.
    let primary = skilldock_lib::LINUX_PATH_OPENERS
        .first()
        .expect("LINUX_PATH_OPENERS should not be empty");
    assert_eq!(primary.0, "xdg-open");

    let names: Vec<&str> = skilldock_lib::LINUX_PATH_OPENERS
        .iter()
        .map(|(program, _)| *program)
        .collect();

    // Tier 1: XDG / GLib standard entry points.
    for required in ["xdg-open", "gio"] {
        assert!(
            names.contains(&required),
            "expected {required} in fallback chain, got {names:?}"
        );
    }
    // Tier 3: direct file-manager binaries covering the major desktops.
    for required in ["nautilus", "thunar", "dolphin"] {
        assert!(
            names.contains(&required),
            "expected {required} in fallback chain, got {names:?}"
        );
    }

    // xdg-open and gio must come before the direct file-manager tier so a
    // correctly-registered handler is preferred over an arbitrarily-picked
    // file manager when both are available.
    let xdg_pos = names.iter().position(|n| *n == "xdg-open").unwrap();
    let gio_pos = names.iter().position(|n| *n == "gio").unwrap();
    let nautilus_pos = names.iter().position(|n| *n == "nautilus").unwrap();
    assert!(
        xdg_pos < nautilus_pos && gio_pos < nautilus_pos,
        "xdg-open/gio must precede direct file managers, got {names:?}"
    );
}
