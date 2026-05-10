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
  root: "/tmp/skills-collection-smoke",
  projects: [
    {
      id: "project-one",
      name: "Project One",
      path: "/tmp/skills-collection-smoke/project-one",
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
      absolutePath: "/tmp/skills-collection-smoke/project-one/skills/tdd",
      defaultLinkName: "project-one-tdd",
      hasAssets: false,
      hasScripts: false,
      hasReferences: true,
      installedAgents: [],
      lastModified: "2026-05-10T00:00:00Z",
    },
  ],
  agentProfiles: [
    {
      profile: {
        id: "codex",
        name: "Codex",
        skillsDir: "/tmp/skills-collection-smoke-agent",
        enabled: true,
        builtIn: true,
        linkMode: "symlink",
      },
      skillsDir: "/tmp/skills-collection-smoke-agent",
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

try {
  const { CoreView, WorkspaceSelector } = await server.ssrLoadModule("/src/App.tsx");
  const { restoreRecentWorkspace, selectWorkspace } = await server.ssrLoadModule("/src/lib/commands.ts");

  assert.equal(await restoreRecentWorkspace(), null);
  await assert.rejects(
    () => selectWorkspace("/tmp/skills-collection-smoke"),
    /Tauri desktop bridge is unavailable/,
  );

  const selector = render(
    React.createElement(WorkspaceSelector, {
      message: "Choose a workspace",
      onInputChange: noop,
      onSubmit: noop,
      value: "/tmp/skills-collection-smoke",
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
    onTaskChange: noop,
    onWorkspaceChange: noop,
    taskHistory: [task],
    workspace,
  };

  const skills = render(React.createElement(CoreView, coreProps));
  assertContains(skills, "TDD", "skills smoke");
  assertContains(skills, "Preview", "link preview smoke");

  const projects = render(
    React.createElement(CoreView, {
      ...coreProps,
      activeView: "Projects",
    }),
  );
  assertContains(projects, "Project One", "scan/project smoke");
  assertContains(projects, "0 ahead / 1 behind", "scan/project smoke");

  const logs = render(
    React.createElement(CoreView, {
      ...coreProps,
      activeView: "Tasks / Logs",
      focusedTaskId: task.id,
    }),
  );
  assertContains(logs, "Linked TDD into Codex.", "logs smoke");
  assertContains(logs, "Copy raw", "logs smoke");
} finally {
  await server.close();
  console.error = originalConsoleError;
}
