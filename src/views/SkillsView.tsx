import { useEffect, useMemo, useRef, useState } from "react";
import {
  linkSkill,
  linkSkillsBatch,
  previewLinkSkill,
  previewLinkSkillsBatch,
  previewUnlinkSkill,
  readSkillMarkdownPreview,
  unlinkSkill,
} from "../lib/commands";
import { renderMarkdown } from "../lib/format";
import { copyTextWithFallback, openWorkspacePathWithCopyFallback } from "../lib/openPathFallback";
import { EmptyState, PanelHeader, errorMessage } from "../lib/shared";
import type {
  AgentProfileState,
  BatchLinkOperationResult,
  Skill,
  TaskOperationResult,
  Workspace,
} from "../lib/types";

function batchWorkspaceKey(workspace: Workspace) {
  return JSON.stringify({
    profiles: workspace.agentProfiles
      .map((state) => ({
        exists: state.exists,
        id: state.profile.id,
        skillsDir: state.skillsDir,
        writable: state.writable,
      }))
      .sort((left, right) => left.id.localeCompare(right.id)),
    root: workspace.root,
    skills: workspace.skills
      .map((skill) => ({
        id: skill.id,
        path: skill.absolutePath,
        linkName: skill.defaultLinkName,
      }))
      .sort((left, right) => left.id.localeCompare(right.id)),
  });
}

function profileName(profiles: AgentProfileState[], profileId: string) {
  return profiles.find((state) => state.profile.id === profileId)?.profile.name || profileId;
}

export function SkillsView({
  onBatchLinkResult,
  onOperationResult,
  workspace,
}: {
  onBatchLinkResult: (result: BatchLinkOperationResult) => void;
  onOperationResult: (result: TaskOperationResult) => void;
  workspace: Workspace;
}) {
  const { agentProfiles, projects, skills } = workspace;
  const [query, setQuery] = useState("");
  const [projectFilter, setProjectFilter] = useState("all");
  const [agentFilter, setAgentFilter] = useState("all");
  const [selectedSkillId, setSelectedSkillId] = useState(skills[0]?.id ?? "");
  const [selectedSkillIds, setSelectedSkillIds] = useState<string[]>([]);
  const [selectedProfileIds, setSelectedProfileIds] = useState<string[]>([]);
  const [singleProfileId, setSingleProfileId] = useState(agentProfiles[0]?.profile.id ?? "");
  const [detailMessage, setDetailMessage] = useState<string | null>(null);
  const [markdownPreview, setMarkdownPreview] = useState<string | null>(null);
  const [batchMessage, setBatchMessage] = useState<string | null>(null);
  const [executeBusy, setExecuteBusy] = useState(false);
  const [singleBusy, setSingleBusy] = useState(false);
  const singleBusyRef = useRef(false);
  // Guards rapid repeat clicks on detail-panel async buttons (Open, Copy path,
  // Preview) so an in-flight command cannot be stacked by double clicks.
  const detailActionBusyRef = useRef(false);
  const [detailActionBusy, setDetailActionBusy] = useState(false);

  const projectById = useMemo(
    () => new Map(projects.map((project) => [project.id, project])),
    [projects],
  );
  const selectedSkill = skills.find((skill) => skill.id === selectedSkillId) ?? skills[0];
  const selectedProject = selectedSkill
    ? projectById.get(selectedSkill.sourceProjectId)
    : undefined;
  const selectedInstalledProfiles = selectedSkill
    ? selectedSkill.installedAgents.map((install) => install.agentProfileId)
    : [];

  const busy = executeBusy || singleBusy;
  const selectedPairCount = selectedSkillIds.length * selectedProfileIds.length;
  const installedPairSet = useMemo(() => {
    const set = new Set<string>();
    for (const skill of skills) {
      for (const install of skill.installedAgents) {
        set.add(`${skill.id}:${install.agentProfileId}`);
      }
    }
    return set;
  }, [skills]);
  const batchNewPairCount = useMemo(() => {
    let count = 0;
    for (const skillId of selectedSkillIds) {
      for (const profileId of selectedProfileIds) {
        if (!installedPairSet.has(`${skillId}:${profileId}`)) {
          count += 1;
        }
      }
    }
    return count;
  }, [installedPairSet, selectedProfileIds, selectedSkillIds]);
  const batchAlreadyInstalledCount = selectedPairCount - batchNewPairCount;
  const workspaceKey = useMemo(() => batchWorkspaceKey(workspace), [workspace]);

  // Reset detail message when selection changes
  useEffect(() => {
    setDetailMessage(null);
  }, [singleProfileId, selectedSkill?.id]);

  useEffect(() => {
    setBatchMessage(null);
    setDetailMessage(null);
    if (selectedSkillId && !skills.some((skill) => skill.id === selectedSkillId)) {
      setSelectedSkillId(skills[0]?.id ?? "");
    } else if (!selectedSkillId && skills[0]) {
      setSelectedSkillId(skills[0].id);
    }
    if (singleProfileId && !agentProfiles.some((state) => state.profile.id === singleProfileId)) {
      setSingleProfileId(agentProfiles[0]?.profile.id ?? "");
    } else if (!singleProfileId && agentProfiles[0]) {
      setSingleProfileId(agentProfiles[0].profile.id);
    }
  }, [agentProfiles, singleProfileId, selectedSkillId, skills, workspaceKey]);

  const projectOptions = useMemo(
    () => Array.from(new Set(skills.map((skill) => skill.sourceProjectId))).sort(),
    [skills],
  );
  // Counts per project and per install-status so the filter dropdowns can
  // communicate their discriminating power (matches ProjectsView behaviour).
  const projectCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const skill of skills) {
      counts.set(skill.sourceProjectId, (counts.get(skill.sourceProjectId) ?? 0) + 1);
    }
    return counts;
  }, [skills]);
  const installStatusCounts = useMemo(() => {
    let installed = 0;
    let notInstalled = 0;
    const byStatus = new Map<string, number>();
    for (const skill of skills) {
      if (skill.installedAgents.length > 0) {
        installed += 1;
        for (const install of skill.installedAgents) {
          byStatus.set(install.status, (byStatus.get(install.status) ?? 0) + 1);
        }
      } else {
        notInstalled += 1;
      }
    }
    return { installed, notInstalled, byStatus };
  }, [skills]);
  const filteredSkills = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    return skills.filter((skill) => {
      const installed = skill.installedAgents.length > 0;
      if (projectFilter !== "all" && skill.sourceProjectId !== projectFilter) return false;
      if (agentFilter === "installed" && !installed) return false;
      if (agentFilter === "not-installed" && installed) return false;
      if (
        !["all", "installed", "not-installed"].includes(agentFilter) &&
        !skill.installedAgents.some((install) => install.status === agentFilter)
      )
        return false;
      if (!normalizedQuery) return true;
      return [skill.name, skill.description, skill.relativePath, skill.absolutePath]
        .filter(Boolean)
        .some((value) => value!.toLowerCase().includes(normalizedQuery));
    });
  }, [agentFilter, projectFilter, query, skills]);

  const toggleValue = (kind: "skill" | "profile", value: string) => {
    const selected = kind === "skill" ? selectedSkillIds : selectedProfileIds;
    const nextSelected = selected.includes(value)
      ? selected.filter((item) => item !== value)
      : [...selected, value];
    if (kind === "skill") {
      setSelectedSkillIds(nextSelected);
    } else {
      setSelectedProfileIds(nextSelected);
    }
    setBatchMessage(null);
  };

  const selectVisibleSkills = () => {
    setSelectedSkillIds(filteredSkills.map((skill) => skill.id));
    setBatchMessage(null);
  };

  const clearSelection = () => {
    setSelectedSkillIds([]);
    setSelectedProfileIds([]);
    setBatchMessage(null);
  };

  const directSingleInstall = async () => {
    if (singleBusyRef.current) return;
    if (!selectedSkill || !singleProfileId) {
      setDetailMessage("Select a skill and target profile.");
      return;
    }
    singleBusyRef.current = true;
    setSingleBusy(true);
    setDetailMessage("Installing skill...");
    try {
      const preview = await previewLinkSkill(workspace.root, {
        agentProfileId: singleProfileId,
        skillId: selectedSkill.id,
      });
      const result = await linkSkill(workspace.root, { preview });
      onOperationResult(result);
      setDetailMessage(result.task.summary);
    } catch (error) {
      setDetailMessage(errorMessage(error));
    } finally {
      singleBusyRef.current = false;
      setSingleBusy(false);
    }
  };

  const directSingleUninstall = async (agentProfileId: string, linkName: string) => {
    if (singleBusyRef.current || !selectedSkill) return;
    singleBusyRef.current = true;
    setSingleBusy(true);
    setDetailMessage("Uninstalling skill...");
    try {
      const preview = await previewUnlinkSkill(workspace.root, { agentProfileId, linkName });
      const result = await unlinkSkill(workspace.root, { preview });
      onOperationResult(result);
      setDetailMessage(result.task.summary);
    } catch (error) {
      setDetailMessage(errorMessage(error));
    } finally {
      singleBusyRef.current = false;
      setSingleBusy(false);
    }
  };

  const openPath = async (path: string, label: string) => {
    if (detailActionBusyRef.current) return;
    detailActionBusyRef.current = true;
    setDetailActionBusy(true);
    try {
      setDetailMessage(
        await openWorkspacePathWithCopyFallback({
          label,
          path,
          workspaceRoot: workspace.root,
        }),
      );
    } finally {
      detailActionBusyRef.current = false;
      setDetailActionBusy(false);
    }
  };

  const copyPath = async (path: string) => {
    if (detailActionBusyRef.current) return;
    detailActionBusyRef.current = true;
    setDetailActionBusy(true);
    try {
      try {
        const result = await copyTextWithFallback(path);
        setDetailMessage(result === "copied" ? "Path copied." : "Copy path fallback opened.");
      } catch {
        setDetailMessage("Clipboard is unavailable.");
      }
    } finally {
      detailActionBusyRef.current = false;
      setDetailActionBusy(false);
    }
  };

  const previewSkillMarkdown = async () => {
    if (!selectedSkill) return;
    if (detailActionBusyRef.current) return;
    detailActionBusyRef.current = true;
    setDetailActionBusy(true);
    try {
      const result = await readSkillMarkdownPreview(workspace.root, selectedSkill.id, 200000);
      const raw = result.markdown + (result.truncated ? "\n\n…(truncated)" : "");
      const html = await renderMarkdown(raw);
      setMarkdownPreview(html);
    } catch (error) {
      setDetailMessage(errorMessage(error));
    } finally {
      detailActionBusyRef.current = false;
      setDetailActionBusy(false);
    }
  };

  const directBatchInstall = async () => {
    if (selectedPairCount === 0) {
      setBatchMessage("Select at least one skill and one target profile.");
      return;
    }
    setExecuteBusy(true);
    setBatchMessage("Installing...");
    try {
      const previewResult = await previewLinkSkillsBatch(workspace.root, {
        items: selectedSkillIds.flatMap((skillId) =>
          selectedProfileIds.map((agentProfileId) => ({ skillId, agentProfileId })),
        ),
      });
      const safePreviews = previewResult.previews.filter(
        (preview) => preview.status === "will_link" || preview.status === "already_installed",
      );
      if (safePreviews.length === 0) {
        setBatchMessage("No installable targets found.");
        setExecuteBusy(false);
        return;
      }
      const result = await linkSkillsBatch(workspace.root, { previews: safePreviews });
      onBatchLinkResult(result);
      setBatchMessage(
        `${result.summary.linked} linked, ${result.summary.alreadyInstalled} already installed, ${result.summary.skipped} skipped, ${result.summary.failed} failed.`,
      );
    } catch (error) {
      setBatchMessage(errorMessage(error));
    } finally {
      setExecuteBusy(false);
    }
  };

  if (skills.length === 0) {
    return (
      <EmptyState
        title="No skills found"
        body="Refresh after adding projects with SKILL.md files."
      />
    );
  }

  return (
    <section className="skills-view-grid">
      <section className="data-panel skills-list-panel">
        <PanelHeader title="Skills" detail={`${filteredSkills.length} of ${skills.length}`} />
        <div className="skill-filter-grid">
          <label>
            <span>Search</span>
            <input
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Name, description or path"
              value={query}
            />
          </label>
          <label>
            <span>Project</span>
            <select
              onChange={(event) => setProjectFilter(event.target.value)}
              value={projectFilter}
            >
              <option value="all">All projects ({skills.length})</option>
              {projectOptions.map((projectId) => (
                <option key={projectId} value={projectId}>
                  {projectById.get(projectId)?.name ?? projectId} (
                  {projectCounts.get(projectId) ?? 0})
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>Install status</span>
            <select onChange={(event) => setAgentFilter(event.target.value)} value={agentFilter}>
              <option value="all">All statuses ({skills.length})</option>
              <option value="installed">Installed ({installStatusCounts.installed})</option>
              <option value="not-installed">
                Not installed ({installStatusCounts.notInstalled})
              </option>
              <option value="valid">
                Valid ({installStatusCounts.byStatus.get("valid") ?? 0})
              </option>
              <option value="broken">
                Broken ({installStatusCounts.byStatus.get("broken") ?? 0})
              </option>
              <option value="external">
                External ({installStatusCounts.byStatus.get("external") ?? 0})
              </option>
              <option value="conflict">
                Conflict ({installStatusCounts.byStatus.get("conflict") ?? 0})
              </option>
            </select>
          </label>
        </div>
        <div className="table-list skill-list">
          {filteredSkills.map((skill) => {
            const project = projectById.get(skill.sourceProjectId);
            return (
              <SkillRow
                key={skill.id}
                onSelect={() => setSelectedSkillId(skill.id)}
                onToggle={() => toggleValue("skill", skill.id)}
                project={project}
                selected={skill.id === selectedSkill?.id}
                selectedForBatch={selectedSkillIds.includes(skill.id)}
                skill={skill}
              />
            );
          })}
          {filteredSkills.length === 0 && (
            <p className="batch-message">No skills match the current filters.</p>
          )}
        </div>
      </section>

      <section className="data-panel skill-detail-panel">
        <PanelHeader
          title={selectedSkill?.name ?? "Skill detail"}
          detail={selectedProject?.name ?? ""}
        />
        {selectedSkill && (
          <>
            {selectedSkill.description && (
              <p className="detail-description">{selectedSkill.description}</p>
            )}
            <div className="skill-detail-facts" aria-label="Selected skill summary">
              <span>{selectedProject?.name ?? selectedSkill.sourceProjectId}</span>
              <span>
                {selectedInstalledProfiles.length > 0
                  ? `${selectedInstalledProfiles.length} installed`
                  : "Not installed"}
              </span>
            </div>
            <div className="panel-actions">
              <button
                className="secondary-button"
                disabled={detailActionBusy}
                onClick={() => openPath(selectedSkill.absolutePath, "skill path")}
                type="button"
              >
                Open folder
              </button>
              <button
                className="secondary-button"
                disabled={detailActionBusy}
                onClick={() => copyPath(selectedSkill.absolutePath)}
                type="button"
              >
                Copy path
              </button>
              <button
                className="secondary-button"
                disabled={detailActionBusy}
                onClick={previewSkillMarkdown}
                type="button"
              >
                Preview
              </button>
            </div>
            <div className="single-action-grid">
              <label>
                <span>Install target</span>
                <select
                  onChange={(event) => setSingleProfileId(event.target.value)}
                  value={singleProfileId}
                >
                  {agentProfiles.map((state) => {
                    const alreadyInstalled = selectedInstalledProfiles.includes(state.profile.id);
                    return (
                      <option key={state.profile.id} value={state.profile.id}>
                        {alreadyInstalled
                          ? `${state.profile.name} (installed)`
                          : state.profile.name}
                      </option>
                    );
                  })}
                </select>
              </label>
              <button
                className="primary-button"
                disabled={
                  busy ||
                  !singleProfileId ||
                  selectedInstalledProfiles.includes(singleProfileId) ||
                  agentProfiles.find((s) => s.profile.id === singleProfileId)?.profile.enabled ===
                    false
                }
                onClick={directSingleInstall}
                title={
                  singleProfileId && selectedInstalledProfiles.includes(singleProfileId)
                    ? "Already installed in this agent profile"
                    : undefined
                }
                type="button"
              >
                Install
              </button>
            </div>
            {detailMessage && <p className="batch-message">{detailMessage}</p>}
            <div className="installed-list">
              <h2>Installed agents</h2>
              {selectedSkill.installedAgents.map((install) => (
                <div
                  className="installed-row"
                  key={`${install.agentProfileId}:${install.linkName}`}
                >
                  <div>
                    <strong>{profileName(agentProfiles, install.agentProfileId)}</strong>
                    <span>{install.linkName}</span>
                  </div>
                  <span className="subtle-pill">{install.status}</span>
                  <button
                    className="secondary-button"
                    disabled={busy}
                    onClick={() => directSingleUninstall(install.agentProfileId, install.linkName)}
                    type="button"
                  >
                    Uninstall
                  </button>
                </div>
              ))}
              {selectedSkill.installedAgents.length === 0 && (
                <p className="batch-message">This skill is not installed in any agent profile.</p>
              )}
            </div>
          </>
        )}

        <div className="batch-section">
          <PanelHeader title="Batch install" detail={`${selectedPairCount} targets selected`} />
          <div className="panel-actions">
            <button className="secondary-button" onClick={selectVisibleSkills} type="button">
              Select visible
            </button>
            <button className="secondary-button" onClick={clearSelection} type="button">
              Clear
            </button>
          </div>
          <p className="batch-message">{selectedSkillIds.length} skills selected.</p>
          <div className="check-list">
            {agentProfiles.map((state) => (
              <label className="inline-check" key={state.profile.id}>
                <input
                  checked={selectedProfileIds.includes(state.profile.id)}
                  disabled={!state.profile.enabled}
                  onChange={() => toggleValue("profile", state.profile.id)}
                  type="checkbox"
                />
                <span>{state.profile.name}</span>
              </label>
            ))}
          </div>
          <div className="panel-actions">
            <button
              className="primary-button"
              disabled={busy || selectedPairCount === 0 || batchNewPairCount === 0}
              onClick={directBatchInstall}
              title={
                selectedPairCount > 0 && batchNewPairCount === 0
                  ? "All selected targets are already installed"
                  : undefined
              }
              type="button"
            >
              Install
            </button>
          </div>
          {selectedPairCount > 0 && batchAlreadyInstalledCount > 0 && (
            <p className="batch-message">
              {batchNewPairCount === 0
                ? "All selected targets are already installed."
                : `${batchAlreadyInstalledCount} of ${selectedPairCount} selected targets are already installed and will be skipped.`}
            </p>
          )}
          {batchMessage && <p className="batch-message">{batchMessage}</p>}
        </div>
      </section>

      {markdownPreview !== null && (
        <div
          className="dialog-backdrop"
          role="presentation"
          onClick={() => setMarkdownPreview(null)}
        >
          <div className="data-panel markdown-preview-dialog" onClick={(e) => e.stopPropagation()}>
            <PanelHeader title={`${selectedSkill?.name ?? "Skill"} — SKILL.md`} detail="" />
            <div
              className="markdown-preview-content"
              dangerouslySetInnerHTML={{ __html: markdownPreview }}
            />
            <button
              className="secondary-button"
              onClick={() => setMarkdownPreview(null)}
              type="button"
            >
              Close
            </button>
          </div>
        </div>
      )}
    </section>
  );
}

function SkillRow({
  onSelect,
  onToggle,
  project,
  selected,
  selectedForBatch,
  skill,
}: {
  onSelect: () => void;
  onToggle: () => void;
  project: { name: string } | undefined;
  selected: boolean;
  selectedForBatch: boolean;
  skill: Skill;
}) {
  return (
    <article
      aria-selected={selected}
      className={selected ? "list-row skill-row selected" : "list-row skill-row"}
      onClick={onSelect}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect();
        }
      }}
      role="button"
      tabIndex={0}
    >
      <label className="inline-check" onClick={(event) => event.stopPropagation()}>
        <input
          aria-label={`Select ${skill.name}`}
          checked={selectedForBatch}
          onChange={onToggle}
          type="checkbox"
        />
      </label>
      <div className="skill-row-main">
        <h2>{skill.name}</h2>
        <p>{skill.description || skill.relativePath}</p>
      </div>
      <div className="skill-row-status">
        <strong
          className={skill.installedAgents.length > 0 ? "status-installed" : "status-not-installed"}
        >
          {skill.installedAgents.length > 0
            ? `${skill.installedAgents.length} installed`
            : "Not installed"}
        </strong>
        <span className="skill-repo-name">{project?.name ?? skill.sourceProjectId}</span>
      </div>
    </article>
  );
}
