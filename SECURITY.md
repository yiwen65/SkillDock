# Security Policy

SkillDock manages local Git repositories and creates symlinks into agent skill directories. Please treat every imported skill repository as executable trust material: skills can include instructions, scripts, references, and assets that may influence agent behavior.

## Supported Versions

Security fixes target the latest version on the `main` branch until formal releases are published.

## Security Model

SkillDock is designed around these safety rules:

- It does not install skills by copying source files into agent directories.
- It creates symlinks only from the current SkillDock workspace into configured agent skill directories.
- It previews install conflicts before mutating agent directories.
- It does not overwrite real files or real directories during install.
- It uninstalls only symlinks that point back into the active workspace.
- It skips dirty Git repositories during update by default.
- It uses `git pull --ff-only --prune` for updates by default.
- It does not fetch, pull, or otherwise go online unless the user explicitly starts an import or update action.

## Reporting a Vulnerability

If you find a vulnerability, please open a private security advisory on GitHub if available. If advisories are not enabled, open an issue with a minimal description and ask for a private contact path before sharing exploit details.

Useful reports include:

- Skill install or uninstall can overwrite or delete real files.
- A symlink can be created from outside the active workspace.
- Dirty repositories are updated without explicit approval.
- A Git operation runs unexpectedly without user action.
- Agent profile paths can be abused to mutate unintended directories.

## Importing Third-Party Skills

SkillDock helps track where skills come from, but it does not certify that third-party skills are safe. Review the source repository, commit history, scripts, and `SKILL.md` content before installing skills into an agent you use for real work.

