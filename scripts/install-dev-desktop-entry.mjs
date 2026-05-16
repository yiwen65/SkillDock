import { copyFileSync, mkdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";

function commandExists(command) {
  const result = spawnSync(command, ["--version"], { stdio: "ignore" });
  return result.status === 0;
}

if (!commandExists("cargo")) {
  console.error(`Missing Rust toolchain: SkillDock's Tauri dev mode requires cargo on PATH.

Install Rust with rustup, then restart your terminal:
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

After installation, verify:
  cargo --version

Then rerun:
  npm run tauri:dev`);
  process.exit(1);
}

const appId = "dev.skilldock.app";
const fallbackId = "skilldock";
const root = process.cwd();
const localShare = join(homedir(), ".local", "share");
const iconRoot = join(localShare, "icons", "hicolor");
const applicationsRoot = join(localShare, "applications");
const binaryTarget = join(root, "src-tauri", "target", "debug", "skilldock");

const iconSources = [
  ["32x32", "32x32.png"],
  ["128x128", "128x128.png"],
  ["256x256@2", "128x128@2x.png"],
  ["512x512", "icon.png"],
];

for (const id of [appId, fallbackId]) {
  for (const [size, filename] of iconSources) {
    const target = join(iconRoot, size, "apps", `${id}.png`);
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(join(root, "src-tauri", "icons", filename), target);
  }
}

mkdirSync(applicationsRoot, { recursive: true });

for (const [desktopId, startupWmClass] of [
  [appId, appId],
  [fallbackId, fallbackId],
]) {
  writeFileSync(
    join(applicationsRoot, `${desktopId}.desktop`),
    `[Desktop Entry]
Categories=Development;
Comment=Desktop manager for a local SkillDock workspace
Exec=${binaryTarget}
StartupWMClass=${startupWmClass}
Icon=${appId}
Name=SkillDock
StartupNotify=true
Terminal=false
Type=Application
`,
  );
}

spawnSync("gtk-update-icon-cache", ["-f", "-t", iconRoot], { stdio: "ignore" });
