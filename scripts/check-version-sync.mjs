import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const readJson = (path) => JSON.parse(readFileSync(resolve(root, path), "utf8"));

const packageJson = readJson("package.json");
const packageLock = readJson("package-lock.json");
const tauriConfig = readJson("src-tauri/tauri.conf.json");
const cargoToml = readFileSync(resolve(root, "src-tauri/Cargo.toml"), "utf8");
const cargoLock = readFileSync(resolve(root, "src-tauri/Cargo.lock"), "utf8");

const cargoTomlVersion = cargoToml.match(/^\s*version\s*=\s*"([^"]+)"\s*$/m)?.[1];

const cargoLockVersion = cargoLock.match(
  /\[\[package\]\]\s+name = "skilldock"\s+version = "([^"]+)"/m,
)?.[1];

const expected = packageJson.version;
const versions = [
  ["package.json", packageJson.version],
  ["package-lock.json", packageLock.version],
  ['package-lock.json packages[""]', packageLock.packages?.[""]?.version],
  ["src-tauri/tauri.conf.json", tauriConfig.version],
  ["src-tauri/Cargo.toml", cargoTomlVersion],
  ["src-tauri/Cargo.lock", cargoLockVersion],
];

const failures = versions.filter(([, version]) => version !== expected);

const releaseTag = process.env.RELEASE_TAG;
if (releaseTag) {
  const expectedTag = `v${expected}`;
  if (releaseTag !== expectedTag) {
    failures.push(["release tag", releaseTag]);
  }
}

if (failures.length > 0) {
  console.error(`Version mismatch. Expected ${expected}.`);
  for (const [name, version] of failures) {
    console.error(`- ${name}: ${version ?? "missing"}`);
  }
  process.exit(1);
}

console.log(`Version sync OK: ${expected}`);
