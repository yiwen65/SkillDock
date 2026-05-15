import { memo, useEffect, useMemo, useRef, useState } from "react";
import { previewUnlinkSkill, saveAgentProfiles, scanWorkspace, unlinkSkill } from "../lib/commands";
import { EmptyState, PanelHeader, errorMessage } from "../lib/shared";
import type {
  AgentProfile,
  AgentProfileState,
  Skill,
  TaskOperationResult,
  Workspace,
} from "../lib/types";

type LinkedProfileSkill = {
  linkName: string;
  projectName: string;
  skillId: string;
  skillName: string;
  sourcePath: string;
  status: Skill["installedAgents"][number]["status"];
  targetPath: string;
};

function isValidProfilePath(path: string) {
  if (path.includes("\0")) return false;
  if (path.startsWith("\\\\") || path.startsWith("//")) return isValidUncPath(path);
  return (
    path === "~" ||
    path.startsWith("~/") ||
    path.startsWith("~\\") ||
    path.startsWith("/") ||
    /^[a-zA-Z]:[\\/]/.test(path)
  );
}

function isValidUncPath(path: string) {
  const normalized = path.replace(/\\/g, "/");
  if (!normalized.startsWith("//")) return false;
  const parts = normalized
    .slice(2)
    .split("/")
    .filter((part) => part.length > 0);
  return parts.length >= 2;
}

function normalizeProfilePathForCompare(path: string) {
  let normalized = path.trim().replace(/\\/g, "/");
  const hasUncPrefix = normalized.startsWith("//");
  if (hasUncPrefix) {
    normalized = `//${normalized.slice(2).replace(/\/+/g, "/")}`;
  } else {
    normalized = normalized.replace(/\/+/g, "/");
  }
  while (normalized.length > 1 && normalized.endsWith("/")) {
    normalized = normalized.slice(0, -1);
  }
  if (/^[a-zA-Z]:/.test(normalized)) {
    normalized = `${normalized[0].toLowerCase()}${normalized.slice(1)}`;
  }
  return normalized;
}

function validateProfileDrafts(profiles: AgentProfile[]) {
  const ids = new Set<string>();
  const skillsDirs = new Set<string>();
  for (const profile of profiles) {
    const id = profile.id.trim();
    if (!id) return "Profile id is required.";
    if (profile.id !== id)
      return `Profile id '${id}' must not contain leading or trailing whitespace.`;
    if (!/^[a-zA-Z0-9._-]+$/.test(id))
      return `Profile id '${id}' may only use letters, numbers, dots, underscores and hyphens.`;
    if (ids.has(id)) return `Profile id '${id}' is duplicated.`;
    ids.add(id);
    if (!profile.name.trim()) return `Profile '${id}' requires a name.`;
    const skillsDir = profile.skillsDir.trim();
    if (!isValidProfilePath(skillsDir))
      return `Profile '${id}' requires an absolute or home-relative skills directory.`;
    const normalizedSkillsDir = normalizeProfilePathForCompare(skillsDir);
    if (skillsDirs.has(normalizedSkillsDir))
      return `Profile skills directory '${skillsDir}' is duplicated.`;
    skillsDirs.add(normalizedSkillsDir);
  }
  return null;
}

function emptyCustomProfile(): AgentProfile {
  return {
    id: "",
    name: "",
    skillsDir: "",
    enabled: true,
    builtIn: false,
    linkMode: "symlink",
  };
}

export const AgentsView = memo(function AgentsView({
  onCreateAgentDir,
  onOperationResult,
  onWorkspaceChange,
  workspace,
}: {
  onCreateAgentDir: (profile: AgentProfileState) => Promise<void> | void;
  onOperationResult: (result: TaskOperationResult) => void;
  onWorkspaceChange: (workspace: Workspace, message: string) => void;
  workspace: Workspace;
}) {
  const profiles = workspace.agentProfiles;
  const [draftProfiles, setDraftProfiles] = useState<AgentProfile[]>(
    profiles.map((state) => state.profile),
  );
  const [editingId, setEditingId] = useState<string | null>(null);
  const [formProfile, setFormProfile] = useState<AgentProfile>(() => emptyCustomProfile());
  const [profileMessage, setProfileMessage] = useState<string | null>(null);
  const [agentsBusy, setAgentsBusy] = useState(false);
  const agentsBusyRef = useRef(false);

  useEffect(() => {
    const nextProfiles = profiles.map((state) => state.profile);
    setDraftProfiles(nextProfiles);
    setEditingId(null);
    setFormProfile(emptyCustomProfile());
    setProfileMessage(null);
  }, [profiles, workspace.root]);

  const linkedSkillsByProfile = useMemo(() => {
    const projectById = new Map(workspace.projects.map((p) => [p.id, p]));
    const grouped = new Map<string, LinkedProfileSkill[]>();
    for (const skill of workspace.skills) {
      for (const install of skill.installedAgents) {
        const links = grouped.get(install.agentProfileId) ?? [];
        links.push({
          linkName: install.linkName,
          projectName: projectById.get(skill.sourceProjectId)?.name ?? skill.sourceProjectId,
          skillId: skill.id,
          skillName: skill.name,
          sourcePath: install.sourcePath,
          status: install.status,
          targetPath: install.targetPath,
        });
        grouped.set(install.agentProfileId, links);
      }
    }
    for (const links of grouped.values()) {
      links.sort((left, right) => left.skillName.localeCompare(right.skillName));
    }
    return grouped;
  }, [workspace.skills, workspace.projects]);

  const startAddProfile = () => {
    if (agentsBusyRef.current) return;
    setEditingId(null);
    setFormProfile(emptyCustomProfile());
    setProfileMessage(null);
  };

  const startEditProfile = (profile: AgentProfile) => {
    if (agentsBusyRef.current) return;
    setEditingId(profile.id);
    setFormProfile({ ...profile });
    setProfileMessage(null);
  };

  const persistProfiles = async (nextProfiles: AgentProfile[], message: string) => {
    if (agentsBusyRef.current) return;
    const validationError = validateProfileDrafts(nextProfiles);
    if (validationError) {
      setProfileMessage(validationError);
      return;
    }
    agentsBusyRef.current = true;
    setAgentsBusy(true);
    setProfileMessage("Saving profiles...");
    try {
      await saveAgentProfiles(nextProfiles);
      const nextWorkspace = await scanWorkspace(workspace.root);
      setDraftProfiles(nextProfiles);
      setEditingId(null);
      setFormProfile(emptyCustomProfile());
      setProfileMessage(message);
      onWorkspaceChange(nextWorkspace, message);
    } catch (error) {
      setProfileMessage(errorMessage(error));
    } finally {
      agentsBusyRef.current = false;
      setAgentsBusy(false);
    }
  };

  const saveProfileForm = async () => {
    const normalized: AgentProfile = {
      ...formProfile,
      name: formProfile.name.trim(),
      skillsDir: formProfile.skillsDir.trim(),
    };
    const nextProfiles = editingId
      ? draftProfiles.map((profile) => (profile.id === editingId ? normalized : profile))
      : [...draftProfiles, normalized];
    await persistProfiles(nextProfiles, `${normalized.name} saved.`);
  };

  const toggleProfile = async (profile: AgentProfile) => {
    const nextProfile = { ...profile, enabled: !profile.enabled };
    await persistProfiles(
      draftProfiles.map((item) => (item.id === profile.id ? nextProfile : item)),
      `${profile.name} ${nextProfile.enabled ? "enabled" : "disabled"}.`,
    );
  };

  const directLinkedSkillUninstall = async (
    profile: AgentProfileState,
    link: LinkedProfileSkill,
  ) => {
    if (agentsBusyRef.current) return;
    agentsBusyRef.current = true;
    setAgentsBusy(true);
    setProfileMessage(`Uninstalling ${link.skillName}...`);
    try {
      const preview = await previewUnlinkSkill(workspace.root, {
        agentProfileId: profile.profile.id,
        linkName: link.linkName,
      });
      const result = await unlinkSkill(workspace.root, { preview });
      onOperationResult(result);
      setProfileMessage(result.task.summary);
    } catch (error) {
      setProfileMessage(errorMessage(error));
    } finally {
      agentsBusyRef.current = false;
      setAgentsBusy(false);
    }
  };

  const createProfileDir = async (profile: AgentProfileState) => {
    if (agentsBusyRef.current) return;
    agentsBusyRef.current = true;
    setAgentsBusy(true);
    try {
      await onCreateAgentDir(profile);
    } finally {
      agentsBusyRef.current = false;
      setAgentsBusy(false);
    }
  };

  return (
    <section className="data-panel agents-panel">
      <PanelHeader title="Agents" detail={`${profiles.length} profiles`} />
      <div className="agents-layout">
        <div className="table-list">
          {profiles.length === 0 && (
            <EmptyState
              title="No agent profiles"
              body="Add a custom profile to start linking skills."
            />
          )}
          {profiles.map((state) => {
            const linkedSkills = linkedSkillsByProfile.get(state.profile.id) ?? [];
            return (
              <article className="list-row agent-row" key={state.profile.id}>
                <div className="agent-main">
                  <div className="agent-title-line">
                    <h2>{state.profile.name}</h2>
                    <span className="subtle-pill">
                      {state.profile.builtIn ? "Built-in" : "Custom"}
                    </span>
                    <span
                      className={state.profile.enabled ? "subtle-pill" : "subtle-pill muted-pill"}
                    >
                      {state.profile.enabled ? "Enabled" : "Disabled"}
                    </span>
                  </div>
                  <p>{state.skillsDir}</p>
                  <div className="project-meta agent-meta">
                    <span>{linkedSkills.length} skills installed</span>
                  </div>
                  <div className="agent-linked-list">
                    {linkedSkills.length === 0 && <p>No workspace skills linked.</p>}
                    {linkedSkills.map((link) => (
                      <div
                        className="agent-linked-row"
                        key={`${state.profile.id}\u0000${link.linkName}`}
                      >
                        <div>
                          <strong>{link.skillName}</strong>
                          <span>{link.projectName}</span>
                        </div>
                        <span>{link.status}</span>
                        <button
                          className="secondary-button"
                          disabled={agentsBusy}
                          onClick={() => directLinkedSkillUninstall(state, link)}
                          type="button"
                        >
                          Uninstall
                        </button>
                      </div>
                    ))}
                  </div>
                </div>
                <div className="row-actions agent-actions">
                  {!state.exists && (
                    <button
                      className="secondary-button"
                      disabled={agentsBusy}
                      onClick={() => createProfileDir(state)}
                      type="button"
                    >
                      Create directory
                    </button>
                  )}
                  <button
                    className="secondary-button"
                    disabled={agentsBusy}
                    onClick={() => toggleProfile(state.profile)}
                    type="button"
                  >
                    {state.profile.enabled ? "Disable" : "Enable"}
                  </button>
                  {!state.profile.builtIn && (
                    <button
                      className="secondary-button"
                      disabled={agentsBusy}
                      onClick={() => startEditProfile(state.profile)}
                      type="button"
                    >
                      Edit
                    </button>
                  )}
                </div>
              </article>
            );
          })}
        </div>
        <form
          className="compact-form agent-editor"
          onSubmit={(event) => {
            event.preventDefault();
            saveProfileForm();
          }}
        >
          <div className="form-title-row">
            <h2>{editingId ? "Edit custom profile" : "Add custom profile"}</h2>
            <button
              className="secondary-button"
              disabled={agentsBusy}
              onClick={startAddProfile}
              type="button"
            >
              New
            </button>
          </div>
          <label>
            <span>Profile id</span>
            <input
              disabled={Boolean(editingId) || agentsBusy}
              readOnly={agentsBusy}
              onChange={(event) => setFormProfile({ ...formProfile, id: event.target.value })}
              placeholder="custom-agent"
              value={formProfile.id}
            />
          </label>
          <label>
            <span>Name</span>
            <input
              onChange={(event) => setFormProfile({ ...formProfile, name: event.target.value })}
              placeholder="Custom Agent"
              value={formProfile.name}
              readOnly={agentsBusy}
            />
          </label>
          <label>
            <span>Skills directory</span>
            <input
              onChange={(event) =>
                setFormProfile({ ...formProfile, skillsDir: event.target.value })
              }
              placeholder="~/.agent_name/skills"
              value={formProfile.skillsDir}
              readOnly={agentsBusy}
            />
          </label>
          <label className="inline-check">
            <input
              checked={formProfile.enabled}
              disabled={agentsBusy}
              onChange={(event) =>
                setFormProfile({ ...formProfile, enabled: event.target.checked })
              }
              type="checkbox"
            />
            <span>Enabled</span>
          </label>
          <button className="primary-button" disabled={agentsBusy} type="submit">
            Save profile
          </button>
          {profileMessage && <p className="form-error">{profileMessage}</p>}
        </form>
      </div>
    </section>
  );
});
