# Development

Catabase uses Rust, Tauri 2, Svelte 5, Vite and Monaco. The [usage guide](usage.md) describes app behavior; [distribution](distribution.md) covers preview packages, signing and releases.

## Requirements and setup

- Rust with Cargo; CI uses Rust 1.96.1.
- Node.js 24 or newer.
- pnpm; the root `package.json` pins pnpm 10.33.0.
- Python 3.11 or newer for release-version checks.
- The [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform. On macOS, this includes Xcode Command Line Tools.

Use pnpm for JavaScript dependencies. The workspace's `minimumReleaseAge: 10080` policy excludes versions published less than one week ago. Keep it enabled. The Tauri CLI is pinned in the root workspace dependencies and installed with the rest of the packages.

```sh
git clone https://github.com/smolijar/catabase.git
cd catabase
pnpm install --frozen-lockfile
pnpm tauri dev
```

To run just the frontend development server:

```sh
pnpm --dir ui dev
```

The normal app requires the native Tauri bridge for database and persistence commands. Browser fixtures under `ui/tests/fixtures/` provide isolated mock-bridge scenarios for UI work without using saved database connections.

## Documentation screenshots

Start `pnpm --dir ui dev`, then open `http://127.0.0.1:1420/tests/fixtures/showcase.html`. This fixture renders the real workspace with fictional bookshop data through a mock Tauri bridge. It does not connect to a database, native storage or an autocomplete service.

Run the seeded Book catalog query, open Schema and expand `books`. The title and date columns can be widened with the grid's resize controls. The fixture supports this screenshot, not arbitrary SQL execution; applying updates is intentionally unsupported.

Save the capture to `docs/images/workspace.jpg`. Keep the fictional-data caption in the README, and keep private queries, connection details and real database rows out of documentation images.

## Build

Run from the repository root:

```sh
pnpm tauri build
```

The frontend is built automatically by Tauri's build hook. On macOS, bundles are written beneath `target/release/bundle/`.

To build for both Apple Silicon and Intel:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm tauri build --target universal-apple-darwin --bundles app,dmg -- --locked
```

Universal output is under `target/universal-apple-darwin/release/bundle/`. It compiles both architectures sequentially; a first release build can take substantially longer than the checks. Developer ID signing, notarization and update signing are configured separately in the [release pipeline](distribution.md).

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
pnpm --dir ui run check
node --experimental-strip-types --test ui/tests/*.test.ts
pnpm --dir ui run build
python3 scripts/check-release-version.py
python3 -B -m unittest discover -s scripts/tests
```

These match the automated checks. The standard Cargo suite does not require a PostgreSQL database or Supermaven service; live integration tests are ignored by default.

### Opt-in integration tests

The Docker reflection suite uses the saved connection named **zenbase docker**. Supply a Catabase SQLite connection store containing that connection. Temporary write fixtures are rolled back:

```sh
CATABASE_TEST_CONNECTION_STORE="/path/to/catabase.sqlite3" \
  cargo test -p catabase-core --test reflection_docker -- --ignored --nocapture
```

A separate provider smoke test sends synthetic SQL to the official Supermaven agent:

```sh
CATABASE_SUPERMAVEN_LIVE_TEST=1 \
  cargo test -p catabase-core supermaven::tests::official_agent_synthetic_completion -- --ignored --nocapture
```

Do not enable these tests in ordinary CI or point them at production connections. Tests against copied user history likewise require their own explicit opt-in and a disposable copy.

## Code map

| Location | Responsibility |
| --- | --- |
| `crates/catabase-core/src/` | PostgreSQL execution, reflection, cancellation, row editing, local storage and Supermaven integration. |
| `src-tauri/src/` | Native app lifecycle, Tauri commands and shared application state. |
| `ui/src/App.svelte` | Workspace state and orchestration. |
| `ui/src/components/` | Editor, grid, dialogs, navigation and other UI components. |
| `ui/src/lib/` | Frontend models, API bridge and testable behavior. |
| `ui/tests/` | Frontend unit tests and browser fixtures. |
| `scripts/` | Release-version validation and its tests. |
| `.github/workflows/` | CI, universal macOS preview builds and signed draft releases. |

## Execution and metadata

SELECT-like queries combine fresh preparation, result description and execution in one extended-protocol request. Empty results use a description-only fallback to retain column headers; SQL is not executed twice. Other statements, transaction commands and compound batches retain their own execution paths.

Schema reflection includes table OIDs, column identifiers/types, complete usable primary/unique keys and foreign keys. Result columns, including aliases and joins, are mapped through this connection-scoped metadata. Missing or expired metadata is refreshed in batches rather than with per-column catalog queries. Cache entries expire after 60 seconds; explicit schema refresh, pool/configuration changes and non-read statements invalidate relevant metadata. Manual transactions have separate caches tied to their held client. Applying edits validates fresh catalog metadata instead of relying on cached keys.

Usable row identities exclude partial, expression, invalid and deferred indexes. A nullable unique key requires non-null key values in every loaded row unless it uses `NULLS NOT DISTINCT`. Row updates are parameterized and use the original key plus stale-value checks for changed cells. Foreign-key picker lookups revalidate constraints and run isolated, time-limited read-only transactions without changing current results or adding query history.

Cancellation does not hold the cancellation registry lock during network I/O. Query response and cancel delivery are given a bounded five-second completion wait; an unresponsive connection is closed, not recycled. The UI preserves unknown-outcome errors rather than implying that a write or commit was rolled back.

## History and persistence

Native state lives in `app_data_dir()/catabase.sqlite3`. Connections, settings, drafts and history use SQLite; live tab result sets and staged cell changes are not restored after a restart. Historical snapshots are a separate persisted representation. Keep the app identifier and credential-service name stable when changing packaging or releases.

History stores SQL and stable response payloads in separate SHA-256-addressed tables. Execution records retain their own timestamp, connection, errors and timings, plus transaction membership where applicable. Different timings do not change the response hash; row order, values, metadata and pagination flags do. Deleting entries collects only payloads that have no remaining references.

Snapshot serialization borrows results and stops at the 256 KiB cap. A raw-size preflight rejects clearly oversized row payloads before escaping or copying them. Such executions keep SQL/counts/timings but no result snapshot or invented exact snapshot size. History storage accounting is on demand; it is not part of each query's hot path.

Each completed statement is recorded before its result returns to the UI, rather than waiting for transaction commit. Reviewed cell updates use the same history mechanisms. Unfinished transaction history is marked interrupted when the app starts again.

The workspace close path waits for pending tab archiving, finalizes rename state and flushes saved drafts. The updater freezes workspace controls and flushes this same state before invoking its guarded native install command. A native operation gate rejects concurrent database/storage work, checks transaction state, and blocks exit during replacement. Do not add an unguarded restart path; native restart bypasses the ordinary exit handshake.

## Timing instrumentation

Timing spans use monotonic clocks and add no database requests, result copies or per-cell instrumentation. The displayed query duration covers preparation through row decoding. The detailed dialog also includes connection setup, history saving, an estimated response/IPC residual, and frontend state/DOM updates. Server execution and network transfer cannot be separated by client timestamps; browser paint is outside the measured UI span.

## Supermaven integration

The optional integration verifies the official agent download's SHA-256 checksum and launches it in an isolated non-project directory. It follows the [official Neovim agent protocol](https://github.com/supermaven-inc/supermaven-nvim) over newline-delimited stdio. The bounded virtual SQL document includes the active draft and commented schema context. UTF-16 editor positions are translated to UTF-8 byte offsets for the agent.

Stale responses, cross-file edits and destructive changes are rejected. A local grounding check can reject known-invalid table/column names but does not establish SQL validity or infer arbitrary CTE/derived-table shapes. Disabling the feature stops the process and cancels pending work. Known provider transport errors are mapped to fixed messages without retaining raw provider logs, account details or SQL.

## Release automation

CI runs on pull requests and `main`; successful main/manual runs additionally upload ad-hoc-signed universal macOS previews. Version tags use a separate signing preflight and draft-release workflow. Versions must agree across the Cargo workspace, app lockfile entries, UI package and Tauri configuration. See [distribution.md](distribution.md) for required credentials, the release checklist and the remaining installer/updater work.
