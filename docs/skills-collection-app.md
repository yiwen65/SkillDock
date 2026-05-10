# Skills Collection App 设计文档

## 背景

当前仓库通过在根目录执行 `git clone <git 仓库地址>` 的方式收集 GitHub 开源优质 agent skills、plugins、tools。每个顶层目录对应一个上游项目，根目录负责提供集合级 README、AGENTS 和管理脚本。

已有能力：

- `scripts/link-skill.sh`：把子仓库中的某个 `SKILL.md` 目录软链接到 Claude Code、Codex 或全部目标 agent。
- `scripts/list-skills.sh`：列出当前 workspace 内全部 skill 目录。
- `scripts/sync-projects.sh`：遍历顶层 Git 子仓库并执行安全同步，默认 `git pull --ff-only --prune`。

目标是把这些管理方式封装为一个支持 Linux 和 macOS 的带 UI 应用，让导入、同步、检查更新、skill 预览、agent 软链接安装和卸载都能在图形界面中完成。

## 产品定位

这是一个本地优先的 agent resources collection manager。

MVP 先服务个人本地工作流，但底层架构按未来可发布给其他用户使用的产品设计。

核心目标：

- 管理一个 collection workspace。
- 导入 GitHub 或任意 Git 仓库。
- 扫描项目内的 `SKILL.md`、README、LICENSE 和 Git 状态。
- 检查和更新子仓库。
- 把 workspace 内的 skills 灵活软链接到 Claude Code、Codex 和自定义 agent profiles。
- 支持批量安装、冲突预览、卸载软链接。
- 提供任务日志，便于排查 Git 和文件系统错误。

非目标：

- 不做完整 Git 客户端。
- 不做 skill 编辑器。
- 不做 GitHub 搜索和推荐市场。
- 不做后台守护进程或开机自启。
- 不删除本地项目目录。
- 不管理 workspace 外部的 skill 来源。

## 技术选型

应用形态：Tauri 桌面应用。

前端：

- React
- TypeScript
- Vite

后端：

- Rust / Tauri commands
- Git 操作调用系统 `git` 可执行文件，而不是绑定 libgit2
- 文件系统、路径规范化、软链接、配置读写和任务队列由 Rust 实现

目录规划：

```text
app/
  package.json
  src/
  src-tauri/
docs/
  skills-collection-app.md
scripts/
  link-skill.sh
  list-skills.sh
  sync-projects.sh
```

CLI 能力继续作为一等能力保留。UI 后端实现应与 CLI 遵循同一套行为规范，脚本作为无 UI fallback。

## Workspace 模型

MVP 管理单个当前 workspace。

启动流程：

- 首次启动选择 collection workspace 目录。
- 应用记住最近使用的 workspace。
- 如果目录为空，可初始化根目录说明和配置。
- 如果目录已有项目，扫描顶层 Git 仓库和 skills。

workspace 是 source of truth：

- 项目和 skills 的真实状态来自文件系统和 Git。
- source skills 必须来自当前 workspace 内部。
- 不支持从任意本地路径导入或链接 skill。

扫描策略：

- 启动时扫描。
- 用户点击刷新时全量扫描。
- 导入、更新、链接、卸载后局部刷新。
- MVP 不做实时 file watcher。

## 配置模型

采用两层配置。

Workspace config：

```text
<workspace>/.skills-collection/config.json
```

存储与 collection 相关、可迁移的 metadata：

- 项目标签
- 项目备注
- 收藏状态
- 隐藏状态
- 项目级自动更新策略
- 用户自定义显示名
- 用户覆盖后的项目分类

User app config：

- 最近 workspace
- agent profiles
- agent skills directory
- UI 偏好
- 窗口大小
- 全局自动检查设置
- 定时检查间隔

原则：

- 文件系统和 Git 状态不依赖 metadata。
- metadata 只存用户偏好、覆盖值和缓存信息。
- agent 安装状态从 agent 目标目录中的软链接实时反查。

## 导入仓库

MVP 支持：

- GitHub `owner/repo` 快捷输入
- HTTPS Git URL
- SSH Git URL
- `git@...`
- `ssh://...`
- 其他任意 Git URL

转换规则：

- `owner/repo` -> `https://github.com/owner/repo.git`
- 完整 URL 原样传给 `git clone`

导入流程：

1. 用户输入仓库地址。
2. 应用推导默认目录名，默认使用 repo name。
3. 用户可在高级选项里修改目录名。
4. 目录名必须是安全的单层目录名，不允许 `/`、空名、`.`、`..`。
5. 执行本地 `git clone`。
6. clone 完成后扫描 README、LICENSE、`SKILL.md`、remote 和 branch。

目标目录已存在时：

- 目录不存在：正常 clone。
- 目录存在且是 Git 仓库：提示已存在，允许纳入管理并扫描，不执行 clone。
- 目录存在但不是 Git 仓库：报错，要求用户改目录名或手动处理。

clone 模式：

- 默认完整 clone。
- 高级选项支持 shallow clone，也就是 `--depth 1`。

MVP 不接 GitHub API，不做仓库搜索、star、README 远程预览或推荐。

## 项目分类

导入后自动识别一次项目类型，并允许用户覆盖。

初始规则：

- 包含 `SKILL.md`：`skills`
- 包含 `.codex-plugin/plugin.json` 或 `.claude-plugin/plugin.json`：`plugins`
- 包含明显 CLI/package 脚本：`tools`
- 包含大量 `DESIGN.md` 或 design references：`design resources`
- 其他：`uncategorized`

用户手动覆盖后不再自动覆盖，除非用户点击“重新识别”。

## Git 状态与更新

MVP 不支持分支切换，只管理当前 checked-out branch 的 upstream。

启动时：

- 只做本地状态扫描。
- 不自动联网。
- 显示 branch、upstream、dirty、detached HEAD、是否存在 remote。

检查更新：

- 用户显式点击“检查更新”时执行 `git fetch --prune`。
- fetch 后计算 ahead/behind。

状态枚举：

- `up_to_date`
- `behind`
- `ahead`
- `diverged`
- `dirty`
- `no_upstream`
- `detached`
- `fetch_failed`
- `unknown`

更新项目：

- 执行 `git pull --ff-only --prune`。
- 默认跳过 dirty working tree。
- 可选 `--autostash` 行为作为高级选项。
- 不自动 merge，不产生 merge commit。

自动更新策略：

- 默认只检查，不自动 pull。
- 用户可以对单个项目或全局开启自动 pull。
- 自动 pull 仍跳过 dirty 项目。
- MVP 支持应用运行期间定时检查更新，不做后台守护或开机自启。

## Skill 扫描与解析

扫描规则：

- 在 workspace 内查找 `SKILL.md`。
- 忽略 `.git` 和 `node_modules`。
- source skill 必须位于当前 workspace 内。

解析字段：

- `name`
- `description`
- `source_project`
- `relative_path`
- `category`
- `has_assets`
- `has_scripts`
- `has_references`
- `installed_agents`
- `last_modified`

解析策略：

- 优先读取 `SKILL.md` frontmatter。
- 没有 frontmatter 时 fallback 到目录名和正文标题。
- 读取正文首段作为详情摘要。
- MVP 做名称、描述、路径的简单搜索，不做全文索引。

README / LICENSE：

- 项目卡片显示 README 摘要和 LICENSE 状态。
- 不做完整富文本 Markdown 浏览器。
- 提供打开 README、打开项目目录、复制路径。

Skill 详情：

- 展示 metadata。
- 预览 `SKILL.md` 前段内容。
- 显示来源项目、相对路径、安装状态。
- 不内置编辑器。
- 可在系统文件管理器或外部编辑器中打开。

## Agent Profiles

软链接目标采用可扩展 agent profiles。

MVP 内置：

- Claude Code: `~/.claude/skills`
- Codex: `~/.codex/skills`

用户可新增自定义 agent profile：

- `id`
- `name`
- `skills_dir`
- `enabled`
- `link_mode`

agent profile 状态：

- skills dir path
- exists / missing
- writable / not writable
- symlink count
- linked skills from current workspace

目标目录缺失时，UI 提供确认后一键创建目录。

## 软链接安装规则

默认链接名：

```text
<repo-name>-<skill-name>
```

示例：

- `skills-tdd`
- `agent-skills-test-driven-development`
- `superpowers-test-driven-development`

原因：

- 当前 collection 中已存在同名 skill，例如 `test-driven-development`。
- 仅使用 skill 目录名容易冲突。

用户可在安装前自定义链接名。

单个安装规则：

- 目标不存在：创建 symlink。
- 目标已是指向同一个 skill 的 symlink：显示已安装，不做事。
- 目标是普通文件或目录：阻止，要求用户手动处理或换名。
- 目标是指向其他位置的 symlink：提示冲突，允许改名或强制替换。
- 目标是同一 repo 的旧路径 symlink：可提供“修复链接”。

批量安装：

1. 用户选择多个 skills。
2. 用户选择一个或多个 target agents。
3. 应用生成冲突预览。
4. 用户确认后执行安全项。
5. 冲突项保持未处理。

预览状态：

- `will_link`
- `already_installed`
- `name_conflict`
- `blocked_by_real_file`
- `blocked_by_real_directory`
- `missing_source`
- `agent_path_missing`
- `agent_path_not_writable`

## 卸载规则

MVP 支持卸载，但只删除 agent 目录中的 symlink。

允许卸载：

- 目标是指向当前 workspace 内某个 skill 的 symlink。

不允许卸载：

- 普通文件
- 普通目录
- 指向 workspace 外部的 symlink

批量卸载也需要预览。

卸载不会修改 source skill，也不会删除项目目录。

## 项目移除

MVP 不提供删除本地项目目录。

支持：

- 隐藏项目
- 清理 missing 项目的 metadata
- 打开项目目录让用户自行处理

未来可以增加危险区删除，但必须：

- 二次确认
- 检查 dirty 状态
- 检查是否有 agent symlink 指向项目内 skills
- 明确展示将删除的路径

## UI 信息架构

主界面以 Skills 为中心，同时保留 Projects 和 Agents 视图。

建议导航：

- Skills
- Projects
- Agents
- Tasks / Logs
- Settings

Skills 视图：

- 搜索 name / description / path
- 按项目过滤
- 按分类过滤
- 按标签过滤
- 按 agent 安装状态过滤
- 单个安装 / 卸载
- 批量选择和安装预览

Projects 视图：

- 项目列表
- remote URL
- branch / upstream
- dirty 状态
- ahead / behind
- README 摘要
- LICENSE 状态
- 导入仓库
- 检查更新
- 更新单个项目
- 更新全部项目
- 隐藏项目

Agents 视图：

- Claude Code
- Codex
- custom profiles
- skills dir 状态
- 一键创建缺失目录
- 查看当前 workspace 安装的 symlink
- 卸载 symlink

Tasks / Logs 视图：

- 最近任务
- 当前任务状态
- 摘要错误
- 可展开原始 stdout / stderr
- 复制日志

Settings：

- workspace 选择
- 最近 workspace
- 自动检查频率
- agent profiles
- UI 偏好

## 任务模型

MVP 采用全局任务队列，默认串行执行。

原因：

- Git、文件系统和 agent 目录都是共享资源。
- 串行任务更容易保证状态一致。
- 日志和错误处理更清晰。

任务状态：

- `queued`
- `running`
- `succeeded`
- `skipped`
- `failed`
- `cancelled`

任务类型：

- `scan_workspace`
- `import_project`
- `fetch_project`
- `pull_project`
- `sync_all_projects`
- `link_skill`
- `link_skills_batch`
- `unlink_skill`
- `unlink_skills_batch`
- `create_agent_dir`

取消语义：

- queued task：直接取消。
- running batch task：当前子步骤结束后停止后续步骤。
- running single git command：MVP 不强杀，等待命令结束。
- UI 显示“取消请求已提交，等待当前 Git 操作结束”。

后续版本可增加安全进程终止和清理策略。

## Tauri Command 边界

建议 commands：

- `select_workspace`
- `get_recent_workspaces`
- `scan_workspace`
- `import_project`
- `check_updates`
- `pull_project`
- `sync_projects`
- `list_skills`
- `preview_link_skill`
- `link_skill`
- `preview_link_skills_batch`
- `link_skills_batch`
- `preview_unlink_skill`
- `unlink_skill`
- `list_agent_profiles`
- `save_agent_profile`
- `create_agent_skills_dir`
- `get_task_status`
- `cancel_task`
- `get_task_logs`

Rust 返回 serde JSON，前端维护对应 TypeScript types。

## 数据模型草案

```ts
type Workspace = {
  root: string;
  projects: Project[];
  skills: Skill[];
};

type Project = {
  id: string;
  name: string;
  path: string;
  remoteUrl?: string;
  provider: "github" | "gitlab" | "unknown";
  branch?: string;
  upstream?: string;
  gitStatus: GitStatus;
  category: ProjectCategory;
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
  installedAgents: InstalledAgentSkill[];
  lastModified?: string;
};

type AgentProfile = {
  id: string;
  name: string;
  skillsDir: string;
  enabled: boolean;
  builtIn: boolean;
  linkMode: "symlink";
};

type InstalledAgentSkill = {
  agentProfileId: string;
  linkName: string;
  targetPath: string;
  sourcePath: string;
  status: "valid" | "broken" | "external" | "conflict";
};

type GitStatus =
  | "up_to_date"
  | "behind"
  | "ahead"
  | "diverged"
  | "dirty"
  | "no_upstream"
  | "detached"
  | "fetch_failed"
  | "unknown";

type ProjectCategory =
  | "skills"
  | "plugins"
  | "tools"
  | "design_resources"
  | "uncategorized";
```

## 安全原则

- 不覆盖普通文件或目录。
- 不删除项目目录。
- 不删除 agent 目录中的真实目录或文件。
- 默认跳过 dirty Git 仓库。
- 默认使用 `git pull --ff-only --prune`。
- 不自动联网，除非用户触发检查更新或开启定时检查。
- 软链接来源必须在当前 workspace 内。
- 软链接卸载只处理指向当前 workspace 的 symlink。
- 所有批量 mutating 操作先预览，再执行。

## MVP 范围

MVP 必须完成：

- Tauri app scaffold。
- 选择并记住单个 workspace。
- 扫描顶层 Git 仓库。
- 扫描和解析 `SKILL.md`。
- 导入 Git 仓库。
- 手动检查更新。
- 手动 pull 单个或全部项目。
- Projects 视图。
- Skills 视图。
- Agents 视图。
- Claude Code / Codex 内置 profiles。
- 创建缺失 agent skills 目录。
- 单个和批量 soft link 安装。
- 单个和批量卸载 symlink。
- 冲突预览。
- 串行任务队列。
- 任务日志。
- workspace config 和 user config。

MVP 不做：

- GitHub API 搜索。
- 推荐市场。
- skill 编辑器。
- 后台守护。
- 实时文件监听。
- 删除本地项目目录。
- 分支切换。
- 完整 Markdown 文档浏览器。
- 推荐安装组合。

## 后续版本

可选增强：

- GitHub API 搜索和导入。
- 项目推荐、评分、收藏榜。
- Presets / skill bundles。
- 有限并发 fetch。
- File watcher。
- 后台定时检查。
- 完整 README/SKILL.md 渲染。
- 外部 skill 来源。
- 安全删除项目。
- 自定义分类规则。
- OpenCode、Gemini CLI、Cursor 等内置 agent profiles。
- 一键生成 collection 报告。

## 实施追踪

- 实施计划：[docs/skills-collection-app-implementation-plan.md](skills-collection-app-implementation-plan.md)
- 任务拆分：[docs/skills-collection-app-tasks.md](skills-collection-app-tasks.md)
