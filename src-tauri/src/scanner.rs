use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    is_pull_all_eligible, load_workspace_config, AgentDirectoryEntry, AgentDirectoryEntryKind,
    AgentProfile, AgentProfileState, GitProvider, GitStatus, InstalledAgentSkill,
    InstalledAgentSkillStatus, Project, ProjectCategory, Skill, Workspace, WorkspaceConfig,
    WorkspaceError,
};

pub fn scan_workspace_at(
    workspace_root: impl AsRef<Path>,
    agent_profiles: &[AgentProfile],
) -> Result<Workspace, WorkspaceError> {
    let workspace_root = crate::validate_workspace_root(workspace_root.as_ref())?;
    let config = load_workspace_config(&workspace_root)
        .map_err(|error| WorkspaceError::config(Path::new(&error.path), error.message))?;
    let mut skills = scan_skills(&workspace_root)?;
    let agent_profile_states = scan_agent_profiles(&workspace_root, &mut skills, agent_profiles)?;
    let projects = scan_projects(&workspace_root, &config, &skills)?;

    Ok(Workspace {
        root: workspace_root.display().to_string(),
        projects,
        skills,
        agent_profiles: agent_profile_states,
    })
}

#[cfg_attr(feature = "desktop", tauri::command(rename_all = "camelCase"))]
pub fn scan_workspace_command(workspace_root: String) -> Result<Workspace, WorkspaceError> {
    let user_config = crate::load_user_config()
        .map_err(|error| WorkspaceError::config(Path::new(&error.path), error.message))?;
    scan_workspace_at(workspace_root, &user_config.agent_profiles)
}

fn scan_projects(
    workspace_root: &Path,
    config: &WorkspaceConfig,
    skills: &[Skill],
) -> Result<Vec<Project>, WorkspaceError> {
    let mut projects = Vec::new();
    let metadata_by_id = config
        .projects
        .iter()
        .map(|project| (project.project_id.as_str(), project))
        .collect::<HashMap<_, _>>();

    for entry in fs::read_dir(workspace_root)
        .map_err(|error| WorkspaceError::io(workspace_root, error.to_string()))?
    {
        let entry = entry.map_err(|error| WorkspaceError::io(workspace_root, error.to_string()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| WorkspaceError::io(&entry.path(), error.to_string()))?;
        if !file_type.is_dir() || !is_git_repository(&entry.path()) {
            continue;
        }

        let path = fs::canonicalize(entry.path())
            .map_err(|error| WorkspaceError::io(&entry.path(), error.to_string()))?;
        let id = entry.file_name().to_string_lossy().to_string();
        let metadata = metadata_by_id.get(id.as_str()).copied();
        let skill_count = skills
            .iter()
            .filter(|skill| skill.source_project_id == id)
            .count();

        let git_state = local_git_state(&path);
        let upstream = git_output(
            &path,
            &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
        )
        .ok();
        let pull_all_eligible = is_pull_all_eligible(&git_state.status, upstream.as_deref());

        projects.push(Project {
            id: id.clone(),
            name: metadata
                .and_then(|metadata| metadata.display_name.clone())
                .unwrap_or_else(|| id.clone()),
            path: path.display().to_string(),
            remote_url: git_output(&path, &["config", "--get", "remote.origin.url"]).ok(),
            provider: git_output(&path, &["config", "--get", "remote.origin.url"])
                .ok()
                .as_deref()
                .map(detect_provider)
                .unwrap_or(GitProvider::Unknown),
            branch: git_output(&path, &["symbolic-ref", "--short", "HEAD"]).ok(),
            upstream,
            git_status: git_state.status,
            ahead_count: git_state.ahead_count,
            behind_count: git_state.behind_count,
            pull_all_eligible,
            category: metadata
                .and_then(|metadata| metadata.category.clone())
                .unwrap_or_else(|| detect_category(&path, skill_count)),
            license_file: find_named_file(&path, &["LICENSE", "LICENSE.md", "COPYING"]),
            readme_file: find_named_file(&path, &["README.md", "README", "README.txt"]),
            readme_summary: readme_summary(&path),
            skill_count,
            hidden: metadata.map(|metadata| metadata.hidden).unwrap_or(false),
            favorite: metadata.map(|metadata| metadata.favorite).unwrap_or(false),
            tags: metadata
                .map(|metadata| metadata.tags.clone())
                .unwrap_or_default(),
            notes: metadata.and_then(|metadata| metadata.notes.clone()),
        });
    }

    projects.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(projects)
}

fn scan_skills(workspace_root: &Path) -> Result<Vec<Skill>, WorkspaceError> {
    let mut skills = Vec::new();
    visit_skill_dirs(workspace_root, workspace_root, &mut skills)?;
    skills.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(skills)
}

fn visit_skill_dirs(
    workspace_root: &Path,
    dir: &Path,
    skills: &mut Vec<Skill>,
) -> Result<(), WorkspaceError> {
    for entry in fs::read_dir(dir).map_err(|error| WorkspaceError::io(dir, error.to_string()))? {
        let entry = entry.map_err(|error| WorkspaceError::io(dir, error.to_string()))?;
        let path = entry.path();
        let file_name = entry.file_name();

        if file_name == OsStr::new(".git") || file_name == OsStr::new("node_modules") {
            continue;
        }

        let file_type = entry
            .file_type()
            .map_err(|error| WorkspaceError::io(&path, error.to_string()))?;
        if file_type.is_dir() {
            if path.join("SKILL.md").is_file() {
                skills.push(parse_skill(workspace_root, &path)?);
            }
            visit_skill_dirs(workspace_root, &path, skills)?;
        }
    }

    Ok(())
}

fn parse_skill(workspace_root: &Path, skill_dir: &Path) -> Result<Skill, WorkspaceError> {
    let canonical_dir = fs::canonicalize(skill_dir)
        .map_err(|error| WorkspaceError::io(skill_dir, error.to_string()))?;
    if !canonical_dir.starts_with(workspace_root) {
        return Err(WorkspaceError::outside_workspace(&canonical_dir));
    }

    let relative_path = canonical_dir
        .strip_prefix(workspace_root)
        .map_err(|error| WorkspaceError::io(&canonical_dir, error.to_string()))?
        .to_string_lossy()
        .to_string();
    let skill_md = canonical_dir.join("SKILL.md");
    let contents = fs::read_to_string(&skill_md).unwrap_or_default();
    let parsed = parse_skill_markdown(&contents);
    let fallback_name = canonical_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("skill")
        .to_string();
    let name = parsed.name.unwrap_or_else(|| fallback_name.clone());
    let description = parsed.description.or(parsed.fallback_description);
    let source_project_id = first_relative_component(&relative_path)
        .unwrap_or("workspace")
        .to_string();
    let modified = fs::metadata(&skill_md)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs().to_string());

    Ok(Skill {
        id: relative_path.clone(),
        name: name.clone(),
        description,
        source_project_id: source_project_id.clone(),
        relative_path,
        absolute_path: canonical_dir.display().to_string(),
        default_link_name: format!("{}-{}", slugify(&source_project_id), slugify(&name)),
        has_assets: canonical_dir.join("assets").is_dir(),
        has_scripts: canonical_dir.join("scripts").is_dir(),
        has_references: canonical_dir.join("references").is_dir(),
        installed_agents: Vec::new(),
        last_modified: modified,
    })
}

fn scan_agent_profiles(
    workspace_root: &Path,
    skills: &mut [Skill],
    agent_profiles: &[AgentProfile],
) -> Result<Vec<AgentProfileState>, WorkspaceError> {
    let mut skill_index_by_path = HashMap::new();
    for (index, skill) in skills.iter().enumerate() {
        let canonical = fs::canonicalize(&skill.absolute_path).map_err(|error| {
            WorkspaceError::io(Path::new(&skill.absolute_path), error.to_string())
        })?;
        skill_index_by_path.insert(canonical, index);
    }

    let mut states = Vec::new();
    for profile in agent_profiles {
        let skills_dir = expand_home(&profile.skills_dir);
        if !skills_dir.exists() {
            states.push(AgentProfileState {
                profile: profile.clone(),
                skills_dir: skills_dir.display().to_string(),
                exists: false,
                writable: false,
                symlink_count: 0,
                workspace_link_count: 0,
                entries: Vec::new(),
            });
            continue;
        }

        let mut entries = Vec::new();
        let mut symlink_count = 0;
        let mut workspace_link_count = 0;
        for entry in fs::read_dir(&skills_dir)
            .map_err(|error| WorkspaceError::io(&skills_dir, error.to_string()))?
        {
            let entry =
                entry.map_err(|error| WorkspaceError::io(&skills_dir, error.to_string()))?;
            let path = entry.path();
            let file_type = fs::symlink_metadata(&path)
                .map_err(|error| WorkspaceError::io(&path, error.to_string()))?
                .file_type();
            let name = entry.file_name().to_string_lossy().to_string();

            if file_type.is_symlink() {
                symlink_count += 1;
                let raw_target = fs::read_link(&path)
                    .map_err(|error| WorkspaceError::io(&path, error.to_string()))?;
                let resolved_target = if raw_target.is_absolute() {
                    raw_target
                } else {
                    skills_dir.join(raw_target)
                };
                let (status, source_path, removable) =
                    classify_symlink_target(workspace_root, &resolved_target, &skill_index_by_path);
                if status == InstalledAgentSkillStatus::Valid {
                    workspace_link_count += 1;
                    if let Some(source_path) = source_path.as_ref() {
                        if let Some(index) = skill_index_by_path.get(Path::new(source_path)) {
                            skills[*index].installed_agents.push(InstalledAgentSkill {
                                agent_profile_id: profile.id.clone(),
                                link_name: name.clone(),
                                target_path: path.display().to_string(),
                                source_path: source_path.clone(),
                                status: status.clone(),
                            });
                        }
                    }
                }
                entries.push(AgentDirectoryEntry {
                    name,
                    path: path.display().to_string(),
                    target_path: Some(resolved_target.display().to_string()),
                    source_path,
                    kind: AgentDirectoryEntryKind::Symlink,
                    status,
                    removable,
                });
            } else {
                entries.push(AgentDirectoryEntry {
                    name,
                    path: path.display().to_string(),
                    target_path: None,
                    source_path: None,
                    kind: if file_type.is_dir() {
                        AgentDirectoryEntryKind::Directory
                    } else if file_type.is_file() {
                        AgentDirectoryEntryKind::File
                    } else {
                        AgentDirectoryEntryKind::Other
                    },
                    status: InstalledAgentSkillStatus::Conflict,
                    removable: false,
                });
            }
        }

        entries.sort_by(|left, right| left.name.cmp(&right.name));
        states.push(AgentProfileState {
            profile: profile.clone(),
            skills_dir: skills_dir.display().to_string(),
            exists: true,
            writable: is_writable_dir(&skills_dir),
            symlink_count,
            workspace_link_count,
            entries,
        });
    }

    Ok(states)
}

fn classify_symlink_target(
    workspace_root: &Path,
    resolved_target: &Path,
    skill_index_by_path: &HashMap<PathBuf, usize>,
) -> (InstalledAgentSkillStatus, Option<String>, bool) {
    let Ok(canonical_target) = fs::canonicalize(resolved_target) else {
        return (InstalledAgentSkillStatus::Broken, None, false);
    };

    if skill_index_by_path.contains_key(&canonical_target) {
        return (
            InstalledAgentSkillStatus::Valid,
            Some(canonical_target.display().to_string()),
            true,
        );
    }

    if canonical_target.starts_with(workspace_root) {
        return (
            InstalledAgentSkillStatus::Conflict,
            Some(canonical_target.display().to_string()),
            false,
        );
    }

    (
        InstalledAgentSkillStatus::External,
        Some(canonical_target.display().to_string()),
        false,
    )
}

fn is_git_repository(path: &Path) -> bool {
    path.join(".git").exists()
        && Command::new("git")
            .arg("-C")
            .arg(path)
            .arg("rev-parse")
            .arg("--git-dir")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
}

struct LocalGitState {
    status: GitStatus,
    ahead_count: u32,
    behind_count: u32,
}

fn local_git_state(path: &Path) -> LocalGitState {
    if git_output(path, &["symbolic-ref", "--short", "HEAD"]).is_err() {
        return git_state(GitStatus::Detached, 0, 0);
    }

    let dirty = git_output(path, &["status", "--porcelain=v1"])
        .map(|output| !output.is_empty())
        .unwrap_or(false);

    if git_output(
        path,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .is_err()
    {
        return git_state(
            if dirty {
                GitStatus::Dirty
            } else {
                GitStatus::NoUpstream
            },
            0,
            0,
        );
    }

    match git_output(
        path,
        &["rev-list", "--left-right", "--count", "HEAD...@{u}"],
    ) {
        Ok(counts) => {
            let mut parts = counts.split_whitespace();
            let ahead = parts
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(0);
            let behind = parts
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(0);
            let status = if dirty {
                GitStatus::Dirty
            } else {
                match (ahead, behind) {
                    (0, 0) => GitStatus::UpToDate,
                    (_, 0) => GitStatus::Ahead,
                    (0, _) => GitStatus::Behind,
                    _ => GitStatus::Diverged,
                }
            };
            git_state(status, ahead, behind)
        }
        Err(_) => git_state(
            if dirty {
                GitStatus::Dirty
            } else {
                GitStatus::Unknown
            },
            0,
            0,
        ),
    }
}

fn git_state(status: GitStatus, ahead_count: u32, behind_count: u32) -> LocalGitState {
    LocalGitState {
        status,
        ahead_count,
        behind_count,
    }
}

fn git_output(path: &Path, args: &[&str]) -> Result<String, ()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .map_err(|_| ())?;
    if !output.status.success() {
        return Err(());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn detect_provider(remote_url: &str) -> GitProvider {
    if remote_url.contains("github.com") {
        GitProvider::Github
    } else if remote_url.contains("gitlab.com") {
        GitProvider::Gitlab
    } else {
        GitProvider::Unknown
    }
}

fn detect_category(path: &Path, skill_count: usize) -> ProjectCategory {
    if skill_count > 0 {
        return ProjectCategory::Skills;
    }
    if path.join(".codex-plugin").join("plugin.json").is_file()
        || path.join(".claude-plugin").join("plugin.json").is_file()
    {
        return ProjectCategory::Plugins;
    }
    if path.join("package.json").is_file()
        || path.join("Cargo.toml").is_file()
        || path.join("bin").is_dir()
    {
        return ProjectCategory::Tools;
    }
    ProjectCategory::Uncategorized
}

fn find_named_file(path: &Path, candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|candidate| path.join(candidate).is_file())
        .map(|candidate| (*candidate).to_string())
}

fn readme_summary(path: &Path) -> Option<String> {
    let readme_file = find_named_file(path, &["README.md", "README", "README.txt"])?;
    fs::read_to_string(path.join(readme_file))
        .ok()
        .and_then(|contents| first_meaningful_markdown_line(&contents))
}

struct ParsedSkillMarkdown {
    name: Option<String>,
    description: Option<String>,
    fallback_description: Option<String>,
}

fn parse_skill_markdown(contents: &str) -> ParsedSkillMarkdown {
    let mut name = None;
    let mut description = None;
    let body = if let Some(rest) = contents.strip_prefix("---\n") {
        if let Some((frontmatter, body)) = rest.split_once("\n---") {
            for line in frontmatter.lines() {
                if let Some(value) = line.strip_prefix("name:") {
                    name = Some(clean_yaml_scalar(value));
                } else if let Some(value) = line.strip_prefix("description:") {
                    description = Some(clean_yaml_scalar(value));
                }
            }
            body
        } else {
            contents
        }
    } else {
        contents
    };

    ParsedSkillMarkdown {
        name,
        description,
        fallback_description: first_meaningful_markdown_line(body),
    }
}

fn clean_yaml_scalar(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;

    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
    }

    slug.trim_matches('-').to_string()
}

fn first_meaningful_markdown_line(contents: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "---" {
            return None;
        }
        Some(trimmed.trim_start_matches('#').trim().to_string())
    })
}

fn first_relative_component(relative_path: &str) -> Option<&str> {
    Path::new(relative_path)
        .components()
        .find_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(path));
    }

    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }

    PathBuf::from(path)
}

fn is_writable_dir(path: &Path) -> bool {
    for attempt in 0..3 {
        let probe_path = path.join(unique_writability_probe_name(attempt));
        let write_result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&probe_path)?;
            file.write_all(b"skills-collection writability probe\n")?;
            file.sync_all()?;
            drop(file);
            fs::remove_file(&probe_path)?;
            Ok::<(), std::io::Error>(())
        })();

        match write_result {
            Ok(()) => return true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => {
                let _ = fs::remove_file(&probe_path);
                return false;
            }
        }
    }

    false
}

fn unique_writability_probe_name(attempt: u8) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        ".skills-collection-writable-probe-{}-{nanos}-{attempt}.tmp",
        std::process::id()
    )
}
