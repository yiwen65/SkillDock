# Releasing

This repo ships Linux and macOS bundles from tagged releases via `.github/workflows/release.yml`. Windows is intentionally excluded.

## Cutting a release

```bash
# on an up-to-date main
git tag -a v0.2.0 -m "v0.2.0"
git push origin v0.2.0
```

The workflow:

1. Creates (or finds) a draft GitHub release for the tag.
2. Builds in parallel on three runners:
   - `macos-latest` → universal `.dmg`, `.app.tar.gz`, `.zip`
   - `ubuntu-22.04` → x86_64 `.deb`, `.rpm`, `.AppImage`
   - `ubuntu-22.04-arm` → aarch64 `.deb`, `.rpm`, `.AppImage`
3. Flips the draft to published once every build succeeds.

If any build fails, the release stays in draft so you can fix the issue and re-run the workflow (Actions tab → **Release** → **Run workflow** with the same tag).

## Version management

Three files must agree on the version:

- `package.json` `"version"`
- `src-tauri/tauri.conf.json` `"version"`
- `src-tauri/Cargo.toml` `version = "..."`

Bump all three together before tagging. The tag must be the same semver value prefixed with `v`.

## Code signing — macOS (optional)

The release workflow ships **unsigned** macOS bundles by default. Users see a Gatekeeper warning on first launch (`"SkillDock" cannot be opened because the developer cannot be verified`) and must right-click → Open or clear the quarantine attribute manually.

To ship signed + notarized binaries you need an Apple Developer account ($99/year) and the six secrets below. They are read by `tauri-action` — no workflow changes required once the secrets exist.

### Required repo secrets

Set these in **Settings → Secrets and variables → Actions**:

| Secret | Where it comes from |
|---|---|
| `APPLE_CERTIFICATE` | Base64 of the exported `.p12` (see below) |
| `APPLE_CERTIFICATE_PASSWORD` | The password you chose when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | The Common Name, e.g. `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID` | Apple ID email used to create the app-specific password |
| `APPLE_PASSWORD` | **App-specific** password from [appleid.apple.com](https://appleid.apple.com) → Sign-In and Security → App-Specific Passwords |
| `APPLE_TEAM_ID` | 10-character team ID from [developer.apple.com/account](https://developer.apple.com/account) → Membership |

### One-time certificate setup

1. In [developer.apple.com/account/resources/certificates](https://developer.apple.com/account/resources/certificates), create a **Developer ID Application** certificate. Follow the CSR flow (Keychain Access → Certificate Assistant → Request a Certificate From a Certificate Authority).
2. After downloading and double-clicking it, open Keychain Access → **login** keychain → My Certificates, find the new cert, right-click → **Export**, choose `.p12`, and set a password.
3. Base64-encode the `.p12`:
   ```bash
   base64 -i developer_id_application.p12 | pbcopy
   ```
4. Paste the clipboard contents as the `APPLE_CERTIFICATE` secret value. Set `APPLE_CERTIFICATE_PASSWORD` to the password from step 2.
5. Find the signing identity string:
   ```bash
   security find-identity -v -p codesigning
   ```
   Copy the value in quotes (for example `Developer ID Application: Jane Doe (ABCDE12345)`) into `APPLE_SIGNING_IDENTITY`.
6. Generate an app-specific password at [appleid.apple.com](https://appleid.apple.com) and save it as `APPLE_PASSWORD`. Save your Apple ID email as `APPLE_ID` and your team ID as `APPLE_TEAM_ID`.

The next release workflow run will sign the `.app`, wrap it in a signed `.dmg`, submit for notarization (Apple's automated malware check), and staple the ticket. Signed bundles launch without Gatekeeper warnings.

### Wiring the env block

After the six Apple secrets are set, paste this block into the `Build and upload bundles` step of `.github/workflows/release.yml` (directly below `GITHUB_TOKEN`):

```yaml
          APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
          APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
          APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
          APPLE_ID: ${{ secrets.APPLE_ID }}
          APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}
          APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
```

The env variables are pasted rather than left in the workflow permanently because Tauri's bundler treats a defined-but-empty `APPLE_CERTIFICATE` as "please sign" and then fails on the missing keychain import. Passing the block in only once the secrets actually contain values keeps the unsigned path reliable.

## Code signing — Linux

Not currently set up. `.AppImage` can be signed with GPG; `.deb` and `.rpm` each use their own signing tooling. If you want to enable this, open an issue — it's additive to the release workflow.

## Tauri updater signing (optional)

If you later add an in-app updater, Tauri can verify downloads using an Ed25519 keypair. The release workflow already wires the env vars; you just need to provide secrets.

1. Generate a keypair:
   ```bash
   npx tauri signer generate -w ~/.tauri/skilldock.key
   ```
   This prints a public key — paste it into `src-tauri/tauri.conf.json` under `plugins.updater.pubkey` when you turn the updater on.
2. Store the private key contents as the `TAURI_SIGNING_PRIVATE_KEY` secret. If you set a password, store it as `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
3. Paste this block into `.github/workflows/release.yml` under `Build and upload bundles` (same reasoning as the Apple block: only add once secrets exist):

   ```yaml
             TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
             TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
   ```

With these present, `tauri-action` produces `.sig` files alongside the updater bundles (`.app.tar.gz`, `.AppImage`). Without them the release still builds, just without signatures.

## Troubleshooting

- **`tauri build` hangs on Linux for minutes** — first build pulls the full Rust dependency graph (webkit2gtk, rsvg, etc.). Subsequent builds use the `Swatinem/rust-cache@v2` cache.
- **macOS notarization fails with `errSecInternalError`** — usually means `APPLE_ID` and `APPLE_PASSWORD` are out of sync, or the password is a regular Apple ID password instead of an app-specific one.
- **`.AppImage` won't run on older distros** — check glibc version. The workflow uses `ubuntu-22.04` for broader compatibility; older Ubuntu LTS targets would need custom runners.
- **Release stays in draft after workflow success** — only the `publish` job flips it. If `publish` didn't run, check whether all three `build` matrix legs succeeded.
