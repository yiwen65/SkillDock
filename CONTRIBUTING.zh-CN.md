# SkillDock 贡献指南

[English](CONTRIBUTING.md) | **简体中文**

本文说明日常改动落地到 `main`、以及发布 release 的标准流程。这份流程反映了仓库实际的配置 —— branch protection、4 个必需 CI 检查、rebase-merge、三处版本号 —— 照着做就不会在 merge 环节卡住。

相关文档:

- [`AGENTS.md`](AGENTS.md) —— 项目结构、编码规范、常用命令
- [`docs/RELEASING.md`](docs/RELEASING.md) —— 发布流程细节,包括 macOS 代码签名

## `main` 上的必需 CI 检查

每个 PR 必须这 4 项全绿才能 merge:

| 检查 | 运行环境 | 典型耗时 |
|---|---|---|
| Quality (lint, types, build, audit) | ubuntu-latest | ~20s |
| Rust tests (ubuntu-latest) | ubuntu-latest | ~25s |
| Rust tests (macos-latest) | macos-latest | ~45s |
| Playwright (chromium) | ubuntu-latest(在 Quality 通过后跑) | ~1 分钟 |

Branch protection 已开启:`main` 要求线性历史,禁止 force-push / 删除,4 个检查不全绿就不能 merge。Admin 在紧急情况下可绕过,但不应当作默认。

## 场景 A —— 修 bug / 加功能

任何要合进 `main` 的代码改动,走这个默认流程。

```bash
# 0. 前置:main 干净、同步
git checkout main && git pull --ff-only

# 1. 按改动性质命名新分支(前缀对照见下表)
git checkout -b fix/login-race

# 2. 改完之后在本地跑一遍 CI 要跑的检查
npm run lint
npm run format:check
npm run typecheck
npm run build
npm run ui:smoke
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run e2e                       # 只在改了 UI 或 e2e 本身时需要

# 3. Conventional Commits 风格提交
git add <files>
git commit -m "fix(scope): 一行总结"

# 4. push + 开 PR
git push -u origin fix/login-race
gh pr create --base main --head fix/login-race \
  --title "fix(scope): ..." \
  --body "背景/问题 · 改动清单 · 本地测试结果"

# 5. 看 CI,4 个必须全绿
gh pr checks <PR#>                # 或 gh pr view <PR#> --web

# 6. rebase merge 并删分支
gh pr merge <PR#> --rebase --delete-branch

# 7. 本地同步 main
git checkout main && git pull --ff-only
```

实用建议:

- **在 push 之前**跑一遍本地检查比等 CI 便宜多了,能避开一次 3 分钟的 CI 往返。唯一没法本地复现的是 macOS 上的 Rust 测试,那个只能在 CI 上见分晓。
- `npm run format:check` 挂了就跑 `npm run format`;`npm run lint` 挂了就跑 `npm run lint:fix`。
- Playwright job 依赖 Quality。Quality 挂了 Playwright 会自动跳过,不用单独处理。

## 场景 B —— 发布 release

前置:`main` 上已经是想发布的代码状态,所有计划中的 PR 都已经合了。

```bash
# 1. 用独立 PR 同步三处版本号(走完整场景 A 流程)
#
#    - package.json                 "version": "0.2.0"
#    - src-tauri/tauri.conf.json    "version": "0.2.0"
#    - src-tauri/Cargo.toml         version = "0.2.0"
#
#    commit 消息通常是 `chore(release): bump to v0.2.0`

# 2. bump PR merge 后,本地同步 main
git checkout main && git pull --ff-only

# 3. 打 annotated tag(tag message 就是 release notes 摘要)
git tag -a v0.2.0 -m "v0.2.0 —— 一句话描述 + 亮点"

# 4. push tag,release workflow 自动触发
git push origin v0.2.0

# 5. 看进度(整个 workflow 大约 6–7 分钟)
gh run list --workflow=release.yml --limit 1
gh run watch <run-id>

# 6. 验证已发布 release
gh release view v0.2.0 --json assets --jq '.assets[].name'
```

release workflow 自动做的事:

1. 为 tag 创建(或查找)一个 draft release
2. 并行在三个 runner 上构建:
   - `macos-latest` → universal `.dmg`、`.app.tar.gz`、`.zip`
   - `ubuntu-22.04` → x86_64 `.deb`、`.rpm`、`.AppImage`
   - `ubuntu-22.04-arm` → aarch64 `.deb`、`.rpm`、`.AppImage`
3. 三个 build 全成功后把 draft 翻成正式 release

一共 9 个资产。Windows 有意不包含。

## 场景 C —— 重发同一个版本号的 release

**仅**在构建失败、版本号没必要动的情况下用(比如 CI 或打包问题导致上次没成功)。如果 release 已经实际发出去、有人下载过,就**不要**重发——bump 一个 patch 版本号更干净。

```bash
# 1. 删掉挂掉的 release 和对应 tag(远程 + 本地)
gh release delete v0.2.0 --cleanup-tag --yes
git tag -d v0.2.0

# 2. 按场景 A 合入修复

# 3. 在修好的 main 上重新打 tag
git checkout main && git pull --ff-only
git tag -a v0.2.0 -m "..."
git push origin v0.2.0
```

## 分支前缀 ↔ commit 类型对照表

| 分支前缀 | commit 类型 | 用在什么改动 |
|---|---|---|
| `fix/` | `fix:` | bug 修复 |
| `feat/` | `feat:` | 用户可见的新功能 |
| `chore/` | `chore:` | 依赖升级、版本号 bump、license、杂项 |
| `ci/` | `ci:` | GitHub Actions、Dependabot、branch protection 等 |
| `test/` | `test:` | 只改测试(行为不变) |
| `docs/` | `docs:` | 只改文档 |
| `refactor/` | `refactor:` | 不改行为的重构 |
| `perf/` | `perf:` | 只改性能的优化 |

subject 控制在 ~70 字符以内,祈使句语气。body 说**为什么**和**怎么验证**,别只复述 diff。

## commit 消息模板

```
<type>(<scope>): 一行总结

较长的正文,说明这是为了解决什么问题,采取的方案,以及考虑过的
权衡。每行 72 字符左右换行。

Testing:
- npm run lint / format:check / typecheck / build / ui:smoke — 通过
- cargo test --locked — 通过
- npm run e2e(如果改了 UI)— 通过

Closes #<issue>
```

## CI 挂了怎么办

- **lint / format**:跑 `npm run lint:fix` + `npm run format`,commit 再推。不要禁用规则。
- **types / build**:读报错信息,修根因。不要 `// @ts-ignore`。
- **Rust 测试**:本地 `cargo test --manifest-path src-tauri/Cargo.toml --locked`。只在 macOS 挂的通常涉及文件系统大小写或软链接,参考 [`src-tauri/tests/git_operations.rs::temp_dir`](src-tauri/tests/git_operations.rs) 的修复模式。
- **Playwright**:UI 标签或流程可能漂移了。本地 `npm run e2e` 复现,改 spec 去匹配当前 UI —— 不是反过来。
- **npm audit**:当前 `--audit-level=high`。如果某个 moderate 升到 high,通过 Dependabot 或手动升级对应依赖。

代码没改就重跑 workflow 基本不是正确答案。

## Dependabot

每周一 09:00(Asia/Shanghai)为 npm、Cargo、GitHub Actions 三个生态开 PR(每个生态 5 个 PR 上限,minor + patch 分组,major 独立)。它们和人写的 PR 走同样的 CI gate。合并策略:

- Minor / patch 升级:CI 绿就合。
- Major 升级:扫一眼 changelog 里的 breaking change,如果 bump 的是 React / Vite / Playwright 就本地跑 `npm run e2e`,没问题就合。
- 如果某个 major bump 需要配套重构(比如 React 升 v19 会带来新 lint rule),关掉该 PR 并留评论指向跟踪 issue,合并为一个独立 PR 一起做。
