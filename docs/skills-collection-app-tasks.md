# Skills Collection App Task Breakdown

## Overview

These tasks implement the MVP from [docs/skills-collection-app.md](skills-collection-app.md) through the plan in [docs/skills-collection-app-implementation-plan.md](skills-collection-app-implementation-plan.md). Tasks are sized as 1-2 day slices where possible and ordered by dependency.

## Task Index

| ID | Task | Phase | Depends On |
| --- | --- | --- | --- |
| 01 | Scaffold Tauri React app | Foundation | None |
| 02 | Define shared domain models | Foundation | 01 |
| 03 | Implement config storage | Foundation | 02 |
| 04 | Implement workspace selection flow | Foundation | 03 |
| 05 | Scan top-level Git projects | Workspace scanning | 02, 04 |
| 06 | Scan and parse skills | Workspace scanning | 02, 04 |
| 07 | Detect installed agent skills | Workspace scanning | 03, 06 |
| 08 | Build app navigation and core views | Workspace scanning | 02, 04 |
| 09 | Implement repository import | Git and import | 05, 12 |
| 10 | Implement update checks | Git and import | 05, 12 |
| 11 | Implement safe project pull | Git and import | 05, 10, 12 |
| 12 | Implement serial task queue and logs | Git and import | 02 |
| 13 | Manage agent profiles | Agent linking | 03, 07 |
| 14 | Implement link preview and single install | Agent linking | 07, 13 |
| 15 | Implement batch link workflow | Agent linking | 14 |
| 16 | Implement unlink preview and uninstall | Agent linking | 07, 13 |
| 17 | Complete Projects view | UX completion | 05, 09, 10, 11 |
| 18 | Complete Skills view | UX completion | 06, 14, 15, 16 |
| 19 | Complete Agents view | UX completion | 13, 14, 16 |
| 20 | Complete Tasks Logs and Settings views | UX completion | 03, 12, 13 |
| 21 | Add integration fixtures and tests | UX completion | 05-16 |

## Task 01: Scaffold Tauri React App

### Context

Part of the implementation for [Skills Collection App](skills-collection-app.md). This creates the `app/` project described in the spec.

### Description

Create a Tauri desktop app scaffold with React, TypeScript and Vite under `app/`. Keep the existing root scripts and upstream project directories unchanged.

### Acceptance Criteria

- [x] `app/package.json` defines dev/build commands for the frontend and Tauri app.
- [x] `app/src/` contains a minimal React app shell.
- [x] `app/src-tauri/` contains a compiling Rust/Tauri command entrypoint.
- [x] The app can be started in development mode on a configured machine.
- [x] Generated files are limited to `app/` and do not alter upstream project directories.

Verification note: `npm run build`, `cargo test --manifest-path app/src-tauri/Cargo.toml`, `cargo check --manifest-path app/src-tauri/Cargo.toml --features desktop` and `npm run tauri:dev` pass after installing the required Linux Tauri development packages. Runtime emitted local EGL/Mesa warnings in this environment but the app process started.

### Technical Details

Use the Tauri scaffold that matches the selected local toolchain. Add only minimal placeholder UI and a single health-check command before domain work begins.

### Dependencies

- Blocked by: None
- Blocks: Tasks 02-04

## Task 02: Define Shared Domain Models

### Context

The spec defines Workspace, Project, Skill, AgentProfile, InstalledAgentSkill, GitStatus and ProjectCategory. These must be represented consistently in Rust and TypeScript.

### Description

Create Rust serde models and matching TypeScript types for the core command boundary.

### Acceptance Criteria

- [x] Rust model structs/enums exist for workspace, project, skill, agent profile, installed agent skill, Git status, project category, task and link preview.
- [x] TypeScript types mirror Rust JSON shapes.
- [x] Enum values match the spec exactly where specified.
- [x] The health-check command can return a typed placeholder workspace payload.

### Technical Details

Prefer stable snake_case JSON values for enum variants that already appear in the spec, such as `up_to_date`, `no_upstream` and `agent_path_missing`.

### Dependencies

- Blocked by: Task 01
- Blocks: Most later tasks

## Task 03: Implement Config Storage

### Context

The app uses workspace config at `<workspace>/.skills-collection/config.json` and user app config for recent workspace, profiles and UI preferences.

### Description

Implement Rust config read/write helpers and Tauri commands for loading and saving workspace config and user app config.

### Acceptance Criteria

- [x] Missing config files load as defaults.
- [x] Workspace config stores project metadata only, not authoritative filesystem or Git state.
- [x] User config stores recent workspaces, agent profiles, UI preferences, window size and automatic check settings.
- [x] Invalid config JSON returns a structured error with the file path.
- [x] Config writes are atomic enough to avoid partial file corruption in normal operation.

Verification note: `cargo test --manifest-path app/src-tauri/Cargo.toml`, `cargo check --manifest-path app/src-tauri/Cargo.toml --features desktop` and `npm run build` pass.

### Technical Details

Keep config schemas versioned from the start, even if version `1` is the only supported version.

### Dependencies

- Blocked by: Task 02
- Blocks: Tasks 04, 07, 13, 20

## Task 04: Implement Workspace Selection Flow

### Context

MVP manages a single current workspace and remembers the most recent workspace.

### Description

Add startup behavior and UI flow for selecting, validating and remembering a collection workspace.

### Acceptance Criteria

- [x] First launch prompts for a workspace directory.
- [x] Later launches restore the recent workspace if it still exists.
- [x] Empty directories can be initialized with `.skills-collection/config.json`.
- [x] Existing directories can be selected and scanned without requiring the root itself to be a Git repo.
- [x] Invalid paths produce a clear UI error.

Verification note: `cargo test --manifest-path app/src-tauri/Cargo.toml`, `cargo check --manifest-path app/src-tauri/Cargo.toml --features desktop` and `npm run build` pass. The first-launch UI currently accepts a directory path and validates it through Tauri commands.

### Technical Details

Use Tauri dialog APIs for selection and Rust validation for path existence, directory type and canonicalization.

### Dependencies

- Blocked by: Task 03
- Blocks: Tasks 05-08

## Task 05: Scan Top-Level Git Projects

### Context

The workspace contains top-level directories that may be upstream Git clones.

### Description

Implement read-only scanning for one-level-deep Git repositories under the selected workspace.

### Acceptance Criteria

- [x] Scanner discovers top-level child Git repositories.
- [x] Scanner ignores non-Git directories as projects unless metadata cleanup needs them later.
- [x] Project data includes name, path, remote URL, provider, branch, upstream, Git status, README, LICENSE, skill count, hidden/favorite/tags/notes.
- [x] Startup scan does not contact remotes.
- [x] Dirty and detached states are detected locally.

Verification note: covered by `app/src-tauri/tests/workspace_scanning.rs` using local `git init` fixtures; no fetch/check remote command is used during startup scan.

### Technical Details

Use `git -C <dir>` commands and exit codes. Do not parse human-oriented `git status` output when a porcelain or explicit command is available.

### Dependencies

- Blocked by: Tasks 02, 04
- Blocks: Tasks 09-11, 17

## Task 06: Scan And Parse Skills

### Context

MVP scans `SKILL.md` files inside the selected workspace and ignores `.git` and `node_modules`.

### Description

Implement skill discovery, frontmatter parsing and fallback metadata extraction.

### Acceptance Criteria

- [x] Scanner finds `SKILL.md` files under the workspace.
- [x] Scanner ignores `.git` and `node_modules`.
- [x] Skills outside the workspace are rejected.
- [x] Frontmatter `name` and `description` are preferred.
- [x] Missing frontmatter falls back to directory name and first heading or first paragraph.
- [x] Skill data includes source project, relative path, default link name, assets/scripts/references flags and last modified time.

Verification note: covered by `app/src-tauri/tests/workspace_scanning.rs` with frontmatter, fallback, ignored directory and resource flag fixtures.

### Technical Details

Default link name should follow `<repo-name>-<skill-name>`. Keep parsing conservative and resilient to malformed frontmatter.

### Dependencies

- Blocked by: Tasks 02, 04
- Blocks: Tasks 07, 18

## Task 07: Detect Installed Agent Skills

### Context

Installed state is derived by inspecting symlinks in agent profile skills directories.

### Description

Implement agent directory scanning that maps symlink targets back to workspace skills.

### Acceptance Criteria

- [x] Built-in Claude Code and Codex profile directories are inspected when configured or defaulted.
- [x] Symlink status maps to `valid`, `broken`, `external` or `conflict`.
- [x] Links pointing to current workspace skills populate `installedAgents` on the matching skill.
- [x] Ordinary files and directories are reported as non-removable entries, not treated as installed skills.
- [x] Missing and non-writable profile paths are represented in profile state.

Verification note: covered by `app/src-tauri/tests/workspace_scanning.rs` with valid, broken, external, conflict and ordinary-file agent directory entries.

### Technical Details

Resolve relative symlink targets from the link directory. Canonicalize existing targets before workspace containment checks.

### Dependencies

- Blocked by: Tasks 03, 06
- Blocks: Tasks 13-16, 19

## Task 08: Build App Navigation And Core Views

### Context

The UI information architecture has Skills, Projects, Agents, Tasks / Logs and Settings sections.

### Description

Create the primary app layout, navigation and initial empty/loading/error states for all MVP views.

### Acceptance Criteria

- [x] Navigation exposes Skills, Projects, Agents, Tasks / Logs and Settings.
- [x] Views can consume typed workspace state.
- [x] Loading, empty and error states are implemented without layout shifts.
- [x] Current workspace path is visible in the app chrome or settings surface.
- [x] Refresh action triggers a scan command and updates state.

Verification note: `npm run build`, `cargo test --manifest-path app/src-tauri/Cargo.toml` and `cargo check --manifest-path app/src-tauri/Cargo.toml --features desktop` pass. Core views now render typed workspace lists, empty states, task logs and workspace settings.

### Technical Details

Keep the UI utilitarian and dense enough for repeated management workflows. Avoid marketing-style landing pages.

### Dependencies

- Blocked by: Tasks 02, 04
- Blocks: Tasks 17-20

## Task 09: Implement Repository Import

### Context

MVP imports GitHub shorthand and arbitrary Git URLs into the current workspace.

### Description

Implement import validation, default directory name inference, optional custom directory name and `git clone` execution.

### Acceptance Criteria

- [x] `owner/repo` converts to `https://github.com/owner/repo.git`.
- [x] Full HTTPS, SSH, `git@...`, `ssh://...` and arbitrary Git URLs pass through unchanged.
- [x] Directory name is a safe single path segment and rejects empty, `.`, `..` and names containing separators.
- [x] Existing Git directory is adopted and scanned without cloning.
- [x] Existing non-Git directory returns a blocking error.
- [x] Shallow clone option adds `--depth 1`.
- [x] Import completion triggers local scan of the new project.

Verification note: covered by `app/src-tauri/tests/git_operations.rs` using local Git fixtures for planning, clone, adopt and blocking cases.

### Technical Details

Run clone as a task so stdout/stderr are captured in logs.

### Dependencies

- Blocked by: Tasks 05, 12
- Blocks: Task 17

## Task 10: Implement Update Checks

### Context

Checking updates explicitly runs `git fetch --prune` and then computes ahead/behind.

### Description

Implement single-project and all-project update checks through task queue jobs.

### Acceptance Criteria

- [x] Startup scan does not fetch.
- [x] User-triggered check runs `git fetch --prune`.
- [x] Ahead/behind calculation supports `up_to_date`, `behind`, `ahead`, `diverged`, `no_upstream`, `detached`, `fetch_failed` and `unknown`.
- [x] All-project check records per-project success, skip and failure results.
- [x] Results refresh project state and are visible in Projects view and logs.

Verification note: covered by `app/src-tauri/tests/git_operations.rs` with local bare remotes for fetch, behind state and all-project ok/skip/failure summaries.

### Technical Details

For projects without upstream, skip fetch-dependent ahead/behind calculation and report `no_upstream`.

### Dependencies

- Blocked by: Tasks 05, 12
- Blocks: Tasks 11, 17

## Task 11: Implement Safe Project Pull

### Context

The app updates projects with `git pull --ff-only --prune`, skipping dirty working trees by default.

### Description

Implement single-project and all-project pull operations with optional advanced autostash behavior.

### Acceptance Criteria

- [x] Dirty projects are skipped by default.
- [x] Pull uses `git pull --ff-only --prune`.
- [x] Optional autostash adds `--autostash` only after explicit user selection.
- [x] Pull does not run for detached or no-upstream projects.
- [x] All-project pull continues after skipped projects and summarizes results.
- [x] Pull completion refreshes affected project and skill state.

Verification note: covered by `app/src-tauri/tests/git_operations.rs` with dirty, fast-forward pull and all-project continuation fixtures.

### Technical Details

Mirror the existing safety behavior of `scripts/sync-projects.sh`.

### Dependencies

- Blocked by: Tasks 05, 10, 12
- Blocks: Task 17

## Task 12: Implement Serial Task Queue And Logs

### Context

MVP uses a global serial queue for Git, filesystem and agent directory tasks.

### Description

Create a Rust task queue with task status, cancellation semantics and log capture.

### Acceptance Criteria

- [x] Queue supports `queued`, `running`, `succeeded`, `skipped`, `failed` and `cancelled`.
- [x] Supported task types include all MVP task kinds from the spec.
- [x] Tasks run serially.
- [x] Queued tasks can be cancelled immediately.
- [x] Running batch task cancellation stops after the current substep.
- [x] Running single Git command is not force killed in MVP.
- [x] stdout and stderr are captured and retrievable.

Verification note: covered by `app/src-tauri/tests/task_queue.rs`; Git/import operations run through task records and expose status/log commands.

### Technical Details

Expose `get_task_status`, `cancel_task` and `get_task_logs` commands early so Git/import/link work can depend on them.

### Dependencies

- Blocked by: Task 02
- Blocks: Tasks 09-11, 14-16, 20

## Task 13: Manage Agent Profiles

### Context

MVP includes built-in Claude Code and Codex profiles and supports custom profiles.

### Description

Implement profile listing, saving, validation and create-directory command.

### Acceptance Criteria

- [x] Built-in profiles default to `~/.claude/skills` and `~/.codex/skills`.
- [x] Custom profiles store id, name, skills_dir, enabled and link_mode.
- [x] Profile state includes exists, missing, writable, not writable, symlink count and linked workspace skills.
- [x] Missing profile directories can be created only after explicit UI confirmation.
- [x] Disabled profiles are excluded from default install target selection.

Verification note: covered by `app/src-tauri/tests/agent_profiles.rs` and agent profile scan coverage in `app/src-tauri/tests/workspace_scanning.rs`; `cargo test --manifest-path app/src-tauri/Cargo.toml` and `npm run build` pass.

### Technical Details

Link mode is `symlink` only for MVP, but keep the enum extensible.

### Dependencies

- Blocked by: Tasks 03, 07
- Blocks: Tasks 14-16, 19, 20

## Task 14: Implement Link Preview And Single Install

### Context

Single skill installation must detect conflicts before creating a symlink.

### Description

Implement preview and execution for linking one workspace skill into one agent profile.

### Acceptance Criteria

- [x] Preview statuses include `will_link`, `already_installed`, `name_conflict`, `blocked_by_real_file`, `blocked_by_real_directory`, `missing_source`, `agent_path_missing` and `agent_path_not_writable`.
- [x] Target missing creates a symlink on execution.
- [x] Existing symlink to the same skill is no-op.
- [x] Ordinary file or directory blocks execution.
- [x] Symlink to another path blocks execution unless the user chooses an allowed replacement path.
- [x] Source path must be inside current workspace.

Verification note: covered by `app/src-tauri/tests/link_operations.rs`; conflicting symlinks remain untouched and users can choose a different safe link name/path. `cargo test --manifest-path app/src-tauri/Cargo.toml` and `npm run build` pass.

### Technical Details

The execution command should accept the preview decision or a preview id so the UI cannot accidentally mutate a different target than the user reviewed.

### Dependencies

- Blocked by: Tasks 07, 12, 13
- Blocks: Tasks 15, 18, 19

## Task 15: Implement Batch Link Workflow

### Context

Batch install selects multiple skills and one or more target agents, then executes only safe items after preview.

### Description

Implement batch preview, confirmation and execution for symlink installs.

### Acceptance Criteria

- [x] Batch preview returns one result per skill/profile pair.
- [x] Conflict items remain unmodified after execution.
- [x] Safe items are executed serially through the task queue.
- [x] Batch result summarizes linked, already installed, skipped and failed counts.
- [x] UI supports selecting multiple skills and multiple target profiles.

Verification note: covered by `app/src-tauri/tests/link_operations.rs`; Skills view now supports multi-skill and multi-profile preview plus safe batch execution. `cargo test --manifest-path app/src-tauri/Cargo.toml` and `npm run build` pass.

### Technical Details

Reuse the single-link preview engine so conflict logic stays identical.

### Dependencies

- Blocked by: Task 14
- Blocks: Task 18

## Task 16: Implement Unlink Preview And Uninstall

### Context

Uninstall only removes symlinks in agent directories that point to current workspace skills.

### Description

Implement single and batch unlink preview/execution.

### Acceptance Criteria

- [x] Symlink pointing to a current workspace skill can be removed.
- [x] Ordinary files are blocked.
- [x] Ordinary directories are blocked.
- [x] Symlinks pointing outside the workspace are blocked.
- [x] Broken symlinks are blocked unless they can be proven to represent a current workspace link.
- [x] Batch uninstall requires preview and leaves blocked items untouched.

Verification note: covered by `app/src-tauri/tests/link_operations.rs`; unlink execution revalidates previews before removing symlinks and batch unlink leaves blocked items untouched. `cargo test --manifest-path app/src-tauri/Cargo.toml` and `npm run build` pass.

### Technical Details

Prefer `unlink` semantics for symlinks. Never recursively remove directories.

### Dependencies

- Blocked by: Tasks 07, 12, 13
- Blocks: Tasks 18, 19

## Task 17: Complete Projects View

### Context

Projects view manages import, Git status and update operations.

### Description

Build the complete Projects UI with filtering, project status display, import dialog and Git actions.

### Acceptance Criteria

- [ ] Project list shows remote URL, branch, upstream, dirty state, ahead/behind, README summary and LICENSE state.
- [ ] User can import a repository with advanced options for directory name and shallow clone.
- [ ] User can check updates for one project or all projects.
- [ ] User can pull one project or all safe projects.
- [ ] Hidden projects can be hidden from default view and redisplayed through filters.
- [ ] Per-project errors link to task logs.

### Technical Details

Do not expose branch switching in MVP.

### Dependencies

- Blocked by: Tasks 05, 09, 10, 11
- Blocks: None

## Task 18: Complete Skills View

### Context

Skills view is the primary MVP surface.

### Description

Build search, filters, detail preview and single/batch install/uninstall workflows for skills.

### Acceptance Criteria

- [ ] Search works across name, description and path.
- [ ] Filters cover project, category, tags and agent installation status.
- [ ] Detail panel shows metadata, `SKILL.md` preview, source project, relative path and installed agents.
- [ ] Single install and uninstall actions show previews before mutation.
- [ ] Batch selection supports install preview and execution.
- [ ] Open README, open project directory and copy path actions are available where applicable.

### Technical Details

Keep detail preview read-only. MVP does not include a skill editor.

### Dependencies

- Blocked by: Tasks 06, 14, 15, 16
- Blocks: None

## Task 19: Complete Agents View

### Context

Agents view manages profiles and linked skills.

### Description

Build profile status cards/table, custom profile editing, create-dir action and linked-skill management.

### Acceptance Criteria

- [ ] Built-in Claude Code and Codex profiles are visible.
- [ ] Custom profiles can be added, edited, disabled and re-enabled.
- [ ] Skills dir exists/writable state is visible.
- [ ] Missing directory creation requires confirmation.
- [ ] Linked skills from current workspace are listed per profile.
- [ ] Uninstall action uses unlink preview.

### Technical Details

Profile editing should validate duplicate ids and invalid paths before save.

### Dependencies

- Blocked by: Tasks 13, 14, 16
- Blocks: None

## Task 20: Complete Tasks Logs And Settings Views

### Context

Tasks / Logs and Settings are required MVP views.

### Description

Implement task status browsing, raw log display/copy, workspace settings, recent workspaces, agent profile settings, UI preferences and automatic check settings.

### Acceptance Criteria

- [ ] Tasks / Logs shows recent tasks, current task status and summary errors.
- [ ] Raw stdout/stderr can be expanded and copied.
- [ ] Queued tasks can be cancelled.
- [ ] Settings can change current workspace and recent workspace list.
- [ ] Settings can edit automatic check frequency.
- [ ] Settings can edit UI preferences and agent profiles.

### Technical Details

Keep logs bounded or paginated so long Git output does not freeze the UI.

### Dependencies

- Blocked by: Tasks 03, 12, 13
- Blocks: None

## Task 21: Add Integration Fixtures And Tests

### Context

The riskier parts are path safety, Git status mapping, skill parsing and link/unlink previews.

### Description

Add test fixtures and automated coverage for backend logic and core UI workflows.

### Acceptance Criteria

- [ ] Rust tests cover safe directory name validation.
- [ ] Rust tests cover workspace containment and symlink resolution.
- [ ] Rust tests cover skill parser frontmatter and fallback behavior.
- [ ] Rust tests cover link and unlink preview statuses.
- [ ] Git fixture tests cover dirty, no-upstream, detached and ahead/behind state where practical.
- [ ] UI smoke tests cover workspace selection, scan, link preview and logs.

### Technical Details

Use temporary directories for filesystem tests. Avoid network-dependent tests in the default suite.

### Dependencies

- Blocked by: Tasks 05-16
- Blocks: Release readiness
