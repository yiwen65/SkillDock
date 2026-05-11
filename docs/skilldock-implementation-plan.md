# SkillDock 实施计划

## Overview

本计划把 [SkillDock 设计文档](skilldock.md) 拆成可执行的 Tauri MVP 实施路径。MVP 目标是交付一个本地优先的 Linux/macOS 桌面应用，用图形界面完成 workspace 扫描、项目导入、Git 更新检查、skill 预览、agent profile 管理、软链接安装和卸载。

## Linked Specification

- Spec: [docs/skilldock.md](skilldock.md)
- Task breakdown: [docs/skilldock-tasks.md](skilldock-tasks.md)

## Clarifications And Assumptions

- **Tauri 版本**: 具体版本在 scaffold 时按项目工具链锁定。本计划不依赖特定 minor 版本。
- **发布打包**: MVP 先保证开发运行和本地构建，不把签名、公证、自动更新和安装包分发列为首批验收。
- **Git 执行**: 后端调用系统 `git` 可执行文件。缺失 `git` 时返回可解释错误，不内置 Git。
- **Markdown 预览**: MVP 只展示 README / `SKILL.md` 摘要和前段文本，不实现完整 Markdown renderer。
- **自动检查**: MVP 支持应用运行期间的定时检查配置，但不做后台守护或开机自启。
- **根目录 Git**: collection workspace 本身可以不是 Git 仓库；扫描对象是顶层子 Git 仓库。
- **Notion workflow adaptation**: 原始 spec 是本地 Markdown 文件，因此计划和任务也落在 `docs/` 下，而不是创建 Notion 页面或数据库任务。

## Requirements Summary

### Functional Requirements

- 选择并记住单个 collection workspace。
- 扫描顶层 Git 项目，读取 remote、branch、upstream、dirty、ahead/behind、README 和 LICENSE 状态。
- 扫描 workspace 内 `SKILL.md`，解析 frontmatter、来源项目、路径、资产/脚本/reference 标记和安装状态。
- 导入 GitHub `owner/repo`、HTTPS、SSH、`git@...`、`ssh://...` 和任意 Git URL。
- 显式检查更新并执行安全 pull；默认跳过 dirty 项目，使用 `git pull --ff-only --prune`。
- 管理 Claude Code、Codex 和自定义 agent profiles。
- 预览并执行单个/批量 symlink 安装，处理名称冲突和目标路径问题。
- 预览并执行单个/批量卸载，只删除指向当前 workspace 内 skill 的 symlink。
- 提供 Skills、Projects、Agents、Tasks / Logs、Settings 视图。
- 维护串行任务队列，记录 task status、摘要错误、stdout 和 stderr。
- 读写 workspace config 和 user app config。

### Non-Functional Requirements

- **Safety**: 不覆盖普通文件或目录；不删除项目目录；不删除 workspace 外部 symlink；批量 mutating 操作必须先预览。
- **Local-first**: 文件系统和 Git 状态是 source of truth；不自动联网，除非用户触发检查或开启定时检查。
- **Portability**: 支持 Linux 和 macOS；路径处理必须规范化并防止 workspace escape。
- **Maintainability**: Rust command 返回 serde JSON，前端维护对应 TypeScript types；CLI fallback 继续保留。
- **Observability**: 所有 Git 和文件系统任务都要有可查看、可复制的日志。

### Acceptance Criteria

- [ ] `app/` 下存在 Tauri + React + TypeScript + Vite scaffold，并能本地运行。
- [x] 首次启动可选择 workspace，后续启动能恢复最近 workspace。
- [x] 已有 collection workspace 可被扫描，项目和 skills 数据正确显示。
- [x] 导入仓库能处理正常 clone、已有 Git 目录、已有非 Git 目录和 shallow clone。
- [x] 检查更新只在用户触发或配置的运行期定时检查时联网。
- [x] Pull 操作默认跳过 dirty 项目，且不产生 merge commit。
- [x] Claude Code 和 Codex profiles 默认可用，并能新增自定义 profile。
- [x] 缺失 agent skills 目录可在确认后创建。
- [x] 单个和批量安装都先给出冲突预览，再执行安全项。
- [x] 卸载只会删除指向当前 workspace 内 skill 的 symlink。
- [x] Tasks / Logs 能展示任务状态、错误摘要和原始 stdout / stderr。
- [ ] Settings 能编辑 workspace、agent profiles、UI 偏好和自动检查配置。

## Technical Approach

### Architecture

- `app/src` 负责 React UI、路由、view state、command 调用和展示层类型。
- `app/src-tauri` 负责 workspace 扫描、Git 命令、symlink 操作、配置读写和串行任务队列。
- Rust domain 层以 workspace、project、skill、agent profile、link preview 和 task 为核心模型。
- 前端不直接访问文件系统；所有 mutating 操作经由 Tauri commands 并返回结构化结果。
- CLI 脚本保留为 fallback；后端行为应与现有脚本规则一致。

### Suggested Module Layout

```text
app/
  package.json
  src/
    App.tsx
    main.tsx
    components/
    views/
    lib/
      commands.ts
      types.ts
      workspace-state.ts
  src-tauri/
    Cargo.toml
    src/
      main.rs
      commands/
      config.rs
      git.rs
      model.rs
      scanner.rs
      skills.rs
      agents.rs
      links.rs
      tasks.rs
```

### Key Design Decisions

1. **Serial task queue in Rust**: Git and symlink operations touch shared resources, so serialization keeps logs and state transitions predictable.
2. **Preview-before-mutation contract**: Link/unlink batch operations produce an explicit preview result that the UI confirms before execution.
3. **Filesystem-derived truth**: Project, skill, Git, and installed-agent state are recomputed from disk rather than trusted from metadata.
4. **Metadata as user preference only**: `.skilldock/config.json` stores labels, notes, favorites, hidden state and overrides, not authoritative Git or install state.
5. **Structured command boundary**: Rust command responses are typed and mirrored in TypeScript to avoid ad hoc UI parsing of stdout.

## Implementation Phases

### Phase 1: App Foundation

**Goal**: Establish a runnable desktop app and the shared domain boundary.

**Tasks**:
- [ ] [Scaffold Tauri React app](skilldock-tasks.md#task-01-scaffold-tauri-react-app)
- [ ] [Define shared domain models](skilldock-tasks.md#task-02-define-shared-domain-models)
- [ ] [Implement config storage](skilldock-tasks.md#task-03-implement-config-storage)
- [ ] [Implement workspace selection flow](skilldock-tasks.md#task-04-implement-workspace-selection-flow)

**Deliverables**: Runnable app shell, recent workspace persistence, shared model definitions.

### Phase 2: Workspace Scanning

**Goal**: Populate the app from an existing collection workspace without mutating it.

**Tasks**:
- [ ] [Scan top-level Git projects](skilldock-tasks.md#task-05-scan-top-level-git-projects)
- [ ] [Scan and parse skills](skilldock-tasks.md#task-06-scan-and-parse-skills)
- [ ] [Detect installed agent skills](skilldock-tasks.md#task-07-detect-installed-agent-skills)
- [ ] [Build app navigation and core views](skilldock-tasks.md#task-08-build-app-navigation-and-core-views)

**Deliverables**: Projects, Skills and Agents views show accurate read-only workspace state.

### Phase 3: Git And Import Operations

**Goal**: Support safe repository import, update checks and fast-forward pulls.

**Tasks**:
- [ ] [Implement repository import](skilldock-tasks.md#task-09-implement-repository-import)
- [ ] [Implement update checks](skilldock-tasks.md#task-10-implement-update-checks)
- [ ] [Implement safe project pull](skilldock-tasks.md#task-11-implement-safe-project-pull)
- [ ] [Implement serial task queue and logs](skilldock-tasks.md#task-12-implement-serial-task-queue-and-logs)

**Deliverables**: Import and Git operations run through logged tasks and refresh affected workspace state.

### Phase 4: Agent Linking

**Goal**: Install and uninstall skills through safe symlink previews.

**Tasks**:
- [x] [Manage agent profiles](skilldock-tasks.md#task-13-manage-agent-profiles)
- [x] [Implement link preview and single install](skilldock-tasks.md#task-14-implement-link-preview-and-single-install)
- [x] [Implement batch link workflow](skilldock-tasks.md#task-15-implement-batch-link-workflow)
- [x] [Implement unlink preview and uninstall](skilldock-tasks.md#task-16-implement-unlink-preview-and-uninstall)

**Deliverables**: Users can install and uninstall skills for Claude Code, Codex and custom profiles without overwriting unsafe targets.

### Phase 5: UX Completion And Verification

**Goal**: Complete MVP screens, settings and regression coverage.

**Tasks**:
- [ ] [Complete Projects view](skilldock-tasks.md#task-17-complete-projects-view)
- [ ] [Complete Skills view](skilldock-tasks.md#task-18-complete-skills-view)
- [ ] [Complete Agents view](skilldock-tasks.md#task-19-complete-agents-view)
- [ ] [Complete Tasks Logs and Settings views](skilldock-tasks.md#task-20-complete-tasks-logs-and-settings-views)
- [ ] [Add integration fixtures and tests](skilldock-tasks.md#task-21-add-integration-fixtures-and-tests)

**Deliverables**: MVP is usable end to end against this collection workspace.

## Dependencies

- System `git` must be installed on the user's machine.
- Tauri development prerequisites must be installed for Linux/macOS build targets.
- Agent profile target directories must be accessible or creatable by the user.
- Existing CLI scripts define behavior that should be mirrored for link and sync safety.

## Risks And Mitigations

### Path Safety Bugs

- **Probability**: Medium
- **Impact**: High
- **Mitigation**: Canonicalize workspace, source skill and target symlink paths in Rust; reject source paths outside workspace; test symlink edge cases.

### Git Command Variability

- **Probability**: Medium
- **Impact**: Medium
- **Mitigation**: Treat stdout/stderr as diagnostic text only; derive state from explicit Git commands and exit codes; surface command failures in logs.

### UI Drift From CLI Behavior

- **Probability**: Medium
- **Impact**: Medium
- **Mitigation**: Convert existing script rules into shared test cases and command acceptance criteria before broad UI work.

### Long-Running Operations Blocking UI

- **Probability**: Medium
- **Impact**: Medium
- **Mitigation**: Run operations as queued tasks, expose task status polling and refresh views after completion.

### Agent Directory Permission Errors

- **Probability**: Medium
- **Impact**: Low
- **Mitigation**: Probe exists/writable status, preview `agent_path_missing` and `agent_path_not_writable`, and require explicit create-dir confirmation.

## Success Criteria

### Technical Success

- [ ] All MVP acceptance criteria pass on Linux and macOS development environments.
- [ ] Rust unit tests cover path safety, skill parsing, Git status mapping and link/unlink previews.
- [ ] UI smoke tests cover first-run workspace selection, scan, import dialog, link preview and logs.
- [ ] Dirty Git repositories are never pulled unless advanced autostash is explicitly selected.
- [ ] Batch install/uninstall never mutates conflict items.

### Product Success

- [ ] A user can manage the current `Skills-repo` collection without using terminal scripts.
- [ ] Errors from Git and filesystem operations are understandable from Tasks / Logs.
- [ ] Existing CLI scripts remain usable fallback paths.

## Progress Tracking

- Phase 1: Complete. Tasks 01, 02, 03 and 04 complete.
- Phase 2: Complete. Tasks 05, 06, 07 and 08 complete.
- Phase 3: Complete. Tasks 09, 10, 11 and 12 complete.
- Phase 4: Complete. Tasks 13, 14, 15 and 16 complete.
- Phase 5: Not started

**Overall progress**: 76% complete.

**Latest update**: 2026-05-10. Completed Tasks 13-16 with agent profile management, safe link previews, single and batch install, unlink previews, single and batch uninstall, TypeScript command wrappers, minimal batch install UI and regression tests.
