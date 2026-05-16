# SkillDock GitHub Discovery Plan

This document is the public-facing cleanup plan for making SkillDock easier to understand, search, and share.

## Current Diagnosis

SkillDock's low visibility is not mainly a code problem. The repo currently looks like a mature local tool after you read the README, but the first screen does not make the value obvious fast enough.

Observed issues:

- **Name collision**: `SkillDock` is already used by skilldock.io / docs.skilldock.io, so generic searches for "SkillDock" do not reliably surface this repository.
- **Weak first-screen positioning**: the old tagline said "agent skills, plugins, and tools collection", which was broad but did not strongly match what people search for: `AI agent skills`, `Claude Code skills`, `Codex skills`, `SKILL.md`, `skill manager`, `symlink installer`.
- **Too much implementation framing too early**: Tauri, React, Rust, and shell-script history matter to contributors, but users first need the pain point and workflow.
- **Missing repository metadata strategy**: GitHub description and topics are part of search and browsing discovery. They cannot be committed as normal files, so they need to be set manually in the GitHub UI.
- **License adoption friction**: the current PolyForm Noncommercial license is source-available, not OSI open source. That is a valid business choice, but it reduces the chance that companies and open-source directories recommend or adopt the project.

## Search Landscape

SkillDock should not position itself only as "a skill manager". That phrase is already crowded by adjacent products:

- `skilldock.io` / `docs.skilldock.io`: hosted SkillDock platform, CLI, API, search, install, and publish flow.
- `skilltap`: "Homebrew for AI agent skills" with tap-style package management.
- `SkillUse`: GitHub-based registry for AI agent skills.
- VS Code "Skill Dock": editor extension for browsing and installing skills.
- `SkillDeck`: UI-oriented AI code agent skills manager.

The clearest differentiator for this repository is:

```text
local-first Git workspace + safe symlink installer + multi-agent inventory
```

Use that phrase repeatedly. It separates SkillDock from registries, marketplaces, package managers, and editor extensions.

## Recommended GitHub Description

Use this in the GitHub repository About section:

```text
Local-first Git workspace and symlink installer for AI agent skills across Claude Code, Codex, and custom agents.
```

Shorter alternative:

```text
Manage AI agent skills from Git repos and install them into Claude Code, Codex, and custom agents with symlinks.
```

Chinese social tagline:

```text
用 Git 管来源，用软链接管安装，用一个本地工作区管理 Claude Code、Codex 和自定义 Agent Skills。
```

## Recommended GitHub Topics

Set these repository topics manually in GitHub:

```text
ai-agent-skills
agent-skills
claude-code
codex
skill-manager
skill-installer
skill-management
skill-library
skill-workspace
skill-registry
skill-md
symlink
git-workspace
tauri
desktop-app
```

If GitHub rejects uncommon topics, keep the closest accepted topics and prioritize the first eight.

## Project Name Strategy

Keep `SkillDock` if you want a short product name, but do not rely on the bare name for discovery. Always pair it with a descriptive subtitle:

```text
SkillDock: local Git workspace for AI Agent Skills
```

Rename only if search collision becomes a real blocker. Safer rename candidates:

```text
SkillDock Local
Agent SkillDock
SkillDock Desktop
SkillDock Workspace
```

The current best move is to keep the repo URL stable and improve the title, description, README, and topics.

## README Positioning

The README should answer these questions before the screenshot:

- What pain does this solve?
- Who is it for?
- Why Git instead of copying files?
- Why symlinks instead of separate installs?
- Which agents are supported?

Current positioning to repeat consistently:

```text
SkillDock is a local-first desktop manager for AI agent skills. It imports skill repositories with Git, scans SKILL.md folders, and installs skills into Claude Code, Codex, or custom agents with safe symlinks.
```

## Search Queries To Optimize For

Use these phrases naturally in README, releases, issues, and launch posts:

```text
AI agent skills manager
Claude Code skills manager
Codex skills manager
SKILL.md manager
install agent skills from GitHub
sync agent skills across agents
local skill workspace
symlink skill installer
multi-agent skill manager
agent skills desktop app
```

## Launch Post Template

```text
I built SkillDock, a local-first desktop manager for AI agent skills.

It solves a problem I kept running into: skills live in many GitHub repos, but installing them into Claude Code, Codex, and custom agents usually means copying folders around.

SkillDock keeps every source repo in one Git workspace, scans SKILL.md folders, and installs skills with symlinks. That means one source of truth, easy Git updates, and no duplicated skill folders.

Useful if you:
- maintain your own private skill repo
- try community skill collections
- use both Claude Code and Codex
- want installs to stay traceable to the original Git repo

Repo: https://github.com/yiwen65/SkillDock
```

## Highest-Leverage Next Steps

1. Set GitHub description and topics from this document.
2. Add a short demo GIF or 60-second video above the screenshot.
3. Create a GitHub Release with binary downloads for macOS and Linux once the MVP is usable by non-developers.
4. Consider switching to Apache-2.0 or MIT if the goal is broad open-source adoption. If commercial licensing is important, keep PolyForm but call the project "source-available" consistently.
5. Publish one focused launch post in communities where the pain is real: Claude Code, Codex, AI agents, GitHub projects, and personal devtool communities.
