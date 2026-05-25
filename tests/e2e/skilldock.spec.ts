import { expect, type Page, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  const browserFindings: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error" || message.type() === "warning") {
      browserFindings.push(`${message.type()}: ${message.text()}`);
    }
  });
  page.on("pageerror", (error) => {
    browserFindings.push(`pageerror: ${error.message}`);
  });
  page.on("requestfailed", (request) => {
    browserFindings.push(
      `requestfailed: ${request.method()} ${request.url()} ${request.failure()?.errorText}`,
    );
  });
  page.on("response", (response) => {
    const status = response.status();
    if (status >= 400) {
      browserFindings.push(`response: ${status} ${response.url()}`);
    }
  });
  await page.addInitScript(mockTauriBridge);

  test.info().annotations.push({
    type: "browser-findings",
    description:
      "Console warnings/errors, page errors, failed requests and 4xx/5xx responses fail the test.",
  });

  (page as Page & { __browserFindings?: string[] }).__browserFindings = browserFindings;
});

test.afterEach(async ({ page }) => {
  const findings = (page as Page & { __browserFindings?: string[] }).__browserFindings ?? [];
  expect(findings).toEqual([]);
});

test("selects a workspace and validates the skills-centered MVP flow", async ({ page }) => {
  await page.goto("/");

  const brandIcon = page.locator(".brand-mark img");
  await expect(brandIcon).toBeVisible();
  await expect(brandIcon).toHaveAttribute("src", "/app-icon.png");
  await expect
    .poll(() => brandIcon.evaluate((img) => (img as HTMLImageElement).naturalWidth))
    .toBeGreaterThan(0);

  await expect(page.getByRole("heading", { name: "Select workspace" })).toBeVisible();
  await page.getByRole("button", { name: "Open workspace" }).click();
  await expect(page.getByText("Enter a workspace directory path.")).toBeVisible();

  await page.getByLabel("Workspace path").fill("/tmp/skills-e2e-workspace");
  await page.getByRole("button", { name: "Open workspace" }).click();

  await expect(page.getByRole("heading", { name: "Skills" }).first()).toBeVisible();
  await expect(page.locator(".metric").filter({ hasText: "Projects" })).toContainText("2");
  await expect(page.locator(".metric").filter({ hasText: "Skills" })).toContainText("2");
  await expect(page.locator(".metric").filter({ hasText: "Agents" })).toContainText("3");
  await expect(page.locator(".metric").filter({ hasText: "Installs" })).toContainText("1");

  await expect(page.getByRole("heading", { name: "TDD" }).first()).toBeVisible();
  await expect(page.getByText("Drive implementation through tests.").first()).toBeVisible();
  await expect(page.locator(".skill-detail-facts")).toContainText("Agent Skills");
  await expectSkillRowsToHaveStableLayout(page);
  await expectSkillsFiltersToKeepCompactSpacing(page);
  await expectSkillListToFillPanel(page);

  await page.getByLabel("Search").fill("deploy");
  await expect(page.getByRole("heading", { name: "Deploy Guard" })).toBeVisible();
  await expect(page.getByText("Check production readiness before release.").first()).toBeVisible();

  await page.getByLabel("Search").fill("");
  // Single-install flow: the earlier "Preview install → will_link → Install"
  // three-step UI was collapsed into a single "Install" button that
  // previews + links internally. Scope to .single-action-grid so this
  // doesn't clash with the batch section's own Install button.
  const singleInstall = page
    .locator(".single-action-grid")
    .getByRole("button", { name: "Install", exact: true });
  await expect(singleInstall).toBeEnabled();
  await singleInstall.click();
  await expect(page.getByRole("status")).toContainText("Linked TDD into Claude Code.");

  // The Installs metric counts distinct skills that have at least one
  // install, not total links. TDD was already installed in Codex in the
  // mock, so adding Claude Code does not bump the count — Deploy Guard is
  // still uninstalled. The count goes to 2 only after the batch install
  // below adds Deploy Guard's first install.

  // Batch-install flow: likewise collapsed from preview/execute into one
  // button. Scope to .batch-section for the same disambiguation reason.
  const batchSection = page.locator(".batch-section");
  await batchSection.getByRole("button", { name: "Select visible" }).click();
  await batchSection.getByRole("checkbox", { name: "Codex" }).check();
  await batchSection.getByRole("button", { name: "Install", exact: true }).click();
  await expect(page.getByRole("status")).toContainText(
    "Batch link: 1 linked, 1 already installed, 0 skipped, 0 failed.",
  );
  await expect(page.locator(".metric").filter({ hasText: "Installs" })).toContainText("2");
});

test("covers project import, update controls, filtering and hide metadata", async ({ page }) => {
  await openMockWorkspace(page);
  await page.getByRole("button", { name: "Projects" }).click();

  await expect(page.getByRole("heading", { name: "Projects" }).first()).toBeVisible();
  await expect(page.getByRole("heading", { name: "Agent Skills" })).toBeVisible();
  const agentSkillsProject = page.locator(".project-row").filter({ hasText: "Agent Skills" });
  await expect(agentSkillsProject).toContainText("2");
  await expect(agentSkillsProject).toContainText("skills");
  await expect(agentSkillsProject).toContainText("2 behind");
  await expect(agentSkillsProject).toContainText("Pull available");
  await expect(agentSkillsProject).not.toContainText("license: LICENSE");
  await expect(agentSkillsProject).not.toContainText("origin/main");

  await page.getByRole("button", { name: "Import" }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.getByLabel("Repository").fill("openai/codex");
  await page.getByLabel("Directory name").fill("codex-skill");
  await page.getByLabel("Shallow clone").check();
  await page.getByRole("dialog").getByRole("button", { name: "Import" }).click();

  await expect(page.getByText("Imported openai/codex into codex-skill.")).toBeVisible();
  await expect(page.getByRole("heading", { name: "codex-skill" })).toBeVisible();

  await page.getByRole("button", { name: "Check all" }).click();
  await expect(page.getByText("Checked 3 projects.")).toBeVisible();

  await page.getByRole("button", { name: "Pull remote" }).click();
  await expect(page.getByText("Pulled 3 eligible projects.")).toBeVisible();

  const agentSkillsRow = page.locator(".project-row").filter({ hasText: "Agent Skills" });
  await agentSkillsRow.getByRole("button", { name: "Check" }).click();
  await expect(page.getByText("Checked agent-skills.")).toBeVisible();
  await agentSkillsRow.getByRole("button", { name: "Pull" }).click();
  await expect(page.getByText("Pulled agent-skills.")).toBeVisible();

  await page.getByLabel("Filter").fill("agent");
  await expect(page.getByRole("heading", { name: "Agent Skills" })).toBeVisible();
  await page.getByRole("button", { name: "Hide" }).click();
  await expect(page.getByText("Agent Skills is hidden.")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Agent Skills" })).toBeHidden();
});

test("covers agent profile creation, missing directory creation and safe unlink", async ({
  page,
}) => {
  await openMockWorkspace(page);
  await page.getByRole("button", { name: "Agents" }).click();

  await expect(page.getByRole("heading", { name: "Agents" }).first()).toBeVisible();
  await expect(page.getByRole("heading", { name: "Missing Agent" })).toBeVisible();
  await page.getByRole("button", { name: "Create directory" }).click();
  // After the directory exists the Create-directory button disappears from
  // the row (the button is conditionally rendered on !state.exists). Assert
  // that behavioural fact rather than the old textual "exists" badge, which
  // the UI no longer renders.
  const missingAgentRow = page.locator(".agent-row").filter({ hasText: "Missing Agent" });
  await expect(missingAgentRow.getByRole("button", { name: "Create directory" })).toHaveCount(0);

  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("button", { name: "Add profile" }).click();
  const aiderProfileRow = page.locator(".settings-profile-row").last();
  await aiderProfileRow.getByLabel("Name").fill("Aider");
  await aiderProfileRow.getByLabel("Skills directory").fill("/tmp/e2e-aider");
  await page.getByRole("button", { name: "Save profiles" }).click();
  await expect(page.getByRole("status")).toContainText("Agent profiles saved.");
  await page.getByRole("button", { name: "Agents" }).click();
  await expect(page.getByRole("heading", { name: "Aider" })).toBeVisible();

  // Uninstall flow: the earlier "Preview uninstall → Execute uninstall"
  // two-step UI was collapsed into a single "Uninstall" button in each
  // linked-skill row. In the mock, only TDD is linked (into Codex), so
  // .first() unambiguously hits that row's Uninstall control.
  const codexAgentRow = page.locator(".agent-row").filter({ hasText: "Codex" });
  await codexAgentRow.getByRole("button", { name: /linked skills for Codex/ }).click();
  await page.getByRole("button", { name: "Uninstall", exact: true }).first().click();
  await expect(page.getByRole("status")).toContainText("Unlinked agent-skills-tdd from Codex.");
  await expect(page.getByText("No workspace skills linked.").first()).toBeVisible();
});

test("covers task logs and settings persistence", async ({ page }) => {
  await openMockWorkspace(page);
  // Nav and panel header are both labelled just "Logs" now (the old
  // "Tasks / Logs" label was dropped when the view was renamed).
  await page.getByRole("button", { name: "Logs" }).click();

  await expect(page.getByRole("heading", { name: "Logs" }).first()).toBeVisible();
  await expect(page.getByText("Initial workspace scan completed.")).toBeVisible();
  await page.getByRole("button", { name: "Expand logs" }).click();
  await expect(page.getByText("scan complete", { exact: true })).toBeVisible();

  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByRole("heading", { name: "Workspace" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Preferences" })).toBeVisible();
  await expectAgentProfilesToAvoidHorizontalOverflow(page);
  await page.getByLabel("Project sort").selectOption("updated");
  // The interval field is only rendered when "Enable automatic checks" is
  // on, and its unit switched from minutes to days (intervalMinutes patch
  // value is `days * 1440`). 1 day → 1440 minutes, matching the check below.
  await page.getByLabel("Enable automatic checks").check();
  await page.getByLabel("Check interval (days)").fill("1");
  await page.getByRole("button", { name: "Save settings" }).click();
  await expect(page.getByText("Settings saved.")).toBeVisible();

  const calls = await page.evaluate(() => window.__e2eCalls);
  expect(calls.some((call) => call.command === "patch_user_preferences_command")).toBe(true);
  expect(
    calls.some(
      (call) =>
        call.command === "patch_user_preferences_command" &&
        call.args.patch.uiPreferences.theme === "system" &&
        call.args.patch.uiPreferences.projectSort === "updated" &&
        call.args.patch.automaticChecks.intervalMinutes === 1440,
    ),
  ).toBe(true);
});

async function openMockWorkspace(page: Page) {
  await page.goto("/");
  await page.getByLabel("Workspace path").fill("/tmp/skills-e2e-workspace");
  await page.getByRole("button", { name: "Open workspace" }).click();
  await expect(page.getByRole("heading", { name: "Skills" }).first()).toBeVisible();
}

async function expectSkillRowsToHaveStableLayout(page: Page) {
  const findings = await page.locator(".skill-row").evaluateAll((rows) =>
    rows.flatMap((row, index) => {
      const article = row as HTMLElement;
      const title = article.querySelector("h2");
      const description = article.querySelector("p");
      const status = article.querySelector(".skill-row-status");
      const rowRect = article.getBoundingClientRect();
      const titleRect = title?.getBoundingClientRect();
      const descriptionRect = description?.getBoundingClientRect();
      const statusRect = status?.getBoundingClientRect();
      const nextRect = rows[index + 1]?.getBoundingClientRect();
      const rowFindings: string[] = [];

      if (rowRect.height < 64) {
        rowFindings.push(`row ${index} is too short: ${rowRect.height}`);
      }
      if (titleRect && descriptionRect && descriptionRect.top < titleRect.bottom - 1) {
        rowFindings.push(`row ${index} title overlaps description`);
      }
      if (titleRect && statusRect && statusRect.bottom < titleRect.top - 1) {
        rowFindings.push(`row ${index} status is detached from row content`);
      }
      if (nextRect && nextRect.top < rowRect.bottom - 1) {
        rowFindings.push(`row ${index} overlaps next row`);
      }

      return rowFindings;
    }),
  );

  expect(findings).toEqual([]);
}

async function expectSkillsFiltersToKeepCompactSpacing(page: Page) {
  await page.setViewportSize({ width: 1194, height: 868 });
  const findings = await page
    .locator(".skills-view-grid > .data-panel")
    .first()
    .evaluate((panel) => {
      const root = panel as HTMLElement;
      const header = root.querySelector(".panel-header");
      const filters = root.querySelector(".skill-filter-grid");
      const search = root.querySelector(".skill-filter-grid label:nth-child(1)");
      const project = root.querySelector(".skill-filter-grid label:nth-child(2)");
      const status = root.querySelector(".skill-filter-grid label:nth-child(3)");
      const rows = [
        ["header to filters", header, filters, 32],
        ["search to project", search, project, 32],
        ["project to status", project, status, 32],
      ] as const;

      return rows.flatMap(([label, before, after, maxGap]) => {
        if (!before || !after) {
          return [`${label} missing element`];
        }

        const gap = after.getBoundingClientRect().top - before.getBoundingClientRect().bottom;
        return gap > maxGap ? [`${label} gap is ${gap}`] : [];
      });
    });

  expect(findings).toEqual([]);
}

async function expectSkillListToFillPanel(page: Page) {
  const findings = await page.locator(".skills-list-panel").evaluate((panel) => {
    const list = panel.querySelector(".skill-list");
    if (!list) {
      return ["skill list missing"];
    }

    const panelRect = (panel as HTMLElement).getBoundingClientRect();
    const listRect = (list as HTMLElement).getBoundingClientRect();
    const panelGap = panelRect.bottom - listRect.bottom;
    const nextFindings: string[] = [];

    if (panelGap > 24) {
      nextFindings.push(`skill list leaves ${panelGap}px below it`);
    }

    return nextFindings;
  });

  expect(findings).toEqual([]);
}

async function expectAgentProfilesToAvoidHorizontalOverflow(page: Page) {
  const findings = await page.locator(".settings-profiles").evaluate((panel) => {
    const profilePanel = panel as HTMLElement;
    const panelRect = profilePanel.getBoundingClientRect();
    const rows = [...profilePanel.querySelectorAll(".settings-profile-row")] as HTMLElement[];
    const tolerance = 1;
    const rowFindings = rows.flatMap((row, index) => {
      const rect = row.getBoundingClientRect();
      const messages: string[] = [];
      if (row.scrollWidth > row.clientWidth + tolerance) {
        messages.push(
          `profile row ${index} scrollWidth ${row.scrollWidth} exceeds ${row.clientWidth}`,
        );
      }
      if (rect.right > panelRect.right + tolerance) {
        messages.push(`profile row ${index} extends ${rect.right - panelRect.right}px past panel`);
      }
      return messages;
    });

    if (profilePanel.scrollWidth > profilePanel.clientWidth + tolerance) {
      rowFindings.push(
        `profile panel scrollWidth ${profilePanel.scrollWidth} exceeds ${profilePanel.clientWidth}`,
      );
    }

    return rowFindings;
  });

  expect(findings).toEqual([]);
}

function mockTauriBridge() {
  type Project = {
    id: string;
    name: string;
    path: string;
    remoteUrl?: string;
    provider: string;
    branch?: string;
    upstream?: string;
    gitStatus: string;
    aheadCount: number;
    behindCount: number;
    pullAllEligible: boolean;
    category: string;
    licenseFile?: string;
    readmeFile?: string;
    readmeSummary?: string;
    skillCount: number;
    hidden: boolean;
    favorite: boolean;
    tags: string[];
    notes?: string;
  };
  type Skill = {
    id: string;
    name: string;
    description?: string;
    sourceProjectId: string;
    relativePath: string;
    absolutePath: string;
    defaultLinkName: string;
    hasAssets: boolean;
    hasScripts: boolean;
    hasReferences: boolean;
    installedAgents: Array<{
      agentProfileId: string;
      linkName: string;
      targetPath: string;
      sourcePath: string;
      status: string;
    }>;
    lastModified?: string;
  };
  type AgentProfile = {
    id: string;
    name: string;
    skillsDir: string;
    enabled: boolean;
    builtIn: boolean;
    linkMode: string;
  };
  type AgentProfileState = {
    profile: AgentProfile;
    skillsDir: string;
    exists: boolean;
    writable: boolean;
    symlinkCount: number;
    workspaceLinkCount: number;
    entries: Array<unknown>;
  };
  type TaskRecord = {
    id: string;
    workspaceRoot: string;
    kind: string;
    status: string;
    summary: string;
    stdout: string;
    stderr: string;
    projectOutcomes: Array<unknown>;
  };

  const root = "/tmp/skills-e2e-workspace";
  const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value));
  const calls: Array<{ command: string; args: Record<string, unknown> }> = [];
  let taskId = 0;

  const profiles: AgentProfileState[] = [
    profileState({
      id: "claude",
      name: "Claude Code",
      skillsDir: "/tmp/e2e-claude",
      enabled: true,
      builtIn: true,
      linkMode: "symlink",
    }),
    profileState({
      id: "codex",
      name: "Codex",
      skillsDir: "/tmp/e2e-codex",
      enabled: true,
      builtIn: true,
      linkMode: "symlink",
    }),
    profileState(
      {
        id: "missing",
        name: "Missing Agent",
        skillsDir: "/tmp/e2e-missing",
        enabled: true,
        builtIn: false,
        linkMode: "symlink",
      },
      false,
    ),
  ];

  const workspace: {
    root: string;
    projects: Project[];
    skills: Skill[];
    agentProfiles: AgentProfileState[];
  } = {
    root,
    projects: [
      {
        id: "agent-skills",
        name: "Agent Skills",
        path: `${root}/agent-skills`,
        remoteUrl: "https://github.com/example/agent-skills.git",
        provider: "github",
        branch: "main",
        upstream: "origin/main",
        gitStatus: "behind",
        aheadCount: 0,
        behindCount: 2,
        pullAllEligible: true,
        category: "skills",
        licenseFile: "LICENSE",
        readmeFile: "README.md",
        readmeSummary: "Reusable agent skills for development workflows.",
        skillCount: 2,
        hidden: false,
        favorite: false,
        tags: ["core"],
      },
      {
        id: "plugin-lab",
        name: "Plugin Lab",
        path: `${root}/plugin-lab`,
        remoteUrl: "git@github.com:example/plugin-lab.git",
        provider: "github",
        branch: "main",
        upstream: undefined,
        gitStatus: "no_upstream",
        aheadCount: 0,
        behindCount: 0,
        pullAllEligible: true,
        category: "plugins",
        readmeSummary: "Plugin experiments.",
        skillCount: 0,
        hidden: false,
        favorite: false,
        tags: ["plugins"],
      },
    ],
    skills: [
      {
        id: "agent-skills/skills/tdd",
        name: "TDD",
        description: "Drive implementation through tests.",
        sourceProjectId: "agent-skills",
        relativePath: "agent-skills/skills/tdd",
        absolutePath: `${root}/agent-skills/skills/tdd`,
        defaultLinkName: "agent-skills-tdd",
        hasAssets: false,
        hasScripts: false,
        hasReferences: true,
        installedAgents: [
          {
            agentProfileId: "codex",
            linkName: "agent-skills-tdd",
            targetPath: "/tmp/e2e-codex/agent-skills-tdd",
            sourcePath: `${root}/agent-skills/skills/tdd`,
            status: "valid",
          },
        ],
        lastModified: "2026-05-10T00:00:00Z",
      },
      {
        id: "agent-skills/skills/deploy-guard",
        name: "Deploy Guard",
        description: "Check production readiness before release.",
        sourceProjectId: "agent-skills",
        relativePath: "agent-skills/skills/deploy-guard",
        absolutePath: `${root}/agent-skills/skills/deploy-guard`,
        defaultLinkName: "agent-skills-deploy-guard",
        hasAssets: true,
        hasScripts: true,
        hasReferences: false,
        installedAgents: [],
        lastModified: "2026-05-10T01:00:00Z",
      },
    ],
    agentProfiles: profiles,
  };

  const tasks: TaskRecord[] = [
    makeTask("scan_workspace", "Initial workspace scan completed.", "scan complete\n"),
  ];

  let userConfig = {
    schemaVersion: 1,
    recentWorkspaces: [root, "/tmp/previous-workspace"],
    agentProfiles: workspace.agentProfiles.map((state) => state.profile),
    uiPreferences: {
      theme: "system",
      projectSort: "name",
      showHiddenProjects: false,
    },
    windowSize: {
      width: 1280,
      height: 800,
    },
    automaticChecks: {
      enabled: false,
      intervalMinutes: 60,
      pullAfterCheck: false,
    },
  };

  function profileState(profile: AgentProfile, exists = true): AgentProfileState {
    return {
      profile,
      skillsDir: profile.skillsDir,
      exists,
      writable: exists,
      symlinkCount: 0,
      workspaceLinkCount: 0,
      entries: [],
    };
  }

  function makeTask(
    kind: string,
    summary: string,
    stdout = "",
    projectOutcomes: Array<unknown> = [],
  ): TaskRecord {
    taskId += 1;
    return {
      id: `task-${taskId}`,
      workspaceRoot: workspace?.root ?? root,
      kind,
      status: "succeeded",
      summary,
      stdout,
      stderr: "",
      projectOutcomes,
    };
  }

  function pushTask(
    kind: string,
    summary: string,
    stdout = "",
    projectOutcomes: Array<unknown> = [],
  ) {
    const task = makeTask(kind, summary, stdout, projectOutcomes);
    tasks.unshift(task);
    return task;
  }

  function syncAgentCounts() {
    for (const state of workspace.agentProfiles) {
      const installs = workspace.skills.flatMap((skill) =>
        skill.installedAgents.filter((install) => install.agentProfileId === state.profile.id),
      );
      state.workspaceLinkCount = installs.length;
      state.symlinkCount = installs.length;
      state.entries = installs.map((install) => ({
        name: install.linkName,
        path: install.targetPath,
        targetPath: install.sourcePath,
        sourcePath: install.sourcePath,
        kind: "symlink",
        status: install.status,
        removable: true,
      }));
    }
  }

  function findSkill(skillId: string) {
    const skill = workspace.skills.find((item) => item.id === skillId);
    if (!skill) {
      throw new Error(`Unknown skill ${skillId}`);
    }
    return skill;
  }

  function findProfile(profileId: string) {
    const profile = workspace.agentProfiles.find((state) => state.profile.id === profileId);
    if (!profile) {
      throw new Error(`Unknown profile ${profileId}`);
    }
    return profile;
  }

  function linkPreview(request: { skillId: string; agentProfileId: string; linkName?: string }) {
    const skill = findSkill(request.skillId);
    const profile = findProfile(request.agentProfileId);
    const linkName = request.linkName || skill.defaultLinkName;
    const targetPath = `${profile.skillsDir}/${linkName}`;
    const installed = skill.installedAgents.some(
      (install) =>
        install.agentProfileId === request.agentProfileId && install.linkName === linkName,
    );
    return {
      skillId: skill.id,
      agentProfileId: profile.profile.id,
      linkName,
      sourcePath: skill.absolutePath,
      targetPath,
      status: installed
        ? "already_installed"
        : profile.exists && profile.writable
          ? "will_link"
          : "agent_path_missing",
    };
  }

  function executeLink(preview: {
    skillId: string;
    agentProfileId: string;
    linkName: string;
    sourcePath: string;
    targetPath: string;
    status: string;
  }) {
    if (preview.status !== "will_link") {
      return;
    }
    const skill = findSkill(preview.skillId);
    if (
      !skill.installedAgents.some(
        (install) =>
          install.agentProfileId === preview.agentProfileId &&
          install.linkName === preview.linkName,
      )
    ) {
      skill.installedAgents.push({
        agentProfileId: preview.agentProfileId,
        linkName: preview.linkName,
        sourcePath: preview.sourcePath,
        targetPath: preview.targetPath,
        status: "valid",
      });
    }
    syncAgentCounts();
  }

  function unlinkPreview(request: { agentProfileId: string; linkName: string }) {
    for (const skill of workspace.skills) {
      const install = skill.installedAgents.find(
        (item) =>
          item.agentProfileId === request.agentProfileId && item.linkName === request.linkName,
      );
      if (install) {
        return {
          agentProfileId: request.agentProfileId,
          linkName: request.linkName,
          targetPath: install.targetPath,
          sourcePath: install.sourcePath,
          status: "will_unlink",
        };
      }
    }
    return {
      agentProfileId: request.agentProfileId,
      linkName: request.linkName,
      targetPath: `${findProfile(request.agentProfileId).skillsDir}/${request.linkName}`,
      status: "not_found",
    };
  }

  syncAgentCounts();

  window.__e2eCalls = calls;
  window.__TAURI_INTERNALS__ = {
    invoke: async (command: string, args: Record<string, any> = {}) => {
      calls.push({ command, args: clone(args) });
      switch (command) {
        case "restore_recent_workspace_command":
          return null;
        case "select_workspace_command":
          workspace.root = args.workspaceRoot;
          userConfig = {
            ...userConfig,
            recentWorkspaces: [
              args.workspaceRoot,
              ...userConfig.recentWorkspaces.filter((path) => path !== args.workspaceRoot),
            ],
          };
          return clone(workspace);
        case "scan_workspace_command":
          return clone(workspace);
        case "recent_task_records_command":
          return clone(tasks.slice(0, args.limit ?? 80));
        case "read_skill_markdown_preview_command":
          return {
            skillId: args.skillId,
            markdown:
              args.skillId === "agent-skills/skills/tdd"
                ? [
                    "# TDD Skill",
                    "Use red-green-refactor for production changes.",
                    ...Array.from(
                      { length: 16 },
                      (_, index) =>
                        `## Practice ${index + 1}\n\nWrite the failing test, make the smallest implementation change, then clean up the shape of the code before moving to the next behavior.`,
                    ),
                  ].join("\n\n")
                : "# Deploy Guard\n\nVerify release readiness before deployment.",
            truncated: false,
          };
        case "preview_link_skill_command":
          return clone(linkPreview(args.request));
        case "link_skill_command": {
          executeLink(args.request.preview);
          const skill = findSkill(args.request.preview.skillId);
          const profile = findProfile(args.request.preview.agentProfileId);
          const task = pushTask(
            "link_skill",
            `Linked ${skill.name} into ${profile.profile.name}.`,
            "linked\n",
          );
          return { task: clone(task), workspace: clone(workspace) };
        }
        case "preview_link_skills_batch_command":
          return {
            previews: clone(
              args.request.items.map(
                (request: { skillId: string; agentProfileId: string; linkName?: string }) =>
                  linkPreview(request),
              ),
            ),
          };
        case "link_skills_batch_command": {
          let linked = 0;
          let alreadyInstalled = 0;
          let skipped = 0;
          for (const preview of args.request.previews) {
            if (preview.status === "will_link") {
              executeLink(preview);
              linked += 1;
            } else if (preview.status === "already_installed") {
              alreadyInstalled += 1;
            } else {
              skipped += 1;
            }
          }
          const summary = { linked, alreadyInstalled, skipped, failed: 0 };
          const task = pushTask("link_skills_batch", "Batch link completed.", "batch linked\n");
          return {
            task: clone(task),
            workspace: clone(workspace),
            summary,
            previews: clone(args.request.previews),
          };
        }
        case "preview_unlink_skill_command":
          return clone(unlinkPreview(args.request));
        case "unlink_skill_command": {
          const preview = args.request.preview;
          let skillName = preview.linkName;
          for (const skill of workspace.skills) {
            const before = skill.installedAgents.length;
            skill.installedAgents = skill.installedAgents.filter(
              (install) =>
                !(
                  install.agentProfileId === preview.agentProfileId &&
                  install.linkName === preview.linkName
                ),
            );
            if (skill.installedAgents.length !== before) {
              skillName = skill.name;
            }
          }
          syncAgentCounts();
          const profile = findProfile(preview.agentProfileId);
          const task = pushTask(
            "unlink_skill",
            `Unlinked ${preview.linkName} from ${profile.profile.name}.`,
            "unlinked\n",
          );
          return { task: clone(task), workspace: clone(workspace), skillName };
        }
        case "load_workspace_config_command":
          return {
            schemaVersion: 1,
            projects: workspace.projects.map((project) => ({
              projectId: project.id,
              category: project.category,
              favorite: project.favorite,
              hidden: project.hidden,
              tags: project.tags,
              notes: project.notes,
            })),
          };
        case "save_workspace_config_command":
          for (const metadata of args.config.projects) {
            const project = workspace.projects.find((item) => item.id === metadata.projectId);
            if (project) {
              project.hidden = metadata.hidden;
              project.favorite = metadata.favorite;
              project.tags = metadata.tags;
              project.notes = metadata.notes;
              project.category = metadata.category || project.category;
            }
          }
          return clone(args.config);
        case "import_project_command": {
          const request = args.request;
          const directoryName =
            request.directoryName ||
            String(request.source)
              .split("/")
              .pop()
              ?.replace(/\.git$/, "") ||
            "imported";
          workspace.projects.push({
            id: directoryName,
            name: directoryName,
            path: `${workspace.root}/${directoryName}`,
            remoteUrl: request.source.includes("://")
              ? request.source
              : `https://github.com/${request.source}.git`,
            provider:
              request.source.includes("github.com") || /^[^/]+\/[^/]+$/.test(request.source)
                ? "github"
                : "unknown",
            branch: "main",
            upstream: "origin/main",
            gitStatus: "up_to_date",
            aheadCount: 0,
            behindCount: 0,
            pullAllEligible: true,
            category: "uncategorized",
            readmeSummary: "Imported project.",
            skillCount: 0,
            hidden: false,
            favorite: false,
            tags: [],
          });
          const task = pushTask(
            "import_project",
            `Imported ${request.source} into ${directoryName}.`,
            "git clone\n",
          );
          return { task: clone(task), workspace: clone(workspace) };
        }
        case "check_all_project_updates_command": {
          const task = pushTask(
            "fetch_project",
            `Checked ${workspace.projects.length} projects.`,
            "git fetch --prune\n",
          );
          return { task: clone(task), workspace: clone(workspace) };
        }
        case "pull_all_projects_command": {
          const count =
            args.request.safeProjectIds?.length ??
            workspace.projects.filter((project) => project.pullAllEligible).length;
          const task = pushTask(
            "sync_all_projects",
            `Pulled ${count} eligible projects.`,
            "git pull --ff-only --prune\n",
          );
          return { task: clone(task), workspace: clone(workspace) };
        }
        case "check_project_updates_command": {
          const task = pushTask(
            "fetch_project",
            `Checked ${args.projectId}.`,
            "git fetch --prune\n",
          );
          return { task: clone(task), workspace: clone(workspace) };
        }
        case "pull_project_command": {
          const task = pushTask(
            "pull_project",
            `Pulled ${args.request.projectId}.`,
            "git pull --ff-only --prune\n",
          );
          return { task: clone(task), workspace: clone(workspace) };
        }
        case "create_agent_profile_dir_command": {
          const requestedProfile = args.profile as AgentProfile;
          const state =
            workspace.agentProfiles.find(
              (profile) =>
                profile.profile.id === requestedProfile.id ||
                profile.skillsDir === args.resolvedSkillsDir ||
                profile.profile.skillsDir === requestedProfile.skillsDir,
            ) ?? findProfile(requestedProfile.id);
          state.exists = true;
          state.writable = true;
          return clone(workspace);
        }
        case "load_user_config_command":
          return clone(userConfig);
        case "save_agent_profiles_command":
          workspace.agentProfiles = args.profiles.map((profile: AgentProfile) => {
            const existing = workspace.agentProfiles.find(
              (state) => state.profile.id === profile.id,
            );
            return existing
              ? { ...existing, profile, skillsDir: profile.skillsDir }
              : profileState(profile, false);
          });
          userConfig = {
            ...userConfig,
            agentProfiles: workspace.agentProfiles.map((state) => state.profile),
          };
          syncAgentCounts();
          return clone(userConfig);
        case "patch_user_preferences_command":
          userConfig = {
            ...userConfig,
            recentWorkspaces: args.patch.recentWorkspaces,
            uiPreferences: args.patch.uiPreferences,
            automaticChecks: args.patch.automaticChecks,
          };
          return clone(userConfig);
        case "get_task_status_command":
        case "get_task_logs_command":
          return clone(tasks.find((task) => task.id === args.taskId) ?? null);
        case "cancel_task_command":
          return clone(tasks.find((task) => task.id === args.taskId) ?? null);
        case "open_workspace_path_command":
          return undefined;
        default:
          throw new Error(`Unhandled mock command: ${command}`);
      }
    },
  };
}

declare global {
  interface Window {
    __TAURI_INTERNALS__: {
      invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
    };
    __e2eCalls: Array<{ command: string; args: any }>;
  }
}
