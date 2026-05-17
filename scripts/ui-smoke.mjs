import assert from "node:assert/strict";
import react from "@vitejs/plugin-react";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";

const originalConsoleError = console.error;
console.error = (...args) => {
  if (String(args[0]).includes("WebSocket server error")) {
    return;
  }
  originalConsoleError(...args);
};

const server = await createServer({
  appType: "custom",
  configFile: false,
  logLevel: "error",
  plugins: [react()],
  server: { hmr: false, middlewareMode: true },
});

const noop = () => {};

const workspace = {
  root: "/tmp/skilldock-smoke",
  projects: [
    {
      id: "project-one",
      name: "Project One",
      path: "/tmp/skilldock-smoke/project-one",
      remoteUrl: "https://example.com/project-one.git",
      provider: "unknown",
      branch: "main",
      upstream: "origin/main",
      gitStatus: "behind",
      aheadCount: 0,
      behindCount: 1,
      pullAllEligible: true,
      category: "tools",
      licenseFile: "LICENSE",
      readmeFile: "README.md",
      readmeSummary: "Smoke fixture project.",
      skillCount: 1,
      hidden: false,
      favorite: false,
      tags: ["smoke"],
    },
  ],
  skills: [
    {
      id: "project-one/skills/tdd",
      name: "TDD",
      description: "Drive implementation through tests.",
      sourceProjectId: "project-one",
      relativePath: "project-one/skills/tdd",
      absolutePath: "/tmp/skilldock-smoke/project-one/skills/tdd",
      defaultLinkName: "project-one-tdd",
      hasAssets: false,
      hasScripts: false,
      hasReferences: true,
      installedAgents: [
        {
          agentProfileId: "codex",
          linkName: "project-one-tdd",
          sourcePath: "/tmp/skilldock-smoke/project-one/skills/tdd",
          targetPath: "/tmp/skilldock-smoke-agent/project-one-tdd",
          status: "valid",
        },
        {
          agentProfileId: "claude",
          linkName: "project-one-tdd",
          sourcePath: "/tmp/skilldock-smoke/project-one/skills/tdd",
          targetPath: "/tmp/skilldock-smoke-claude/project-one-tdd",
          status: "valid",
        },
      ],
      lastModified: "2026-05-10T00:00:00Z",
    },
  ],
  agentProfiles: [
    {
      profile: {
        id: "codex",
        name: "Codex",
        skillsDir: "/tmp/skilldock-smoke-agent",
        enabled: true,
        builtIn: true,
        linkMode: "symlink",
      },
      skillsDir: "/tmp/skilldock-smoke-agent",
      exists: true,
      writable: true,
      symlinkCount: 0,
      workspaceLinkCount: 0,
      entries: [],
    },
    {
      profile: {
        id: "claude",
        name: "Claude Code",
        skillsDir: "/tmp/skilldock-smoke-claude",
        enabled: true,
        builtIn: true,
        linkMode: "symlink",
      },
      skillsDir: "/tmp/skilldock-smoke-claude",
      exists: true,
      writable: true,
      symlinkCount: 0,
      workspaceLinkCount: 0,
      entries: [],
    },
  ],
};

const task = {
  id: "task-1",
  workspaceRoot: workspace.root,
  kind: "link_skill",
  status: "succeeded",
  summary: "Linked TDD into Codex.",
  stdout: "link project-one-tdd\n",
  stderr: "",
  projectOutcomes: [],
};

function render(element) {
  return renderToStaticMarkup(element);
}

function assertContains(markup, text, label) {
  assert.ok(markup.includes(text), `${label} should contain '${text}'`);
}

function assertExcludes(markup, text, label) {
  assert.ok(!markup.includes(text), `${label} should not contain '${text}'`);
}

try {
  const appModule = await server.ssrLoadModule("/src/App.tsx");
  const { default: App, WorkspaceSelector } = appModule;
  const { CoreView } = await server.ssrLoadModule("/src/CoreView.tsx");
  const { catalogActionState, normalizeProfileDrafts, validateProfileDrafts } =
    await server.ssrLoadModule("/src/views/SettingsView.tsx");
  const { buildLatestProjectErrorIndex, ProjectImportDialog } = await server.ssrLoadModule(
    "/src/views/ProjectsView.tsx",
  );
  const { applyThemePreference, mergeTaskRecords, preserveLogs } =
    await server.ssrLoadModule("/src/lib/shared.tsx");
  const { openWorkspacePathWithCopyFallback } = await server.ssrLoadModule(
    "/src/lib/openPathFallback.ts",
  );
  const { restoreRecentWorkspace, selectWorkspace } =
    await server.ssrLoadModule("/src/lib/commands.ts");

  assert.equal(await restoreRecentWorkspace(), null);
  await assert.rejects(
    () => selectWorkspace("/tmp/skilldock-smoke"),
    /Tauri desktop bridge is unavailable/,
  );

  const themeRoot = {
    dataset: {},
    style: {
      colorScheme: "",
    },
  };
  applyThemePreference("dark", themeRoot);
  assert.equal(themeRoot.dataset.theme, "dark");
  assert.equal(themeRoot.style.colorScheme, "dark");
  applyThemePreference("system", themeRoot);
  assert.equal(themeRoot.dataset.theme, undefined);
  assert.equal(themeRoot.style.colorScheme, "");

  assert.equal(
    await openWorkspacePathWithCopyFallback({
      copyText: async (text) => {
        assert.equal(text, "/tmp/skilldock-smoke/project-one");
        return "copied";
      },
      label: "Project One",
      openPath: async () => {
        throw new Error("No working file opener on this system.");
      },
      path: "/tmp/skilldock-smoke/project-one",
      workspaceRoot: workspace.root,
    }),
    "Could not open Project One; path copied instead. No working file opener on this system.",
  );

  const app = render(React.createElement(App));
  assertContains(app, 'src="/app-icon.png"', "app shell brand icon");

  const unsyncedCatalogActions = catalogActionState({
    busy: false,
    catalog: {
      activeCount: 1,
      catalogPath: "/tmp/skilldock-smoke/.skilldock/catalog",
      gitSyncAvailable: true,
      localOnly: [],
      localOnlyCount: 0,
      missing: [],
      missingCount: 0,
      repositories: [],
    },
    catalogRestorePending: false,
    catalogRemoteDraft: "",
    workspaceRoot: workspace.root,
  });
  assert.equal(
    unsyncedCatalogActions.publishListDisabled,
    true,
    "publish should stay disabled until a catalog remote is configured",
  );
  assert.equal(
    unsyncedCatalogActions.pullListDisabled,
    true,
    "pull should stay disabled until a catalog remote is configured",
  );

  const syncedCatalogActions = catalogActionState({
    busy: false,
    catalog: {
      activeCount: 1,
      catalogPath: "/tmp/skilldock-smoke/.skilldock/catalog",
      gitRemote: "https://github.com/example/skilldock-catalog.git",
      gitSyncAvailable: true,
      localOnly: [],
      localOnlyCount: 0,
      missing: [{ directoryName: "project-one", id: "project-one", remoteUrl: "https://..." }],
      missingCount: 1,
      repositories: [],
    },
    catalogRestorePending: false,
    catalogRemoteDraft: "https://github.com/example/skilldock-catalog.git",
    workspaceRoot: workspace.root,
  });
  assert.equal(syncedCatalogActions.publishListDisabled, false);
  assert.equal(syncedCatalogActions.pullListDisabled, false);
  assert.equal(syncedCatalogActions.cloneMissingDisabled, false);

  const pendingRestoreCatalogActions = catalogActionState({
    busy: false,
    catalog: {
      activeCount: 1,
      catalogPath: "/tmp/skilldock-smoke/.skilldock/catalog",
      gitRemote: "https://github.com/example/skilldock-catalog.git",
      gitSyncAvailable: true,
      localOnly: [],
      localOnlyCount: 0,
      missing: [{ directoryName: "project-one", id: "project-one", remoteUrl: "https://..." }],
      missingCount: 1,
      repositories: [],
    },
    catalogRestorePending: true,
    catalogRemoteDraft: "https://github.com/example/skilldock-catalog.git",
    workspaceRoot: workspace.root,
  });
  assert.equal(
    pendingRestoreCatalogActions.cloneMissingDisabled,
    true,
    "clone missing should stay disabled while a restore task is already queued",
  );

  const normalizedProfiles = normalizeProfileDrafts([
    {
      id: "",
      name: "Custom Agent",
      skillsDir: "/tmp/custom-agent/skills",
      enabled: true,
      builtIn: false,
      linkMode: "symlink",
    },
  ]);
  assert.equal(normalizedProfiles[0].id, "custom-agent");
  assert.equal(validateProfileDrafts(normalizedProfiles), null);

  const selector = render(
    React.createElement(WorkspaceSelector, {
      message: "Choose a workspace",
      onInputChange: noop,
      onSubmit: noop,
      value: "/tmp/skilldock-smoke",
    }),
  );
  assertContains(selector, "Workspace path", "workspace selector");
  assertContains(selector, "Open workspace", "workspace selector");

  const coreProps = {
    activeView: "Skills",
    focusedTaskId: null,
    onBatchLinkResult: noop,
    onCheckAll: noop,
    onCheckProject: noop,
    onCreateAgentDir: noop,
    onImport: noop,
    onOpenTaskLog: noop,
    onOperationResult: noop,
    onPullAll: noop,
    onPullProject: noop,
    onSetProjectHidden: noop,
    onThemePreferenceChange: noop,
    onTaskChange: noop,
    onWorkspaceChange: noop,
    operationBusy: false,
    taskHistory: [task],
    workspace,
  };

  const skills = render(React.createElement(CoreView, coreProps));
  assertContains(skills, "TDD", "skills smoke");
  assertContains(
    skills,
    '<span class="metric-label">Installs</span><strong>1</strong>',
    "install metric smoke",
  );
  assertContains(skills, "2 installed", "per-agent install smoke");

  const projects = render(
    React.createElement(CoreView, {
      ...coreProps,
      activeView: "Projects",
    }),
  );
  assertContains(projects, "Project One", "scan/project smoke");
  assertContains(projects, "1 behind", "scan/project smoke");
  assertContains(projects, "skill</small>", "scan/project smoke");
  assertContains(projects, "Pull available", "scan/project smoke");
  assertExcludes(projects, "license: LICENSE", "scan/project smoke");

  const importDialog = render(
    React.createElement(ProjectImportDialog, {
      directoryName: "",
      onCancel: noop,
      onDirectoryNameChange: noop,
      onShallowChange: noop,
      onSkillPathChange: noop,
      onSourceChange: noop,
      onSubmit: noop,
      shallow: false,
      skillPath: "",
      source: "",
    }),
  );
  assertContains(importDialog, "Skill path", "project import dialog smoke");

  const missingAgentWorkspace = {
    ...workspace,
    agentProfiles: [
      ...workspace.agentProfiles,
      {
        profile: {
          id: "kiro",
          name: "Kiro",
          skillsDir: "~/.kiro/skills",
          enabled: true,
          builtIn: false,
          linkMode: "symlink",
        },
        skillsDir: "/tmp/skilldock-smoke-kiro",
        exists: false,
        writable: false,
        symlinkCount: 0,
        workspaceLinkCount: 0,
        entries: [],
      },
    ],
  };
  const agentsWithMissingDir = render(
    React.createElement(CoreView, {
      ...coreProps,
      activeView: "Agents",
      workspace: missingAgentWorkspace,
    }),
  );
  assertContains(agentsWithMissingDir, "Create directory", "missing agent directory smoke");

  const agentsAfterDirCreate = render(
    React.createElement(CoreView, {
      ...coreProps,
      activeView: "Agents",
      workspace: {
        ...missingAgentWorkspace,
        agentProfiles: missingAgentWorkspace.agentProfiles.map((state) =>
          state.profile.id === "kiro" ? { ...state, exists: true, writable: true } : state,
        ),
      },
    }),
  );
  assertExcludes(agentsAfterDirCreate, "Create directory", "created agent directory smoke");

  const projectCount = 50000;
  const largeProjectTask = {
    ...task,
    id: "task-large",
    projectOutcomes: Array.from({ length: projectCount }, (_, index) => ({
      projectId: `project-${index}`,
      status: index % 17000 === 0 ? "failed" : "succeeded",
      summary: `project-${index}`,
    })),
  };
  const errorIndexStart = performance.now();
  const errorIndex = buildLatestProjectErrorIndex([largeProjectTask], workspace.root);
  const errorIndexMs = performance.now() - errorIndexStart;
  assert.equal(errorIndex.size, 3);
  assert.equal(errorIndex.get("project-0")?.outcome.status, "failed");
  assert.ok(errorIndexMs < 500, `project error index should stay linear; took ${errorIndexMs}ms`);

  const failedFetch = {
    ...task,
    id: "failed-fetch",
    workspaceRoot: workspace.root,
    projectOutcomes: [
      {
        projectId: "project-one",
        status: "failed",
        summary: "fetch failed",
        error: "fatal: unable to access remote",
      },
    ],
  };
  const successfulFetch = {
    ...task,
    id: "successful-fetch",
    workspaceRoot: workspace.root,
    projectOutcomes: [
      {
        projectId: "project-one",
        status: "succeeded",
        summary: "fetch succeeded",
      },
    ],
  };
  const refreshedErrorIndex = buildLatestProjectErrorIndex(
    [successfulFetch, failedFetch],
    workspace.root,
  );
  assert.equal(refreshedErrorIndex.has("project-one"), false);

  const logs = render(
    React.createElement(CoreView, {
      ...coreProps,
      activeView: "Logs",
      focusedTaskId: task.id,
    }),
  );
  assertContains(logs, "Linked TDD into Codex.", "logs smoke");
  assertContains(logs, "Copy raw", "logs smoke");

  // mergeTaskRecords / preserveLogs: verify status-only polls do not clobber
  // previously-loaded task logs stored in history.
  const existingWithLogs = {
    ...task,
    id: "task-with-logs",
    stdout: "loaded stdout",
    stderr: "loaded stderr",
  };
  const incomingStripped = {
    ...task,
    id: "task-with-logs",
    stdout: "",
    stderr: "",
  };
  const merged = mergeTaskRecords([incomingStripped], [existingWithLogs]);
  assert.equal(merged.length, 1);
  assert.equal(merged[0].stdout, "loaded stdout", "merged stdout should be preserved");
  assert.equal(merged[0].stderr, "loaded stderr", "merged stderr should be preserved");

  const preserved = preserveLogs(existingWithLogs, incomingStripped);
  assert.equal(preserved.stdout, "loaded stdout", "preserveLogs retains stdout");
  assert.equal(preserved.stderr, "loaded stderr", "preserveLogs retains stderr");

  const newerWithLogs = {
    ...task,
    id: "task-with-logs",
    stdout: "fresh stdout",
    stderr: "",
  };
  const mergedNewer = mergeTaskRecords([newerWithLogs], [existingWithLogs]);
  assert.equal(
    mergedNewer[0].stdout,
    "fresh stdout",
    "incoming stdout should override when non-empty",
  );
  assert.equal(
    mergedNewer[0].stderr,
    "loaded stderr",
    "stderr should fall back to existing when incoming is empty",
  );
} finally {
  await server.close();
  console.error = originalConsoleError;
}
