# Distribution

## Main → release

A successful [CI](https://github.com/smolijar/catabase/actions/workflows/ci.yml) run on `main` automatically starts [Release macOS](https://github.com/smolijar/catabase/actions/workflows/release.yml). Pull requests run checks without publishing. The release workflow verifies the successful run belongs to this repository and checks out its exact commit.

The workflow:

1. Allocates a version: the checked-in version for the first release; afterward, the next patch above the latest published release, or the checked-in version if it is higher.
2. Updates the Cargo workspace, its lockfile package versions, UI package and Tauri configuration **inside the build checkout only**. It does not commit version bumps back to `main`.
3. Builds one universal macOS app for Apple Silicon and Intel. Complete Apple credentials enable Developer ID signing and notarization; otherwise the app is explicitly labeled as an ad-hoc-signed preview.
4. Verifies the app version, both architectures and its code signature, plus notarization when enabled. Produces the downloads and SHA-256 checksums.
5. Creates a draft, uploads and verifies every asset, then publishes it as the latest GitHub release. Notes list commits since the preceding release, with commit and comparison links.

Downloads are public on [Releases](https://github.com/smolijar/catabase/releases):

| Asset | Purpose |
| --- | --- |
| `Catabase-macos-universal.app.zip` | Complete app bundle used by the one-line installer. |
| `Catabase-macos-universal.dmg` | Drag-to-Applications installer. |
| `SHA256SUMS` | Checksums; run `shasum -a 256 -c SHA256SUMS` after downloading all listed files. |
| `Catabase.app.tar.gz`, `.sig`, `latest.json` | Included when updater signing is configured. The manifest covers both Mac architectures. |

The release tag points to the tested source commit; the packaged version is the version allocated by CI. Building that tag directly from source uses the checked-in base version unless you apply the released version yourself. The tag's release page and app version identify the distributed build.

Release jobs are serialized. GitHub can coalesce pending runs during rapid pushes; all intervening commits are included in the next published changelog. A run older than the latest released source is skipped. Re-running a published source leaves its tag and artifacts untouched; a failed draft can be retried. To retry manually, supply a successful main **CI run ID** to Release macOS. A failed CI run is never eligible.

Workflow artifacts are retained for 14 days for diagnostics; published release downloads do not share that expiry. An incomplete upload stays a draft and never becomes the update feed. Do not manually publish incomplete drafts or move published version tags.

## Installation

```sh
curl -fsSL https://raw.githubusercontent.com/smolijar/catabase/main/install.sh | sh
```

The installer resolves one release version, downloads its app ZIP and checksums from that exact tag, verifies the bundle/version/architecture/signature, and installs it in `~/Applications`. It refuses to replace a running app and keeps the previous app until the replacement succeeds. It does not change Catabase's data directory or remove macOS quarantine/security attributes. To choose a different writable folder, download `install.sh` and run `sh install.sh --dir /absolute/path`. The [README](../README.md) also links the manual DMG download.

Ad-hoc previews are **not Apple notarized**. macOS may block them; the installer does not bypass Gatekeeper. Add Apple credentials below for normal distribution.

## Signing setup

Apple signing and updater signing are independent. Main releases work without either, but partial configuration fails early instead of silently producing a differently signed build. Once a published release enables either, later releases cannot silently drop it. Changing the updater public key is also blocked: existing clients need a deliberate key-rotation migration.

Add these repository secrets under **Settings → Secrets and variables → Actions**:

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64-encoded exported Developer ID Application `.p12`, including its private key. |
| `APPLE_CERTIFICATE_PASSWORD` | Password used when exporting the `.p12`. |
| `APPLE_SIGNING_IDENTITY` | Full `Developer ID Application: … (TEAMID)` identity. Ad-hoc `-` is rejected when Apple credentials are configured. |
| `APPLE_ID` | Apple Developer account email for notarization. |
| `APPLE_PASSWORD` | Apple app-specific password for notarization. |
| `APPLE_TEAM_ID` | Apple Developer team ID. |
| `TAURI_SIGNING_PRIVATE_KEY` | Contents of the Tauri updater private key. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Updater-key password, if set. |

Add the repository **variable** `TAURI_SIGNING_PUBLIC_KEY` containing the matching updater public key. The build embeds this key and the fixed endpoint:

```text
https://github.com/smolijar/catabase/releases/latest/download/latest.json
```

Follow the official [macOS signing guide](https://v2.tauri.app/distribute/sign/macos/) to create/export the Apple certificate. Generate the separate update-signing key using the pinned Tauri CLI:

```sh
pnpm tauri signer generate -w /secure/path/catabase-updater.key
```

Back up the private key and password securely; store the public `.pub` file contents in `TAURI_SIGNING_PUBLIC_KEY`. Keep private keys out of Git and workflow artifacts. Existing apps trust the embedded public key, so changing or losing its private counterpart breaks their ability to accept updates. See the [Tauri updater guide](https://v2.tauri.app/plugin/updater/) for key details.

## In-app updates

The app checks the published release feed and offers an update from Settings. Installation requires updater signing to be configured **in the installed build**. An older build without the public key needs one manual upgrade to a signed-updater build before future in-app installation works.

Release validation checks the generated signature belongs to the configured public key (the pinned CLI otherwise only warns on a mismatch). The runtime updater cryptographically checks the archive signature against that embedded key. Feed URLs point to immutable version tags, not the moving `latest` archive. Installing an update must wait for active work to finish and save workspace drafts before relaunch. Replacing the app does not replace local connections/history; a binary downgrade alone is not a database rollback strategy.

Before declaring the update channel ready, test an actual older-version → newer-version upgrade and preservation of saved SQL, connections and history. The pipeline can build updater artifacts as soon as its keypair is configured; Apple notarization remains a separate distribution requirement.

## Development and platform scope

Use the [development checks](development.md#checks) before pushing. CI uses Node 24, pnpm 10.33.0, Rust 1.96.1 and macOS 15 runners, with checked-in lockfiles and pinned actions. It does not access saved database connections or enable live Docker/Supermaven tests.

To start a new minor/major release line, raise the common version in `Cargo.toml`, `Cargo.lock`, `ui/package.json` and `src-tauri/tauri.conf.json`; `scripts/check-release-version.py` validates them. Routine main pushes do not need manual version changes.

Windows and Linux packages are deferred; distribution currently targets macOS only.
