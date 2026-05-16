import { memo, useEffect, useMemo, useRef, useState } from "react";
import { previewUnlinkSkill, saveAgentProfiles, scanWorkspace, unlinkSkill } from "../lib/commands";
import { EmptyState, PanelHeader, errorMessage } from "../lib/shared";
import type {
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
  const [profileMessage, setProfileMessage] = useState<string | null>(null);
  const [expandedProfileIds, setExpandedProfileIds] = useState<Set<string>>(() => new Set());
  const [loadedExpandedStorageKey, setLoadedExpandedStorageKey] = useState<string | null>(null);
  const [agentsBusy, setAgentsBusy] = useState(false);
  const agentsBusyRef = useRef(false);
  const profileIds = useMemo(() => profiles.map((state) => state.profile.id), [profiles]);
  const profileIdsKey = profileIds.join("\u0000");
  const expandedStorageKey = useMemo(
    () => `skilldock:agents:expanded:${workspace.root}`,
    [workspace.root],
  );

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

  useEffect(() => {
    setLoadedExpandedStorageKey(null);
    try {
      const stored = window.localStorage.getItem(expandedStorageKey);
      const parsed = stored ? JSON.parse(stored) : [];
      const validIds = new Set(profileIds);
      const restored =
        Array.isArray(parsed) && parsed.every((item) => typeof item === "string")
          ? parsed.filter((profileId) => validIds.has(profileId))
          : [];
      setExpandedProfileIds(new Set(restored));
    } catch {
      setExpandedProfileIds(new Set());
    } finally {
      setLoadedExpandedStorageKey(expandedStorageKey);
    }
  }, [expandedStorageKey, profileIdsKey, profileIds]);

  useEffect(() => {
    if (loadedExpandedStorageKey !== expandedStorageKey) return;
    try {
      window.localStorage.setItem(expandedStorageKey, JSON.stringify([...expandedProfileIds]));
    } catch {
      // Local storage is a convenience only; folding still works without it.
    }
  }, [expandedProfileIds, expandedStorageKey, loadedExpandedStorageKey]);

  const toggleLinkedSkills = (profileId: string) => {
    setExpandedProfileIds((current) => {
      const next = new Set(current);
      if (next.has(profileId)) {
        next.delete(profileId);
      } else {
        next.add(profileId);
      }
      return next;
    });
  };

  const toggleProfile = async (profile: AgentProfileState["profile"]) => {
    if (agentsBusyRef.current) return;
    const nextProfile = { ...profile, enabled: !profile.enabled };
    const nextProfiles = profiles.map((state) =>
      state.profile.id === profile.id ? nextProfile : state.profile,
    );

    agentsBusyRef.current = true;
    setAgentsBusy(true);
    setProfileMessage(`${nextProfile.enabled ? "Enabling" : "Disabling"} ${profile.name}...`);
    try {
      await saveAgentProfiles(nextProfiles);
      const nextWorkspace = await scanWorkspace(workspace.root);
      const message = `${profile.name} ${nextProfile.enabled ? "enabled" : "disabled"}.`;
      setProfileMessage(message);
      onWorkspaceChange(nextWorkspace, message);
    } catch (error) {
      setProfileMessage(errorMessage(error));
    } finally {
      agentsBusyRef.current = false;
      setAgentsBusy(false);
    }
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
              body="Configure agent profiles in Settings to start linking skills."
            />
          )}
          {profiles.map((state) => {
            const linkedSkills = linkedSkillsByProfile.get(state.profile.id) ?? [];
            const linkedSkillsId = `agent-linked-skills-${state.profile.id}`;
            const linkedSkillsExpanded = expandedProfileIds.has(state.profile.id);
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
                  <div className="agent-skill-toolbar">
                    <button
                      aria-label={`${linkedSkillsExpanded ? "Hide" : "Show"} linked skills for ${state.profile.name}`}
                      aria-controls={linkedSkillsId}
                      aria-expanded={linkedSkillsExpanded}
                      className="agent-skill-toggle"
                      onClick={() => toggleLinkedSkills(state.profile.id)}
                      type="button"
                    >
                      <span>{formatInstalledSkills(linkedSkills.length)}</span>
                      <ToggleChevron collapsed={!linkedSkillsExpanded} />
                    </button>
                  </div>
                  {linkedSkillsExpanded && (
                    <div className="agent-linked-list" id={linkedSkillsId}>
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
                  )}
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
                </div>
              </article>
            );
          })}
        </div>
      </div>
      {profileMessage && <p className="batch-message">{profileMessage}</p>}
    </section>
  );
});

function formatInstalledSkills(count: number) {
  return `${count} ${count === 1 ? "skill" : "skills"} installed`;
}

function ToggleChevron({ collapsed }: { collapsed: boolean }) {
  return (
    <svg
      aria-hidden="true"
      className={collapsed ? "toggle-chevron collapsed" : "toggle-chevron"}
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      viewBox="0 0 24 24"
    >
      <path d="m6 9 6 6 6-6" />
    </svg>
  );
}
