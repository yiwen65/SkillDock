<div align="center">
  <img src="public/app-icon.png" alt="SkillDock" width="128" height="128" />
  <h1>SkillDock</h1>
  <p><strong>本地优先的 agent skills / plugins / tools 收藏夹管理器</strong></p>
  <p><a href="README.md">English</a> | <strong>简体中文</strong></p>
  <p>
    <img src="https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri" alt="Tauri 2" />
    <img src="https://img.shields.io/badge/React-18-61DAFB?logo=react" alt="React 18" />
    <img src="https://img.shields.io/badge/TypeScript-5-3178C6?logo=typescript" alt="TypeScript" />
    <img src="https://img.shields.io/badge/Rust-stable-B7410E?logo=rust" alt="Rust" />
    <img src="https://img.shields.io/badge/platform-Linux%20%7C%20macOS-lightgrey" alt="platform" />
    <img src="https://img.shields.io/badge/status-MVP-brightgreen" alt="status" />
  </p>
</div>

---

SkillDock 把一组 `git clone` 来的 agent 相关仓库（skills、plugins、tools、design resources）集中到一个 **workspace**，用图形界面完成导入、更新检查、`SKILL.md` 预览、以及向 Claude Code / Codex / 自定义 agent 的软链接安装和卸载。它是此前 `scripts/link-skill.sh`、`scripts/list-skills.sh`、`scripts/sync-projects.sh` 三个 shell 工具的 UI 封装与统一抽象。

## 界面预览

![SkillDock Skills 视图](docs/images/skills-view.png)

Skills 视图：左侧导航、顶部 workspace 概览（Projects / Skills / Agents / Installs），中部是带搜索与过滤的 skill 列表，右侧是选中 skill 的安装目标面板，可单独或批量软链接到已配置的 agent 目录。

## 核心能力

- **Workspace 管理** —— 选择并记住一个 collection 根目录，启动时扫描顶层 Git 仓库与 `SKILL.md`。
- **Git 导入与更新** —— 支持 `owner/repo`、HTTPS、SSH、`git@...` 等任意 Git URL；显式“检查更新”后使用 `git pull --ff-only --prune`，默认跳过 dirty 工作区。
- **Skill 扫描与预览** —— 解析 `SKILL.md` frontmatter、来源项目、相对路径、assets / scripts / references 标记与当前安装状态。
- **Agent Profiles** —— 内置 Claude Code (`~/.claude/skills`) 和 Codex (`~/.codex/skills`)，支持自定义 agent；可一键创建缺失目录。
- **安全软链接** —— 默认链接名 `<repo>-<skill>` 避免冲突；单个与批量安装均先生成冲突预览，不覆盖真实文件或真实目录。
- **安全卸载** —— 只移除指向当前 workspace 内 skill 的 symlink，从不触碰外部路径、真实文件或源项目。
- **串行任务队列 & 日志** —— Git、FS、agent 目录是共享资源，任务默认串行执行，可在 Tasks / Logs 视图查看 stdout / stderr。

设计上刻意 **不做** 的事：GitHub 搜索 / 推荐市场、skill 编辑器、后台守护、实时 file watcher、删除本地项目目录、分支切换、完整 Markdown 渲染器。详见 [docs/skilldock.md](docs/skilldock.md) 的“非目标”。

## 技术栈

| 层 | 选型 |
| -- | ---- |
| 壳 | [Tauri 2](https://tauri.app/) |
| 前端 | React 18 + TypeScript + Vite 5 |
| 后端 | Rust（Tauri commands）|
| Git | 直接调用系统 `git`，**不** 绑定 libgit2 |
| 测试 | Playwright（E2E）+ `cargo test`（Rust 单测/集成）|

## 环境要求

- Linux 或 macOS
- Node.js ≥ 18（推荐通过 nvm）
- Rust stable 工具链（`rustup`）
- 系统可用的 `git`
- Linux 还需 WebKitGTK / GTK 相关依赖，按 [Tauri 官方前置依赖](https://tauri.app/start/prerequisites/) 安装

## 快速开始

```bash
# 克隆
git clone https://github.com/yiwen65/SkillDock.git
cd SkillDock

# 安装前端依赖
npm install

# 启动桌面开发模式（Tauri + Vite）
npm run tauri:dev
```

首次运行 Rust 依赖会全量编译（含 `webkit2gtk-sys` 等原生依赖），需要几分钟。后续增量构建会快得多。

仅调试前端（无 Tauri 外壳）：

```bash
npm run dev         # Vite dev server at http://127.0.0.1:1420
```

构建发布产物：

```bash
npm run tauri:build # 产物在 src-tauri/target/release/bundle/
```

## 常用脚本

| 命令 | 说明 |
| ---- | ---- |
| `npm run dev` | 仅启动 Vite |
| `npm run build` | `tsc` + `vite build` |
| `npm run tauri:dev` | 完整桌面开发模式 |
| `npm run tauri:build` | 构建桌面产物 |
| `npm run e2e` | Playwright E2E 测试 |
| `npm run ui:smoke` | UI smoke 脚本（`scripts/ui-smoke.mjs`）|
| `npm run rust:test` | `cargo test` Rust 后端测试 |

## 目录结构

```text
SkillDock/
├── src/                    # React 前端
│   ├── App.tsx
│   ├── CoreView.tsx
│   ├── views/              # Skills / Projects / Agents / Tasks / Settings
│   └── lib/                # Tauri commands, types, 共享工具
├── src-tauri/              # Rust 后端 (Tauri)
│   ├── src/
│   │   ├── workspace.rs    # workspace 扫描
│   │   ├── scanner.rs      # SKILL.md 解析
│   │   ├── git_ops.rs      # git fetch / pull / status
│   │   ├── links.rs        # symlink 安装 / 卸载 / 预览
│   │   ├── agents.rs       # agent profile 管理
│   │   ├── tasks.rs        # 串行任务队列
│   │   └── config.rs       # workspace + user 配置
│   └── tests/              # 后端集成测试
├── scripts/                # 保留的 shell fallback 与工具脚本
├── tests/e2e/              # Playwright E2E
├── docs/                   # 设计文档
│   ├── skilldock.md                     # 设计文档（产品 + 架构）
│   ├── skilldock-implementation-plan.md # 实施计划
│   ├── skilldock-tasks.md               # 任务拆分
│   └── images/                          # README 资源
└── public/                 # 前端静态资源（app 图标等）
```

## 配置模型

两层配置，详见设计文档。

- **Workspace config** — `<workspace>/.skilldock/config.json`：项目标签、备注、收藏/隐藏、自定义显示名等与 collection 绑定的 metadata。
- **User app config** — 最近 workspace、agent profiles、UI 偏好、检查间隔等与用户绑定的设置。

原则：**文件系统与 Git 是真相来源**；config 只存用户偏好与覆盖值，agent 安装状态永远由 agent 目录中的 symlink 实时反查。

## 安全原则

- 不覆盖普通文件或目录
- 不删除项目目录
- 不删除 agent 目录中的真实文件
- 默认跳过 dirty Git 仓库
- 默认使用 `git pull --ff-only --prune`
- 不自动联网，除非用户显式触发
- 软链接来源必须在当前 workspace 内
- 所有批量 mutating 操作先预览再执行

## 文档

- [产品与架构设计](docs/skilldock.md)
- [实施计划](docs/skilldock-implementation-plan.md)
- [任务拆分](docs/skilldock-tasks.md)
- [AGENTS 协作约定](AGENTS.md)

## 项目状态

MVP 开发中，详见 [docs/skilldock-tasks.md](docs/skilldock-tasks.md)。欢迎 Issue 与 PR。

## License

暂未设定显式 License，使用前请自行评估。计划在首个对外发布版本前补齐 `LICENSE` 文件。
