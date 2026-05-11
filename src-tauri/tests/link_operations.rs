use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use skilldock_lib::{
    link_skill_at, preview_link_skill_at, preview_link_skills_batch_at, preview_unlink_skill_at,
    preview_unlink_skills_batch_at, unlink_skill_at, unlink_skills_batch_at, AgentProfile,
    BatchLinkExecuteRequest, BatchLinkPreviewRequest, BatchUnlinkExecuteRequest,
    BatchUnlinkPreviewRequest, ExecuteLinkSkillRequest, ExecuteUnlinkSkillRequest, LinkMode,
    LinkOperationErrorKind, LinkPreviewStatus, LinkSkillRequest, TaskKind, TaskStatus,
    UnlinkPreviewStatus, UnlinkSkillRequest,
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
    // See tests/git_operations.rs::temp_dir for why we canonicalize: on macOS
    // /var/folders/... resolves to /private/var/folders/..., and production
    // code calls fs::canonicalize on workspace roots before using them.
    std::fs::canonicalize(&dir).unwrap()
}

fn profile(id: &str, skills_dir: &Path) -> AgentProfile {
    AgentProfile {
        id: id.to_string(),
        name: id.to_string(),
        skills_dir: skills_dir.display().to_string(),
        enabled: true,
        built_in: false,
        link_mode: LinkMode::Symlink,
    }
}

fn workspace_skill(workspace_root: &Path, relative_path: &str, name: &str) -> PathBuf {
    let skill_dir = workspace_root.join(relative_path);
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Test skill.\n---\n"),
    )
    .unwrap();
    skill_dir
}

#[cfg(unix)]
#[test]
fn previews_and_links_missing_target_for_discovered_workspace_skill() {
    let workspace_root = temp_dir("link_missing_target_workspace");
    let skill_dir = workspace_skill(&workspace_root, "repo-one/skills/example", "Example");
    let profile_dir = temp_dir("link_missing_target_profile");
    let profiles = vec![profile("codex", &profile_dir)];

    let preview = preview_link_skill_at(
        &workspace_root,
        &profiles,
        LinkSkillRequest {
            skill_id: "repo-one/skills/example".to_string(),
            agent_profile_id: "codex".to_string(),
            link_name: Some("example".to_string()),
        },
    )
    .unwrap();

    assert_eq!(preview.status, LinkPreviewStatus::WillLink);
    assert_eq!(preview.source_path, skill_dir.display().to_string());
    assert_eq!(
        preview.target_path,
        profile_dir.join("example").display().to_string()
    );

    let result = link_skill_at(
        &workspace_root,
        &profiles,
        ExecuteLinkSkillRequest { preview },
    )
    .unwrap();

    assert_eq!(result.task.kind, TaskKind::LinkSkill);
    assert_eq!(result.task.status, TaskStatus::Succeeded);
    assert_eq!(
        std::fs::canonicalize(profile_dir.join("example")).unwrap(),
        std::fs::canonicalize(&skill_dir).unwrap()
    );

    let installed = result
        .workspace
        .skills
        .iter()
        .find(|skill| skill.id == "repo-one/skills/example")
        .unwrap()
        .installed_agents
        .clone();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].agent_profile_id, "codex");
    assert_eq!(installed[0].link_name, "example");
}

#[test]
fn rejects_link_names_that_are_not_single_safe_path_segments() {
    let workspace_root = temp_dir("link_bad_name_workspace");
    workspace_skill(&workspace_root, "repo-one/skills/example", "Example");
    let profile_dir = temp_dir("link_bad_name_profile");
    let profiles = vec![profile("codex", &profile_dir)];

    let error = preview_link_skill_at(
        &workspace_root,
        &profiles,
        LinkSkillRequest {
            skill_id: "repo-one/skills/example".to_string(),
            agent_profile_id: "codex".to_string(),
            link_name: Some("nested\\name".to_string()),
        },
    )
    .unwrap_err();

    assert_eq!(error.kind, LinkOperationErrorKind::Validation);
}

#[cfg(unix)]
#[test]
fn previews_existing_targets_without_mutating_them() {
    let workspace_root = temp_dir("link_existing_targets_workspace");
    let skill_dir = workspace_skill(&workspace_root, "repo-one/skills/example", "Example");
    let other_skill_dir = workspace_skill(&workspace_root, "repo-one/skills/other", "Other");
    let profile_dir = temp_dir("link_existing_targets_profile");
    std::fs::write(profile_dir.join("file-link"), "keep me").unwrap();
    std::fs::create_dir(profile_dir.join("dir-link")).unwrap();
    std::os::unix::fs::symlink(&skill_dir, profile_dir.join("same-link")).unwrap();
    std::os::unix::fs::symlink(&other_skill_dir, profile_dir.join("other-link")).unwrap();
    let profiles = vec![profile("codex", &profile_dir)];

    let status_for = |name: &str| {
        preview_link_skill_at(
            &workspace_root,
            &profiles,
            LinkSkillRequest {
                skill_id: "repo-one/skills/example".to_string(),
                agent_profile_id: "codex".to_string(),
                link_name: Some(name.to_string()),
            },
        )
        .unwrap()
        .status
    };

    assert_eq!(
        status_for("file-link"),
        LinkPreviewStatus::BlockedByRealFile
    );
    assert_eq!(
        status_for("dir-link"),
        LinkPreviewStatus::BlockedByRealDirectory
    );
    assert_eq!(status_for("same-link"), LinkPreviewStatus::AlreadyInstalled);
    assert_eq!(status_for("other-link"), LinkPreviewStatus::NameConflict);
}

#[cfg(unix)]
#[test]
fn execution_blocks_real_files_and_leaves_them_unchanged() {
    let workspace_root = temp_dir("link_blocks_file_workspace");
    workspace_skill(&workspace_root, "repo-one/skills/example", "Example");
    let profile_dir = temp_dir("link_blocks_file_profile");
    let target = profile_dir.join("example");
    std::fs::write(&target, "original").unwrap();
    let profiles = vec![profile("codex", &profile_dir)];

    let preview = preview_link_skill_at(
        &workspace_root,
        &profiles,
        LinkSkillRequest {
            skill_id: "repo-one/skills/example".to_string(),
            agent_profile_id: "codex".to_string(),
            link_name: Some("example".to_string()),
        },
    )
    .unwrap();

    assert_eq!(preview.status, LinkPreviewStatus::BlockedByRealFile);
    let error = link_skill_at(
        &workspace_root,
        &profiles,
        ExecuteLinkSkillRequest { preview },
    )
    .unwrap_err();

    assert_eq!(error.kind, LinkOperationErrorKind::Blocked);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "original");
}

#[cfg(unix)]
#[test]
fn conflicting_symlink_blocks_until_user_chooses_safe_alternate_link_name() {
    let workspace_root = temp_dir("link_replace_conflict_workspace");
    let skill_dir = workspace_skill(&workspace_root, "repo-one/skills/example", "Example");
    let other_skill_dir = workspace_skill(&workspace_root, "repo-one/skills/other", "Other");
    let profile_dir = temp_dir("link_replace_conflict_profile");
    let target = profile_dir.join("example");
    std::os::unix::fs::symlink(&other_skill_dir, &target).unwrap();
    let profiles = vec![profile("codex", &profile_dir)];

    let preview = preview_link_skill_at(
        &workspace_root,
        &profiles,
        LinkSkillRequest {
            skill_id: "repo-one/skills/example".to_string(),
            agent_profile_id: "codex".to_string(),
            link_name: Some("example".to_string()),
        },
    )
    .unwrap();
    assert_eq!(preview.status, LinkPreviewStatus::NameConflict);

    let blocked = link_skill_at(
        &workspace_root,
        &profiles,
        ExecuteLinkSkillRequest {
            preview: preview.clone(),
        },
    )
    .unwrap_err();
    assert_eq!(blocked.kind, LinkOperationErrorKind::Blocked);
    assert_eq!(std::fs::canonicalize(&target).unwrap(), other_skill_dir);

    let replacement_attempt = link_skill_at(
        &workspace_root,
        &profiles,
        ExecuteLinkSkillRequest { preview },
    )
    .unwrap_err();
    assert_eq!(replacement_attempt.kind, LinkOperationErrorKind::Blocked);
    assert_eq!(std::fs::canonicalize(&target).unwrap(), other_skill_dir);

    let alternate_preview = preview_link_skill_at(
        &workspace_root,
        &profiles,
        LinkSkillRequest {
            skill_id: "repo-one/skills/example".to_string(),
            agent_profile_id: "codex".to_string(),
            link_name: Some("example-2".to_string()),
        },
    )
    .unwrap();
    assert_eq!(alternate_preview.status, LinkPreviewStatus::WillLink);

    let result = link_skill_at(
        &workspace_root,
        &profiles,
        ExecuteLinkSkillRequest {
            preview: alternate_preview,
        },
    )
    .unwrap();
    assert_eq!(result.task.status, TaskStatus::Succeeded);
    assert_eq!(std::fs::canonicalize(&target).unwrap(), other_skill_dir);
    assert_eq!(
        std::fs::canonicalize(profile_dir.join("example-2")).unwrap(),
        skill_dir
    );
}

#[cfg(unix)]
#[test]
fn changed_conflicting_symlink_target_after_preview_stays_unmodified() {
    let workspace_root = temp_dir("link_changed_conflict_workspace");
    workspace_skill(&workspace_root, "repo-one/skills/example", "Example");
    let first_conflict = workspace_skill(&workspace_root, "repo-one/skills/first", "First");
    let second_conflict = workspace_skill(&workspace_root, "repo-one/skills/second", "Second");
    let profile_dir = temp_dir("link_changed_conflict_profile");
    let target = profile_dir.join("example");
    std::os::unix::fs::symlink(&first_conflict, &target).unwrap();
    let profiles = vec![profile("codex", &profile_dir)];

    let preview = preview_link_skill_at(
        &workspace_root,
        &profiles,
        LinkSkillRequest {
            skill_id: "repo-one/skills/example".to_string(),
            agent_profile_id: "codex".to_string(),
            link_name: Some("example".to_string()),
        },
    )
    .unwrap();
    assert_eq!(preview.status, LinkPreviewStatus::NameConflict);

    std::fs::remove_file(&target).unwrap();
    std::os::unix::fs::symlink(&second_conflict, &target).unwrap();

    let error = link_skill_at(
        &workspace_root,
        &profiles,
        ExecuteLinkSkillRequest { preview },
    )
    .unwrap_err();

    assert_eq!(error.kind, LinkOperationErrorKind::StalePreview);
    assert_eq!(std::fs::canonicalize(&target).unwrap(), second_conflict);
}

#[cfg(unix)]
#[test]
fn stale_preview_does_not_mutate_changed_target() {
    let workspace_root = temp_dir("link_stale_preview_workspace");
    workspace_skill(&workspace_root, "repo-one/skills/example", "Example");
    let profile_dir = temp_dir("link_stale_preview_profile");
    let target = profile_dir.join("example");
    let profiles = vec![profile("codex", &profile_dir)];

    let preview = preview_link_skill_at(
        &workspace_root,
        &profiles,
        LinkSkillRequest {
            skill_id: "repo-one/skills/example".to_string(),
            agent_profile_id: "codex".to_string(),
            link_name: Some("example".to_string()),
        },
    )
    .unwrap();
    assert_eq!(preview.status, LinkPreviewStatus::WillLink);
    std::fs::write(&target, "arrived after preview").unwrap();

    let error = link_skill_at(
        &workspace_root,
        &profiles,
        ExecuteLinkSkillRequest { preview },
    )
    .unwrap_err();

    assert_eq!(error.kind, LinkOperationErrorKind::StalePreview);
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "arrived after preview"
    );
}

#[test]
fn missing_agent_profile_directory_previews_as_agent_path_missing() {
    let workspace_root = temp_dir("link_missing_agent_workspace");
    workspace_skill(&workspace_root, "repo-one/skills/example", "Example");
    let missing_profile_dir = temp_dir("link_missing_agent_parent")
        .join("missing")
        .join("skills");
    let profiles = vec![profile("codex", &missing_profile_dir)];

    let preview = preview_link_skill_at(
        &workspace_root,
        &profiles,
        LinkSkillRequest {
            skill_id: "repo-one/skills/example".to_string(),
            agent_profile_id: "codex".to_string(),
            link_name: Some("example".to_string()),
        },
    )
    .unwrap();

    assert_eq!(preview.status, LinkPreviewStatus::AgentPathMissing);
}

#[test]
fn missing_workspace_skill_previews_as_missing_source_without_execution() {
    let workspace_root = temp_dir("link_missing_source_workspace");
    let profile_dir = temp_dir("link_missing_source_profile");
    let profiles = vec![profile("codex", &profile_dir)];

    let preview = preview_link_skill_at(
        &workspace_root,
        &profiles,
        LinkSkillRequest {
            skill_id: "repo-one/skills/missing".to_string(),
            agent_profile_id: "codex".to_string(),
            link_name: Some("missing".to_string()),
        },
    )
    .unwrap();

    assert_eq!(preview.status, LinkPreviewStatus::MissingSource);
    assert!(preview
        .source_path
        .starts_with(workspace_root.to_str().unwrap()));

    let error = link_skill_at(
        &workspace_root,
        &profiles,
        ExecuteLinkSkillRequest { preview },
    )
    .unwrap_err();
    assert_eq!(error.kind, LinkOperationErrorKind::Blocked);
}

#[cfg(unix)]
#[test]
fn batch_preview_and_execute_links_safe_items_and_skips_conflicts() {
    let workspace_root = temp_dir("link_batch_workspace");
    let example_dir = workspace_skill(&workspace_root, "repo-one/skills/example", "Example");
    workspace_skill(&workspace_root, "repo-one/skills/other", "Other");
    let conflict_dir = workspace_skill(&workspace_root, "repo-one/skills/conflict", "Conflict");
    let profile_one_dir = temp_dir("link_batch_profile_one");
    let profile_two_dir = temp_dir("link_batch_profile_two");
    let conflict_target = profile_one_dir.join("other");
    std::os::unix::fs::symlink(&example_dir, profile_one_dir.join("example")).unwrap();
    std::os::unix::fs::symlink(&conflict_dir, &conflict_target).unwrap();
    let profiles = vec![
        profile("codex", &profile_one_dir),
        profile("claude", &profile_two_dir),
    ];

    let preview = preview_link_skills_batch_at(
        &workspace_root,
        &profiles,
        BatchLinkPreviewRequest {
            items: vec![
                LinkSkillRequest {
                    skill_id: "repo-one/skills/example".to_string(),
                    agent_profile_id: "codex".to_string(),
                    link_name: Some("example".to_string()),
                },
                LinkSkillRequest {
                    skill_id: "repo-one/skills/example".to_string(),
                    agent_profile_id: "claude".to_string(),
                    link_name: Some("example".to_string()),
                },
                LinkSkillRequest {
                    skill_id: "repo-one/skills/other".to_string(),
                    agent_profile_id: "codex".to_string(),
                    link_name: Some("other".to_string()),
                },
            ],
        },
    )
    .unwrap();

    assert_eq!(preview.previews.len(), 3);
    assert_eq!(
        preview.previews[0].status,
        LinkPreviewStatus::AlreadyInstalled
    );
    assert_eq!(preview.previews[1].status, LinkPreviewStatus::WillLink);
    assert_eq!(preview.previews[2].status, LinkPreviewStatus::NameConflict);

    let result = skilldock_lib::link_skills_batch_at(
        &workspace_root,
        &profiles,
        BatchLinkExecuteRequest {
            previews: preview.previews,
        },
    )
    .unwrap();

    assert_eq!(result.task.kind, TaskKind::LinkSkillsBatch);
    assert_eq!(result.task.status, TaskStatus::Succeeded);
    assert_eq!(result.summary.linked, 1);
    assert_eq!(result.summary.already_installed, 1);
    assert_eq!(result.summary.skipped, 1);
    assert_eq!(result.summary.failed, 0);
    assert_eq!(
        std::fs::canonicalize(profile_two_dir.join("example")).unwrap(),
        example_dir
    );
    assert_eq!(
        std::fs::canonicalize(&conflict_target).unwrap(),
        conflict_dir
    );
    assert!(!profile_one_dir.join("other-2").exists());
}

#[cfg(unix)]
#[test]
fn batch_execute_revalidates_each_preview_and_continues_after_stale_item() {
    let workspace_root = temp_dir("link_batch_stale_workspace");
    workspace_skill(&workspace_root, "repo-one/skills/stale", "Stale");
    let safe_dir = workspace_skill(&workspace_root, "repo-one/skills/safe", "Safe");
    let profile_dir = temp_dir("link_batch_stale_profile");
    let profiles = vec![profile("codex", &profile_dir)];

    let preview = preview_link_skills_batch_at(
        &workspace_root,
        &profiles,
        BatchLinkPreviewRequest {
            items: vec![
                LinkSkillRequest {
                    skill_id: "repo-one/skills/stale".to_string(),
                    agent_profile_id: "codex".to_string(),
                    link_name: Some("stale".to_string()),
                },
                LinkSkillRequest {
                    skill_id: "repo-one/skills/safe".to_string(),
                    agent_profile_id: "codex".to_string(),
                    link_name: Some("safe".to_string()),
                },
            ],
        },
    )
    .unwrap();

    std::fs::write(profile_dir.join("stale"), "arrived after preview").unwrap();

    let result = skilldock_lib::link_skills_batch_at(
        &workspace_root,
        &profiles,
        BatchLinkExecuteRequest {
            previews: preview.previews,
        },
    )
    .unwrap();

    assert_eq!(result.task.kind, TaskKind::LinkSkillsBatch);
    assert_eq!(result.task.status, TaskStatus::Failed);
    assert_eq!(result.summary.linked, 1);
    assert_eq!(result.summary.failed, 1);
    assert_eq!(
        std::fs::read_to_string(profile_dir.join("stale")).unwrap(),
        "arrived after preview"
    );
    assert_eq!(
        std::fs::canonicalize(profile_dir.join("safe")).unwrap(),
        safe_dir
    );
}

#[cfg(unix)]
#[test]
fn previews_and_unlinks_current_workspace_skill_symlink() {
    let workspace_root = temp_dir("unlink_valid_workspace");
    let skill_dir = workspace_skill(&workspace_root, "repo-one/skills/example", "Example");
    let profile_dir = temp_dir("unlink_valid_profile");
    std::os::unix::fs::symlink(&skill_dir, profile_dir.join("example")).unwrap();
    let profiles = vec![profile("codex", &profile_dir)];

    let preview = preview_unlink_skill_at(
        &workspace_root,
        &profiles,
        UnlinkSkillRequest {
            agent_profile_id: "codex".to_string(),
            link_name: "example".to_string(),
        },
    )
    .unwrap();

    assert_eq!(preview.status, UnlinkPreviewStatus::WillUnlink);
    assert_eq!(
        preview.source_path.as_deref(),
        Some(skill_dir.to_str().unwrap())
    );

    let result = unlink_skill_at(
        &workspace_root,
        &profiles,
        ExecuteUnlinkSkillRequest { preview },
    )
    .unwrap();

    assert_eq!(result.task.kind, TaskKind::UnlinkSkill);
    assert_eq!(result.task.status, TaskStatus::Succeeded);
    assert!(!profile_dir.join("example").exists());
    let skill = result
        .workspace
        .skills
        .iter()
        .find(|skill| skill.id == "repo-one/skills/example")
        .unwrap();
    assert!(skill.installed_agents.is_empty());
}

#[cfg(unix)]
#[test]
fn unlink_preview_blocks_files_directories_external_and_broken_symlinks() {
    let workspace_root = temp_dir("unlink_blocked_workspace");
    workspace_skill(&workspace_root, "repo-one/skills/example", "Example");
    let external_dir = temp_dir("unlink_external_target");
    let profile_dir = temp_dir("unlink_blocked_profile");
    std::fs::write(profile_dir.join("ordinary-file"), "keep").unwrap();
    std::fs::create_dir(profile_dir.join("ordinary-dir")).unwrap();
    std::os::unix::fs::symlink(&external_dir, profile_dir.join("external")).unwrap();
    std::os::unix::fs::symlink(profile_dir.join("missing"), profile_dir.join("broken")).unwrap();
    let profiles = vec![profile("codex", &profile_dir)];

    let status_for = |link_name: &str| {
        preview_unlink_skill_at(
            &workspace_root,
            &profiles,
            UnlinkSkillRequest {
                agent_profile_id: "codex".to_string(),
                link_name: link_name.to_string(),
            },
        )
        .unwrap()
        .status
    };

    assert_eq!(
        status_for("ordinary-file"),
        UnlinkPreviewStatus::BlockedByRealFile
    );
    assert_eq!(
        status_for("ordinary-dir"),
        UnlinkPreviewStatus::BlockedByRealDirectory
    );
    assert_eq!(status_for("external"), UnlinkPreviewStatus::ExternalSymlink);
    assert_eq!(status_for("broken"), UnlinkPreviewStatus::BrokenSymlink);

    let preview = preview_unlink_skill_at(
        &workspace_root,
        &profiles,
        UnlinkSkillRequest {
            agent_profile_id: "codex".to_string(),
            link_name: "ordinary-file".to_string(),
        },
    )
    .unwrap();
    let error = unlink_skill_at(
        &workspace_root,
        &profiles,
        ExecuteUnlinkSkillRequest { preview },
    )
    .unwrap_err();

    assert_eq!(error.kind, LinkOperationErrorKind::Blocked);
    assert_eq!(
        std::fs::read_to_string(profile_dir.join("ordinary-file")).unwrap(),
        "keep"
    );
    assert!(profile_dir.join("ordinary-dir").is_dir());
    assert!(profile_dir.join("external").exists());
}

#[cfg(unix)]
#[test]
fn batch_unlink_requires_preview_unlinks_safe_items_and_leaves_blocked_items() {
    let workspace_root = temp_dir("unlink_batch_workspace");
    let example_dir = workspace_skill(&workspace_root, "repo-one/skills/example", "Example");
    let other_dir = workspace_skill(&workspace_root, "repo-one/skills/other", "Other");
    let external_dir = temp_dir("unlink_batch_external");
    let profile_dir = temp_dir("unlink_batch_profile");
    std::os::unix::fs::symlink(&example_dir, profile_dir.join("example")).unwrap();
    std::os::unix::fs::symlink(&other_dir, profile_dir.join("other")).unwrap();
    std::os::unix::fs::symlink(&external_dir, profile_dir.join("external")).unwrap();
    std::fs::write(profile_dir.join("file"), "keep").unwrap();
    let profiles = vec![profile("codex", &profile_dir)];

    let preview = preview_unlink_skills_batch_at(
        &workspace_root,
        &profiles,
        BatchUnlinkPreviewRequest {
            items: vec![
                UnlinkSkillRequest {
                    agent_profile_id: "codex".to_string(),
                    link_name: "example".to_string(),
                },
                UnlinkSkillRequest {
                    agent_profile_id: "codex".to_string(),
                    link_name: "external".to_string(),
                },
                UnlinkSkillRequest {
                    agent_profile_id: "codex".to_string(),
                    link_name: "file".to_string(),
                },
            ],
        },
    )
    .unwrap();

    assert_eq!(preview.previews.len(), 3);
    assert_eq!(preview.previews[0].status, UnlinkPreviewStatus::WillUnlink);
    assert_eq!(
        preview.previews[1].status,
        UnlinkPreviewStatus::ExternalSymlink
    );
    assert_eq!(
        preview.previews[2].status,
        UnlinkPreviewStatus::BlockedByRealFile
    );

    let result = unlink_skills_batch_at(
        &workspace_root,
        &profiles,
        BatchUnlinkExecuteRequest {
            previews: preview.previews,
        },
    )
    .unwrap();

    assert_eq!(result.task.kind, TaskKind::UnlinkSkillsBatch);
    assert_eq!(result.task.status, TaskStatus::Succeeded);
    assert_eq!(result.summary.unlinked, 1);
    assert_eq!(result.summary.skipped, 2);
    assert_eq!(result.summary.failed, 0);
    assert!(!profile_dir.join("example").exists());
    assert_eq!(
        std::fs::canonicalize(profile_dir.join("other")).unwrap(),
        other_dir
    );
    assert_eq!(
        std::fs::canonicalize(profile_dir.join("external")).unwrap(),
        external_dir
    );
    assert_eq!(
        std::fs::read_to_string(profile_dir.join("file")).unwrap(),
        "keep"
    );
}

#[cfg(unix)]
#[test]
fn unlink_execute_rejects_stale_preview_and_leaves_new_path_untouched() {
    let workspace_root = temp_dir("unlink_stale_workspace");
    let skill_dir = workspace_skill(&workspace_root, "repo-one/skills/example", "Example");
    let profile_dir = temp_dir("unlink_stale_profile");
    let target = profile_dir.join("example");
    std::os::unix::fs::symlink(&skill_dir, &target).unwrap();
    let profiles = vec![profile("codex", &profile_dir)];

    let preview = preview_unlink_skill_at(
        &workspace_root,
        &profiles,
        UnlinkSkillRequest {
            agent_profile_id: "codex".to_string(),
            link_name: "example".to_string(),
        },
    )
    .unwrap();
    assert_eq!(preview.status, UnlinkPreviewStatus::WillUnlink);

    std::fs::remove_file(&target).unwrap();
    std::fs::write(&target, "replacement").unwrap();

    let error = unlink_skill_at(
        &workspace_root,
        &profiles,
        ExecuteUnlinkSkillRequest { preview },
    )
    .unwrap_err();

    assert_eq!(error.kind, LinkOperationErrorKind::StalePreview);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "replacement");
}
