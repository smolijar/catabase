# Distribution

## What works before signing is configured

The [CI workflow](https://github.com/smolijar/catabase/actions/workflows/ci.yml) runs on pull requests, pushes to `main`, and manual dispatch. It checks Rust formatting, Clippy, Rust tests, Svelte/TypeScript, frontend tests, the frontend build, and release-version consistency. It does not access a saved database connection or enable the opt-in Docker/history/Supermaven tests.

After the checks pass, main/manual runs build a universal macOS application containing both Apple Silicon and Intel binaries. Open a successful run and download **Catabase-macos-universal-preview-…** under **Artifacts**. It contains:

- `Catabase-macos-universal-preview.app.zip`, containing the complete app bundle with executable permissions preserved;
- a DMG;
- `SHA256SUMS` for verifying the files with `shasum -a 256 -c SHA256SUMS`.

Preview artifacts expire after 14 days and require a GitHub login to download. They are ad-hoc signed, without Apple Developer ID signing or notarization, so macOS may block them. They are for testing the packaging pipeline and do not create a public GitHub Release or update feed. They are not suitable as the endpoint of a public `curl | sh` installer.

Node 24, pnpm 10.33.0, Tauri CLI 2.11.4, Rust 1.96.1, and macOS 15 runners are used in CI. JavaScript and Rust builds use the checked-in lockfiles. The existing pnpm dependency-age policy remains enabled. Actions are pinned to commit hashes.

## Signing setup for the first release

The [Release macOS workflow](https://github.com/smolijar/catabase/actions/workflows/release.yml) runs when a `v*` tag is pushed. It can also be rerun manually on an existing version tag. Its preflight fails with the names of missing credentials before any release build starts. Regular CI and preview builds need none of these secrets.

Add these repository secrets under **Settings → Secrets and variables → Actions** when ready:

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64-encoded exported Developer ID Application `.p12` certificate, including its private key. |
| `APPLE_CERTIFICATE_PASSWORD` | Password used when exporting the `.p12`. |
| `APPLE_SIGNING_IDENTITY` | Full `Developer ID Application: … (TEAMID)` identity. Ad-hoc `-` is rejected. |
| `APPLE_ID` | Apple Developer account email for notarization. |
| `APPLE_PASSWORD` | Apple app-specific password for notarization, not the account's normal password. |
| `APPLE_TEAM_ID` | Apple Developer team ID. |
| `TAURI_SIGNING_PRIVATE_KEY` | Contents of the Tauri updater private key. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password for that updater key, if it has one; otherwise leave unset. |

Also add the repository **variable** `TAURI_SIGNING_PUBLIC_KEY` containing the public key paired with `TAURI_SIGNING_PRIVATE_KEY`. Tauri's build checks require it to generate and verify updater signatures. CI writes it into a temporary release configuration; no placeholder key is committed.

Apple signing and updater signing are separate. Follow the official [macOS signing guide](https://v2.tauri.app/distribute/sign/macos/) to create/export the Apple certificate and configure notarization, and the [updater guide](https://v2.tauri.app/plugin/updater/) to generate and securely back up the update-signing key pair. Keep private keys out of Git and workflow artifacts. The same public updater key must be used when the in-app updater is implemented.

The workflow passes Apple credentials to Tauri's signing/notarization integration. It waits for notarization, checks the resulting app's signatures and stapled ticket, and creates a **draft** GitHub release with the DMG, signed updater archive/signature, and `latest.json`. A failed build may leave an incomplete draft; publish only after the whole workflow is green and both architectures have been tested. First-time Apple notarization can be slow; a timeout leaves the release unpublished and can be retried.

## Release process

1. Update the version together in `Cargo.toml` (`workspace.package.version`), `src-tauri/tauri.conf.json`, and `ui/package.json`. Run Cargo to update the workspace package versions in `Cargo.lock`; include that change in the same commit.
2. Run the checks in the README, including `python3 scripts/check-release-version.py --tag vX.Y.Z`. Prerelease tags such as `v0.2.0-beta.1` are supported; build-metadata suffixes are not.
3. Commit and push the version change to `main`, then create and push the matching tag. For example, after setting the version to `0.2.0`:

   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

4. Wait for **Release macOS** to succeed. Download and test the draft's packages on Apple Silicon and Intel. Check that existing connections, history and SQL drafts survive an upgrade.
5. Review the generated release notes and publish the draft from GitHub. Prerelease tags create prerelease drafts.

Do not move a published version tag or replace its assets. Use a new version for a fix. Keep the app identifier (`com.catabase.app`) and credential service stable so installed versions continue using the existing data and credentials.

## Next distribution steps

This change establishes CI and the signed-release pipeline. It does not yet add a public installer or in-app version/update controls. Once the first signed release is available:

- Add the HTTPS installer that downloads/verifies the correct release and installs the app bundle.
- Integrate Tauri's updater with the public signing key and `https://github.com/smolijar/catabase/releases/latest/download/latest.json`.
- Route installation/restart through workspace persistence and query/transaction/staged-edit checks. The current native restart path bypasses the ordinary exit guard, so it must not be used directly by an updater.
- Test an actual old-version → new-version upgrade, including storage migration. Replacing the application does not replace its data directory, and binary downgrade alone is not a database rollback strategy.

Windows and Linux distribution are deferred; this pipeline targets macOS only.
