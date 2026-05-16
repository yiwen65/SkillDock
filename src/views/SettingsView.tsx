import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  initializeCatalogGitSync,
  loadUserConfig,
  loadWorkspaceCatalogSummary,
  patchUserPreferences,
  publishCatalogGitSync,
  pullCatalogGitSync,
  restoreMissingCatalogRepositories,
  saveAgentProfiles,
  scanWorkspace,
  selectWorkspace,
  syncWorkspaceCatalogFromProjects,
} from "../lib/commands";
import { PanelHeader, errorMessage, type ThemePreference } from "../lib/shared";
import type {
  AgentProfile,
  TaskOperationResult,
  UserConfig,
  Workspace,
  WorkspaceCatalogSummary,
} from "../lib/types";

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

function isValidProfileId(id: string) {
  return /^[a-zA-Z0-9._-]+$/.test(id);
}

function profileDraftLabel(profile: AgentProfile, index: number) {
  return profile.name.trim() || `Profile ${index + 1}`;
}

function slugifyProfileId(value: string) {
  const slug = value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/^[._-]+|[._-]+$/g, "")
    .slice(0, 48);
  if (!slug || slug === "." || slug === ".." || slug === ".git") return "custom-agent";
  return slug;
}

function inferProfileId(profile: AgentProfile, index: number) {
  const existing = profile.id.trim();
  if (existing && existing === profile.id && isValidProfileId(existing)) return existing;
  const skillsDirName = profile.skillsDir
    .trim()
    .replace(/\\/g, "/")
    .split("/")
    .filter(Boolean)
    .pop();
  return slugifyProfileId(profile.name || skillsDirName || `custom-agent-${index + 1}`);
}

export function normalizeProfileDrafts(profiles: AgentProfile[]) {
  const ids = new Set<string>();
  return profiles.map((profile, index) => {
    const baseId = inferProfileId(profile, index);
    let id = baseId;
    let suffix = 2;
    while (ids.has(id)) {
      id = `${baseId}-${suffix}`;
      suffix += 1;
    }
    ids.add(id);
    return {
      ...profile,
      id,
      name: profile.name.trim(),
      skillsDir: profile.skillsDir.trim(),
    };
  });
}

export function validateProfileDrafts(profiles: AgentProfile[]) {
  const skillsDirs = new Set<string>();
  for (const [index, profile] of profiles.entries()) {
    const label = profileDraftLabel(profile, index);
    if (!profile.name.trim()) return `${label} requires a name.`;
    const skillsDir = profile.skillsDir.trim();
    if (!isValidProfilePath(skillsDir))
      return `${label} requires an absolute or home-relative skills directory.`;
    const normalizedSkillsDir = normalizeProfilePathForCompare(skillsDir);
    if (skillsDirs.has(normalizedSkillsDir))
      return `Profile skills directory '${skillsDir}' is duplicated.`;
    skillsDirs.add(normalizedSkillsDir);
  }
  return null;
}

export function catalogActionState({
  busy,
  catalog,
  catalogRestorePending,
  catalogRemoteDraft,
  workspaceRoot,
}: {
  busy: boolean;
  catalog: WorkspaceCatalogSummary | null;
  catalogRestorePending: boolean;
  catalogRemoteDraft: string;
  workspaceRoot: string;
}) {
  const hasWorkspace = workspaceRoot.trim().length > 0;
  const hasCatalogRemote = Boolean(catalog?.gitRemote?.trim());
  const hasCatalogRemoteDraft = catalogRemoteDraft.trim().length > 0;
  const canPullOrPublishCatalog =
    hasWorkspace && Boolean(catalog?.gitSyncAvailable) && hasCatalogRemote;

  return {
    cloneMissingDisabled: busy || catalogRestorePending || !hasWorkspace || !catalog?.missingCount,
    initSyncDisabled: busy || !hasWorkspace || !hasCatalogRemoteDraft,
    publishListDisabled: busy || !canPullOrPublishCatalog,
    pullListDisabled: busy || !canPullOrPublishCatalog,
    refreshDisabled: busy || !hasWorkspace,
    remoteInputDisabled: busy,
    saveLocalListDisabled: busy || !hasWorkspace,
  };
}

export const SettingsView = memo(function SettingsView({
  onThemePreferenceChange,
  onOperationResult,
  onWorkspaceChange,
  workspace,
}: {
  onThemePreferenceChange: (theme: ThemePreference) => void;
  onOperationResult: (result: TaskOperationResult) => void;
  onWorkspaceChange: (workspace: Workspace, message: string) => void;
  workspace: Workspace;
}) {
  const [config, setConfig] = useState<UserConfig | null>(null);
  const [workspaceDraft, setWorkspaceDraft] = useState("");
  const [profileDrafts, setProfileDrafts] = useState<AgentProfile[]>(
    workspace.agentProfiles.map((state) => state.profile),
  );
  const [catalog, setCatalog] = useState<WorkspaceCatalogSummary | null>(null);
  const [catalogRemoteDraft, setCatalogRemoteDraft] = useState("");
  const [catalogRestorePending, setCatalogRestorePending] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // Ref guard backs up the `busy` state so a rapid second click landing in the
  // tiny window before React applies the disabled prop cannot fire an extra
  // backend call.
  const busyRef = useRef(false);
  const workspaceProjectSignature = useMemo(
    () => workspace.projects.map((project) => `${project.id}:${project.remoteUrl || ""}`).join("|"),
    [workspace.projects],
  );
  const catalogActions = catalogActionState({
    busy,
    catalog,
    catalogRestorePending,
    catalogRemoteDraft,
    workspaceRoot: workspace.root,
  });

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
    setCatalog(null);
    setCatalogRemoteDraft("");
    if (!workspace.root) return;
    let cancelled = false;
    loadWorkspaceCatalogSummary(workspace.root)
      .then((summary) => {
        if (cancelled) return;
        setCatalog(summary);
        setCatalogRemoteDraft(summary.gitRemote || "");
        setCatalogRestorePending(false);
      })
      .catch((error) => {
        if (!cancelled) setMessage(errorMessage(error));
      });
    return () => {
      cancelled = true;
    };
  }, [workspace.root, workspaceProjectSignature]);

  const updateConfig = (update: (current: UserConfig) => UserConfig) => {
    if (!config) return;
    setConfig(update(config));
  };

  const updateThemePreference = (theme: ThemePreference) => {
    updateConfig((current) => ({
      ...current,
      uiPreferences: {
        ...current.uiPreferences,
        theme,
      },
    }));
    onThemePreferenceChange(theme);
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
      onThemePreferenceChange(saved.uiPreferences.theme);
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
    const normalizedProfiles = normalizeProfileDrafts(profileDrafts);
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

  const refreshCatalog = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setMessage("Loading catalog...");
    try {
      const summary = await loadWorkspaceCatalogSummary(workspace.root);
      setCatalog(summary);
      setCatalogRemoteDraft(summary.gitRemote || "");
      setCatalogRestorePending(false);
      setMessage("Catalog refreshed.");
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const saveCatalogFromProjects = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setMessage("Saving local project list to catalog...");
    try {
      const summary = await syncWorkspaceCatalogFromProjects(workspace.root);
      setCatalog(summary);
      setMessage(`Catalog saved with ${summary.activeCount} active repositories.`);
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const restoreCatalogRepositories = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setMessage("Restoring missing catalog repositories...");
    try {
      const result = await restoreMissingCatalogRepositories(workspace.root);
      onOperationResult(result);
      setCatalogRestorePending(true);
      setMessage(`${result.task.summary} queued. Watch Logs for progress.`);
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const initializeCatalogSync = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setMessage("Initializing catalog Git sync...");
    try {
      const result = await initializeCatalogGitSync(
        workspace.root,
        catalogRemoteDraft.trim() || undefined,
      );
      const summary = await loadWorkspaceCatalogSummary(workspace.root);
      setCatalog(summary);
      setMessage(result.summary);
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const pullCatalog = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setMessage("Pulling catalog updates...");
    try {
      const result = await pullCatalogGitSync(workspace.root);
      const nextWorkspace = await scanWorkspace(workspace.root);
      const summary = await loadWorkspaceCatalogSummary(workspace.root);
      setCatalog(summary);
      onWorkspaceChange(nextWorkspace, result.summary);
      setMessage(result.summary);
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const publishCatalog = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setMessage("Publishing catalog updates...");
    try {
      const result = await publishCatalogGitSync(workspace.root);
      const summary = await loadWorkspaceCatalogSummary(workspace.root);
      setCatalog(summary);
      setMessage(result.summary);
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
        <PanelHeader
          title="Catalog sync"
          detail={
            catalog
              ? `${catalog.activeCount} tracked, ${catalog.missingCount} missing`
              : "Not loaded"
          }
        />
        <div className="catalog-stat-grid">
          <div>
            <span className="setting-label">Tracked</span>
            <strong>{catalog?.activeCount ?? 0}</strong>
          </div>
          <div>
            <span className="setting-label">Missing here</span>
            <strong>{catalog?.missingCount ?? 0}</strong>
          </div>
          <div>
            <span className="setting-label">Local only</span>
            <strong>{catalog?.localOnlyCount ?? 0}</strong>
          </div>
        </div>
        <label>
          <span>Catalog remote</span>
          <input
            onChange={(event) => setCatalogRemoteDraft(event.target.value)}
            placeholder="git@github.com:you/skilldock-catalog.git"
            value={catalogRemoteDraft}
            disabled={catalogActions.remoteInputDisabled}
          />
        </label>
        <div className="panel-actions">
          <button
            className="secondary-button"
            disabled={catalogActions.refreshDisabled}
            onClick={refreshCatalog}
            type="button"
          >
            Refresh
          </button>
          <button
            className="secondary-button"
            disabled={catalogActions.saveLocalListDisabled}
            onClick={saveCatalogFromProjects}
            type="button"
          >
            Save local list
          </button>
          <button
            className="secondary-button"
            disabled={catalogActions.cloneMissingDisabled}
            onClick={restoreCatalogRepositories}
            type="button"
          >
            Clone missing
          </button>
        </div>
        <div className="panel-actions">
          <button
            className="secondary-button"
            disabled={catalogActions.initSyncDisabled}
            onClick={initializeCatalogSync}
            type="button"
          >
            Init sync
          </button>
          <button
            className="secondary-button"
            disabled={catalogActions.pullListDisabled}
            onClick={pullCatalog}
            type="button"
          >
            Pull list
          </button>
          <button
            className="primary-button"
            disabled={catalogActions.publishListDisabled}
            onClick={publishCatalog}
            type="button"
          >
            Publish list
          </button>
        </div>
        {catalog?.gitRemote && <p className="batch-message">Remote: {catalog.gitRemote}</p>}
        {catalog?.missing.length ? (
          <div className="catalog-list">
            <span className="setting-label">Missing repositories</span>
            {catalog.missing.slice(0, 5).map((item) => (
              <span key={item.id}>{item.directoryName}</span>
            ))}
          </div>
        ) : null}
        {catalog?.localOnly.length ? (
          <div className="catalog-list">
            <span className="setting-label">Local repositories not in catalog</span>
            {catalog.localOnly.slice(0, 5).map((item) => (
              <span key={item.id}>{item.directoryName}</span>
            ))}
          </div>
        ) : null}
      </section>

      <section className="data-panel compact-form settings-grid">
        <PanelHeader title="Preferences" detail="" />
        <div className="settings-form-grid">
          <label>
            <span>Theme</span>
            <select
              onChange={(event) => updateThemePreference(event.target.value as ThemePreference)}
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
