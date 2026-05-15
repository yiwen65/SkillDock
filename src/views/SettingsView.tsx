import { memo, useEffect, useRef, useState } from "react";
import {
  loadUserConfig,
  patchUserPreferences,
  saveAgentProfiles,
  scanWorkspace,
  selectWorkspace,
} from "../lib/commands";
import { PanelHeader, errorMessage } from "../lib/shared";
import type { AgentProfile, UserConfig, Workspace } from "../lib/types";

function clampAutomaticCheckInterval(value: number) {
  if (!Number.isFinite(value)) return 1440;
  return Math.min(43200, Math.max(1440, Math.round(value)));
}

function isValidProfilePath(path: string) {
  if (path.includes("\0")) return false;
  if (path.startsWith("\\\\") || path.startsWith("//")) {
    const normalized = path.replace(/\\/g, "/");
    if (!normalized.startsWith("//")) return false;
    const parts = normalized
      .slice(2)
      .split("/")
      .filter((part) => part.length > 0);
    return parts.length >= 2;
  }
  return (
    path === "~" ||
    path.startsWith("~/") ||
    path.startsWith("~\\") ||
    path.startsWith("/") ||
    /^[a-zA-Z]:[\\/]/.test(path)
  );
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

export const SettingsView = memo(function SettingsView({
  onWorkspaceChange,
  workspace,
}: {
  onWorkspaceChange: (workspace: Workspace, message: string) => void;
  workspace: Workspace;
}) {
  const [config, setConfig] = useState<UserConfig | null>(null);
  const [workspaceDraft, setWorkspaceDraft] = useState("");
  const [profileDrafts, setProfileDrafts] = useState<AgentProfile[]>(
    workspace.agentProfiles.map((state) => state.profile),
  );
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // Ref guard backs up the `busy` state so a rapid second click landing in the
  // tiny window before React applies the disabled prop cannot fire an extra
  // backend call.
  const busyRef = useRef(false);

  useEffect(() => {
    let cancelled = false;
    setMessage("Loading settings...");
    loadUserConfig()
      .then((loaded) => {
        if (cancelled) return;
        setConfig(loaded);
        setProfileDrafts(loaded.agentProfiles);
        setMessage(null);
      })
      .catch((error) => {
        if (!cancelled) setMessage(errorMessage(error));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    setWorkspaceDraft("");
  }, [workspace.root]);

  const updateConfig = (update: (current: UserConfig) => UserConfig) => {
    if (!config) return;
    setConfig(update(config));
  };

  const saveGeneralSettings = async () => {
    if (!config) return;
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setMessage("Saving settings...");
    const patch = {
      recentWorkspaces: config.recentWorkspaces,
      automaticChecks: {
        ...config.automaticChecks,
        intervalMinutes: clampAutomaticCheckInterval(config.automaticChecks.intervalMinutes),
      },
      uiPreferences: config.uiPreferences,
    };
    try {
      const saved = await patchUserPreferences(patch);
      setConfig(saved);
      setMessage("Settings saved.");
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const switchWorkspace = async (path: string) => {
    if (busyRef.current) return;
    const nextPath = path.trim();
    if (!nextPath) {
      setMessage("Workspace path is required.");
      return;
    }
    busyRef.current = true;
    setBusy(true);
    setMessage("Opening workspace...");
    try {
      const nextWorkspace = await selectWorkspace(nextPath);
      const saved = await loadUserConfig();
      setConfig(saved);
      setWorkspaceDraft("");
      onWorkspaceChange(nextWorkspace, "Workspace changed.");
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const saveProfiles = async () => {
    if (busyRef.current) return;
    const normalizedProfiles = profileDrafts.map((profile) => ({
      ...profile,
      name: profile.name.trim(),
      skillsDir: profile.skillsDir.trim(),
    }));
    const validationError = validateProfileDrafts(normalizedProfiles);
    if (validationError) {
      setMessage(validationError);
      return;
    }
    busyRef.current = true;
    setBusy(true);
    setMessage("Saving agent profiles...");
    try {
      const saved = await saveAgentProfiles(normalizedProfiles);
      const nextWorkspace = await scanWorkspace(workspace.root);
      setConfig(saved);
      setProfileDrafts(saved.agentProfiles);
      onWorkspaceChange(nextWorkspace, "Agent profiles saved.");
      setMessage("Agent profiles saved.");
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const updateProfile = (profileIndex: number, update: (profile: AgentProfile) => AgentProfile) => {
    setProfileDrafts((profiles) =>
      profiles.map((profile, index) => (index === profileIndex ? update(profile) : profile)),
    );
  };

  const addProfile = () => {
    setProfileDrafts((profiles) => {
      return [
        ...profiles,
        {
          id: "",
          name: "",
          skillsDir: "",
          enabled: true,
          builtIn: false,
          linkMode: "symlink" as const,
        },
      ];
    });
  };

  if (!config) {
    return (
      <section className="data-panel settings-grid">
        <PanelHeader title="Settings" detail="Loading" />
        <p className="batch-message">{message || "Loading settings..."}</p>
      </section>
    );
  }

  return (
    <section className="settings-layout">
      <section className="data-panel compact-form settings-grid">
        <PanelHeader title="Workspace" detail={workspace.root} />
        <div className="single-action-grid">
          <label>
            <span>Switch workspace</span>
            <input
              onChange={(event) => setWorkspaceDraft(event.target.value)}
              value={workspaceDraft}
              placeholder="copy skills repo path, example: /home/usr/Skills-repo"
            />
          </label>
          <button
            className="primary-button"
            disabled={busy}
            onClick={() => switchWorkspace(workspaceDraft)}
            type="button"
          >
            Add
          </button>
        </div>
        {config.recentWorkspaces.length > 0 && (
          <div className="recent-workspace-list">
            <span className="setting-label">Recent</span>
            {config.recentWorkspaces.map((path) => (
              <button
                className="text-button"
                disabled={busy}
                key={path}
                onClick={() => switchWorkspace(path)}
                type="button"
              >
                {path}
              </button>
            ))}
          </div>
        )}
      </section>

      <section className="data-panel compact-form settings-grid">
        <PanelHeader title="Preferences" detail="" />
        <div className="settings-form-grid">
          <label>
            <span>Theme</span>
            <select
              onChange={(event) =>
                updateConfig((current) => ({
                  ...current,
                  uiPreferences: {
                    ...current.uiPreferences,
                    theme: event.target.value as UserConfig["uiPreferences"]["theme"],
                  },
                }))
              }
              value={config.uiPreferences.theme}
            >
              <option value="system">System</option>
              <option value="dark">Dark</option>
              <option value="light">Light</option>
            </select>
          </label>
          <label>
            <span>Project sort</span>
            <select
              onChange={(event) =>
                updateConfig((current) => ({
                  ...current,
                  uiPreferences: {
                    ...current.uiPreferences,
                    projectSort: event.target.value as UserConfig["uiPreferences"]["projectSort"],
                  },
                }))
              }
              value={config.uiPreferences.projectSort}
            >
              <option value="name">Name</option>
              <option value="updated">Updated</option>
              <option value="skill_count">Skill count</option>
            </select>
          </label>
        </div>
        <div className="settings-check-grid">
          <label className="inline-check">
            <input
              checked={config.uiPreferences.showHiddenProjects}
              onChange={(event) =>
                updateConfig((current) => ({
                  ...current,
                  uiPreferences: {
                    ...current.uiPreferences,
                    showHiddenProjects: event.target.checked,
                  },
                }))
              }
              type="checkbox"
            />
            <span>Show hidden projects by default</span>
          </label>
          <label className="inline-check">
            <input
              checked={config.automaticChecks.enabled}
              onChange={(event) =>
                updateConfig((current) => ({
                  ...current,
                  automaticChecks: { ...current.automaticChecks, enabled: event.target.checked },
                }))
              }
              type="checkbox"
            />
            <span>Enable automatic checks</span>
          </label>
        </div>
        {config.automaticChecks.enabled && (
          <div className="settings-form-grid">
            <label>
              <span>Check interval (days)</span>
              <input
                min={1}
                step={1}
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    automaticChecks: {
                      ...current.automaticChecks,
                      intervalMinutes: Number(event.target.value) * 1440,
                    },
                  }))
                }
                type="number"
                value={Math.round(config.automaticChecks.intervalMinutes / 1440) || 1}
              />
            </label>
            <label className="inline-check">
              <input
                checked={config.automaticChecks.pullAfterCheck}
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    automaticChecks: {
                      ...current.automaticChecks,
                      pullAfterCheck: event.target.checked,
                    },
                  }))
                }
                type="checkbox"
              />
              <span>Auto-pull after check</span>
            </label>
          </div>
        )}
        <button
          className="primary-button"
          disabled={busy}
          onClick={saveGeneralSettings}
          type="button"
        >
          Save settings
        </button>
      </section>

      <section className="data-panel compact-form settings-grid settings-profiles">
        <PanelHeader title="Agent profiles" detail={`${profileDrafts.length} profiles`} />
        <div className="panel-actions">
          <button className="secondary-button" disabled={busy} onClick={addProfile} type="button">
            Add profile
          </button>
          <button className="primary-button" disabled={busy} onClick={saveProfiles} type="button">
            Save profiles
          </button>
        </div>
        <div className="settings-profile-list">
          {profileDrafts.map((profile, index) => (
            <article className="settings-profile-row" key={`${index}:${profile.id}`}>
              <label>
                <span>Id</span>
                <input
                  disabled={profile.builtIn}
                  onChange={(event) =>
                    updateProfile(index, (item) => ({ ...item, id: event.target.value }))
                  }
                  placeholder="custom-agent"
                  value={profile.id}
                />
              </label>
              <label>
                <span>Name</span>
                <input
                  onChange={(event) =>
                    updateProfile(index, (item) => ({ ...item, name: event.target.value }))
                  }
                  placeholder="Custom Agent"
                  value={profile.name}
                />
              </label>
              <label>
                <span>Skills directory</span>
                <input
                  onChange={(event) =>
                    updateProfile(index, (item) => ({ ...item, skillsDir: event.target.value }))
                  }
                  placeholder="~/.agent_name/skills"
                  value={profile.skillsDir}
                />
              </label>
              <label className="inline-check">
                <input
                  checked={profile.enabled}
                  onChange={(event) =>
                    updateProfile(index, (item) => ({ ...item, enabled: event.target.checked }))
                  }
                  type="checkbox"
                />
                <span>Enabled</span>
              </label>
              {!profile.builtIn && (
                <button
                  className="secondary-button"
                  disabled={busy}
                  onClick={() =>
                    setProfileDrafts((profiles) => profiles.filter((_, i) => i !== index))
                  }
                  type="button"
                >
                  Remove
                </button>
              )}
            </article>
          ))}
        </div>
      </section>
      {message && <p className="batch-message settings-message">{message}</p>}
    </section>
  );
});
