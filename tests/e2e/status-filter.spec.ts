import { expect, test } from "@playwright/test";

// Minimal mock: two projects with distinct git statuses, so the Status filter has real effect.
async function openMockWithTwoStatuses(page: import("@playwright/test").Page) {
  await page.addInitScript(() => {
    const root = "/tmp/status-filter-workspace";
    const workspace = {
      root,
      projects: [
        {
          id: "proj-behind",
          name: "Behind Project",
          path: `${root}/proj-behind`,
          remoteUrl: "https://github.com/example/behind.git",
          provider: "github",
          branch: "main",
          upstream: "origin/main",
          gitStatus: "behind",
          aheadCount: 0,
          behindCount: 3,
          pullAllEligible: true,
          category: "skills",
          skillCount: 1,
          hidden: false,
          favorite: false,
          tags: [],
        },
        {
          id: "proj-uptodate",
          name: "Up To Date Project",
          path: `${root}/proj-uptodate`,
          remoteUrl: "https://github.com/example/uptodate.git",
          provider: "github",
          branch: "main",
          upstream: "origin/main",
          gitStatus: "up_to_date",
          aheadCount: 0,
          behindCount: 0,
          pullAllEligible: true,
          category: "skills",
          skillCount: 1,
          hidden: false,
          favorite: false,
          tags: [],
        },
      ],
      skills: [],
      agentProfiles: [],
    };
    const userConfig = {
      schemaVersion: 1,
      recentWorkspaces: [root],
      agentProfiles: [],
      uiPreferences: { theme: "system", projectSort: "name", showHiddenProjects: false },
      windowSize: { width: 1280, height: 800 },
      automaticChecks: { enabled: false, intervalMinutes: 60, pullAfterCheck: false },
    };
    (window as any).__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: any = {}) => {
        switch (command) {
          case "restore_recent_workspace_command":
            return null;
          case "select_workspace_command":
            workspace.root = args.workspaceRoot;
            return JSON.parse(JSON.stringify(workspace));
          case "scan_workspace_command":
            return JSON.parse(JSON.stringify(workspace));
          case "recent_task_records_command":
            return [];
          case "load_user_config_command":
            return JSON.parse(JSON.stringify(userConfig));
          case "load_workspace_config_command":
            return { schemaVersion: 1, projects: [] };
          default:
            return null;
        }
      },
    };
  });
  await page.goto("/");
  await page.getByLabel("Workspace path").fill("/tmp/status-filter-workspace");
  await page.getByRole("button", { name: "Open workspace" }).click();
  await page.getByRole("button", { name: "Projects" }).click();
  await expect(page.getByRole("heading", { name: "Projects" }).first()).toBeVisible();
}

test("Status filter updates the project list immediately", async ({ page }) => {
  await openMockWithTwoStatuses(page);

  // Both projects visible with default "All statuses"
  await expect(page.getByRole("heading", { name: "Behind Project" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Up To Date Project" })).toBeVisible();

  // Select "behind" status
  await page.getByLabel("Status").selectOption("behind");
  await expect(page.getByRole("heading", { name: "Behind Project" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Up To Date Project" })).toBeHidden();

  // Select "up_to_date" status
  await page.getByLabel("Status").selectOption("up_to_date");
  await expect(page.getByRole("heading", { name: "Up To Date Project" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Behind Project" })).toBeHidden();

  // Back to all
  await page.getByLabel("Status").selectOption("all");
  await expect(page.getByRole("heading", { name: "Behind Project" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Up To Date Project" })).toBeVisible();
});

test("Status dropdown keeps selection visible after change", async ({ page }) => {
  await openMockWithTwoStatuses(page);
  // Selected value should match the option text ("up to date" shown)
  await page.getByLabel("Status").selectOption("up_to_date");
  await expect(page.getByLabel("Status")).toHaveValue("up_to_date");
});

test("Status dropdown always lists every git status with per-status counts", async ({ page }) => {
  await openMockWithTwoStatuses(page);

  const statusSelect = page.getByLabel("Status");
  const trimmed = await statusSelect.evaluate((sel) =>
    Array.from((sel as HTMLSelectElement).options).map((opt) => (opt.textContent ?? "").trim()),
  );

  expect(trimmed).toEqual(
    expect.arrayContaining([
      "All statuses (2)",
      "up to date (1)",
      "behind (1)",
      "ahead (0)",
      "diverged (0)",
      "dirty (0)",
      "no upstream (0)",
      "detached (0)",
      "fetch failed (0)",
      "unknown (0)",
    ]),
  );

  // Selecting a zero-count status must empty the list — visible feedback.
  await statusSelect.selectOption("dirty");
  await expect(page.getByRole("heading", { name: "No matching projects" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Behind Project" })).toBeHidden();
  await expect(page.getByRole("heading", { name: "Up To Date Project" })).toBeHidden();
});
