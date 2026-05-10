use skills_collection_app_lib::{
    GitStatus, InstalledAgentSkill, InstalledAgentSkillStatus, ProjectCategory, ProjectTaskRecord,
    Skill, TaskKind, TaskRecord, TaskStatus,
};

#[test]
fn enums_serialize_to_spec_snake_case_values() {
    assert_eq!(
        serde_json::to_value(GitStatus::NoUpstream).unwrap(),
        "no_upstream"
    );
    assert_eq!(
        serde_json::to_value(ProjectCategory::DesignResources).unwrap(),
        "design_resources"
    );
    assert_eq!(
        serde_json::to_value(InstalledAgentSkillStatus::Valid).unwrap(),
        "valid"
    );
}

#[test]
fn task_record_serializes_structured_project_outcomes() {
    let task = TaskRecord {
        id: "task-1".to_string(),
        workspace_root: Some("/workspace".to_string()),
        kind: TaskKind::SyncAllProjects,
        status: TaskStatus::Failed,
        summary: "Some projects failed".to_string(),
        error: Some("summary: ok=1 skipped=0 failed=1".to_string()),
        stdout: "batch log\n".to_string(),
        stderr: String::new(),
        project_outcomes: vec![ProjectTaskRecord {
            project_id: "failing-project".to_string(),
            status: TaskStatus::Failed,
            summary: "failing-project: fetch failed".to_string(),
            error: Some("remote unavailable".to_string()),
        }],
    };

    let json = serde_json::to_value(task).unwrap();

    assert_eq!(json["projectOutcomes"][0]["projectId"], "failing-project");
    assert_eq!(json["workspaceRoot"], "/workspace");
    assert_eq!(json["projectOutcomes"][0]["status"], "failed");
    assert_eq!(json["projectOutcomes"][0]["error"], "remote unavailable");
}

#[test]
fn skill_serializes_with_camel_case_command_boundary_fields() {
    let skill = Skill {
        id: "skill-1".to_string(),
        name: "tdd".to_string(),
        description: Some("Test-driven development".to_string()),
        source_project_id: "project-1".to_string(),
        relative_path: "skills/tdd".to_string(),
        absolute_path: "/workspace/skills/tdd".to_string(),
        default_link_name: "skills-tdd".to_string(),
        has_assets: false,
        has_scripts: true,
        has_references: true,
        installed_agents: vec![InstalledAgentSkill {
            agent_profile_id: "codex".to_string(),
            link_name: "skills-tdd".to_string(),
            target_path: "/home/user/.codex/skills/skills-tdd".to_string(),
            source_path: "/workspace/skills/tdd".to_string(),
            status: InstalledAgentSkillStatus::Valid,
        }],
        last_modified: Some("2026-05-09T00:00:00Z".to_string()),
    };

    let json = serde_json::to_value(skill).unwrap();

    assert_eq!(json["sourceProjectId"], "project-1");
    assert_eq!(json["relativePath"], "skills/tdd");
    assert_eq!(json["defaultLinkName"], "skills-tdd");
    assert_eq!(json["hasAssets"], false);
    assert_eq!(json["hasScripts"], true);
    assert_eq!(json["hasReferences"], true);
    assert_eq!(json["installedAgents"][0]["agentProfileId"], "codex");
}
