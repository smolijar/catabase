# Catabase

Catabase is a desktop PostgreSQL query workspace built with Rust, Tauri 2, Svelte 5, Vite, and Monaco.

## Prerequisites

- Rust toolchain with Cargo (CI uses 1.96.1)
- Node.js 24+
- pnpm 10+
- The pinned Tauri CLI is installed by `pnpm install`
- Tauri platform prerequisites for your OS

## Dependency Policy

Use pnpm only. Do not use npm for this project.

`pnpm-workspace.yaml` enforces `minimumReleaseAge: 10080`, so pnpm refuses package versions published less than one week ago.

## Setup

```sh
pnpm install
```

## Run Desktop App

```sh
pnpm tauri dev
```

Equivalent frontend-only dev server:

```sh
pnpm --dir ui dev
```

## Build

```sh
pnpm tauri build
```

## CI and distribution

[CI](https://github.com/smolijar/catabase/actions/workflows/ci.yml) checks pull requests and pushes to `main`. Successful main/manual runs also provide a universal macOS app and DMG for Apple Silicon and Intel in the run's **Artifacts** section. These previews are ad-hoc signed and are not notarized releases.

Version tags run the separate [release workflow](https://github.com/smolijar/catabase/actions/workflows/release.yml), which requires Apple signing/notarization credentials and a Tauri updater signing key before building a draft release. See [distribution setup](docs/distribution.md) for download instructions, required secrets, and the release process. The public one-line installer and in-app updater are not enabled yet.

## Verify

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir ui run check
node --experimental-strip-types --test ui/tests/*.test.ts
pnpm --dir ui run build
python3 scripts/check-release-version.py
python3 -B -m unittest discover -s scripts/tests
```

Opt-in Docker reflection test (uses only the saved `zenbase docker` connection; all write fixtures are temporary and rolled back):

```sh
CATABASE_TEST_CONNECTION_STORE="/path/to/catabase.sqlite3" cargo test -p catabase-core --test reflection_docker -- --ignored --nocapture
```

## Current Scope

Implemented:

- Tauri desktop shell
- Rust core crate with SQLite persistence
- connection CRUD, URL import/sync, password fallback/keyring attempt
- PostgreSQL query execution and schema introspection
- result metadata resolved from connection-scoped schema reflection, with batched `pg_catalog` fallback
- clickable query-duration status with preparation, reflection, execute/fetch, decode, connection/setup, history, IPC and DOM-update timings
- SQL splitting and simple auto-limit pagination
- execution-target resolution from Monaco selection/cursor/current statement
- transaction banner plus backend transaction connection routing
- server-side query cancellation via PostgreSQL cancel tokens
- loaded-row exports and explicitly re-run, read-only exports up to 10,000 rows
- atomic, parameterized cell edits with complete primary/unique-key validation and stale-cell detection
- schema-aware Monaco completions
- opt-in Supermaven-backed Monaco ghost-text completions using the official Neovim agent protocol
- Svelte/Monaco UI shell with sidebar, tabs, editor, results, history, settings
- independent query tabs with saved drafts, connection choice, and editor position
- virtualized result grid with column resizing, loaded-row sorting, range selection, and copying
- typed cell inspector distinguishing SQL NULL, empty text, and JSON values
- result provenance and read-only history snapshots; paging remains bound to the original SQL and connection
- opaque keyboard-accessible dropdowns and action menus
- pnpm install age gate

Query results and staged edits stay in memory only; restarting restores SQL drafts, not cached data. Sorting is limited to loaded rows. Loading more rows or requesting a larger export re-runs the original SQL in a read-only transaction, so it is unavailable while a manual transaction is open. Commit or roll back that transaction first. Cell edits made inside a manual transaction remain pending until you commit it.

### Supermaven autocomplete

Enable Supermaven in Settings to download and launch the official `sm-agent`. The download manifest and newline-delimited stdio protocol follow the [official Neovim plugin](https://github.com/supermaven-inc/supermaven-nvim). Catabase verifies the manifest's SHA-256 checksum and runs the agent in an isolated non-project directory. It supplies the active editable SQL draft (including comments and literals), its UTF-8 cursor offset and a bounded reference of the active connection's reflected table/column types, primary/unique keys and foreign keys. It does not add connection credentials, result rows or unopened tabs. The schema reference is commented in a neutral virtual `query.sql` document; it never changes the real draft or executed SQL. Referenced/prefix-matched tables are prioritized within the 96 KiB schema budget. The official agent uses its own existing Supermaven authentication; if activation is required, Settings offers a user-initiated sign-in link and free-tier action. Supermaven has [sunset standalone subscriptions but continues autocomplete for existing customers](https://supermaven.com/blog/sunsetting-supermaven); a working account/service is required.

Suggestions appear as Monaco ghost text; Tab accepts, Escape dismisses, and Option/Alt+\ requests one explicitly. Requests are debounced and cancelled on edits, cursor changes, tab/schema switches and disable. Stale results are rejected, UTF-16/UTF-8 offsets are translated, and cross-file jumps/destructive agent edits are not applied. A local grounding check rejects provably unknown new table/column references, including fabricated UUID/table names; it is not a full SQL validator and does not guess CTE/derived-table output types or function signatures. Disabling kills the process and clears pending suggestions. SQL drafts above 1 MiB are not submitted. Supermaven is off by default (including for existing OpenCode configurations), while local schema/foreign-key completion remains available independently. The old OpenCode HTTP endpoints and model selector have been removed.

Table/column-name completion stops at the reflected identifier rather than appending invented filters, dates or IDs. After accepting a name, type the next SQL operator/clause to continue; explicitly request another suggestion with Option/Alt+\ when needed.

After a service disconnect, the next edit or explicit suggestion request lets the official agent reconnect, as in Neovim; it does not continuously retry in the background. Settings distinguishes an agent startup failure from a service disconnect. Known WebSocket failures are classified into safe fixed messages without storing raw provider logs, account details or SQL. Retry restarts the agent; it cannot repair an unavailable upstream service.

An opt-in live smoke test sends only synthetic SQL, never database data:

```sh
CATABASE_SUPERMAVEN_LIVE_TEST=1 cargo test -p catabase-core supermaven::tests::official_agent_synthetic_completion -- --ignored --nocapture
```

Schema reflection includes table OIDs, column IDs/types and complete usable primary/unique keys. The backend reuses it to map result columns (including aliases and joins) without per-column catalog queries. Missing or expired metadata is refreshed for all affected tables in a batch; cache entries expire after 60 seconds to bound external DDL staleness. Explicit schema refresh and pool/configuration changes replace the cache. Non-read statements conservatively invalidate it. Manual transactions have separate caches; their schema reflection uses the held client, and finishing a transaction invalidates shared metadata. Applying edits still validates fresh catalog columns and keys, never trusting the cache for writes.

Click a successful query's status-bar message to inspect its timing breakdown (automatic paging reports the latest batch). The existing duration measures preparation through row decoding, not PostgreSQL EXPLAIN execution time. The dialog also shows connection/setup, history saving, estimated response/IPC overhead and result state/DOM update time, plus reflection cache-hit/refresh information. Server execution and network row transfer remain one combined phase; browser paint is not measured. Monotonic timestamps add no database requests, payload copies, or per-cell instrumentation.

History snapshots borrow query results and stop encoding at the 256 KiB limit. A conservative raw-size preflight skips already-oversized row payloads without scanning or escaping their strings. Oversized entries retain SQL, duration and row count, but omit the discarded snapshot and its unknown exact byte size. Small snapshots still round-trip normally.

History stores exact SQL and stable response payloads in separate SHA-256-addressed tables. Every execution remains in the timeline, with its own connection, timestamp, duration, errors and timing breakdown; repeated SQL and identical snapshots are stored once. Timing differences do not change the response hash, but row order, values, column metadata and pagination flags do. The same SQL returning changed data gets a new snapshot. Existing history migrates transactionally, preserving IDs and full snapshots. Deleting entries collects only payloads with no remaining references.

History entries use a compact one-line SQL preview with **Open** and **Delete** icon buttons. Clicking the SQL preview copies the exact SQL, including line breaks, to the clipboard. **Open** shows the SQL and saved result or error in a read-only tab, including entries inside transaction groups and global-search matches; SQL-only entries can also be opened without executing anything. The preview includes the connection, timestamp and duration. **Open in new query tab** creates an editable copy. Older failures can only show the error information originally saved.

Closing a tab does not ask for confirmation, whether using its close button, context menu or Cmd/Ctrl+W. Editable queries (including empty drafts) are saved under **Tabs → Closed** before being removed from the tab bar; a storage failure leaves the tab open and shows an error. The archive keeps the name, exact SQL, connection, editor position and close time across restarts, but not result rows or staged cell edits. Read-only history and schema references are not archived. The Tabs section lists open and closed tabs, and both its search and global search match names, full SQL and connection names. Selecting a closed tab reopens it without executing SQL. Closing a tab does not commit or roll back a transaction.

History's storage icon shows SQLite history table/index disk usage, unique SQL/snapshot sizes, execution-detail sizes and avoided duplicate bytes. Each entry shows a compact size label whose tooltip includes the size breakdown, shared-reference counts and query/response hashes. Per-entry figures are logical referenced content, not exclusive disk usage. Disk accounting runs only when requested, never on every query. Freed SQLite pages can be reused without shrinking the overall database file.

SELECT-like queries combine fresh preparation, result description and execution into one extended-protocol request, removing the separate prepare round trip without reusing potentially stale column origins after DDL or search-path changes. Result metadata is enriched after fetching. Zero-row results use a description-only fallback to retain column headers; SQL is never executed twice. Transaction commands, other SQL and compound batches retain their existing execution paths. The timing dialog explicitly labels combined preparation and the empty-result fallback.

Connections are single-line rows showing the bold name followed by the username in parentheses; the list scrolls independently beneath its header. Click a connection or press Enter to use and immediately test it. Double-click opens its editor after any first-click activation finishes; F2 edits without activation. Option/Alt-click, right-click or Shift+F10 opens connection actions without a visible menu button. Clicking the toolbar or status-bar connection name retries its test; only the toolbar arrow opens the searchable connection switcher. Both show Running/Cancelling during queries and disable re-testing, and otherwise share testing, green success, red error, and yellow stale-check states, without reverting a failed selection. Testing the already-selected connection preserves its results, staged edits and transaction.

The visible tab's connection is checked automatically when the app gains focus or while it remains in the foreground, only if its last check is at least one minute old (or it has never been checked). Queries refresh the same timestamp without an extra probe. Background/hidden windows never start automatic checks. Queries, cancellation, connection changes, schema loading, connection dialogs, cell editing, and active or unknown transaction status defer checks until the workspace is idle. Failed checks also wait a minute before retrying; manual retry remains available. An already-started check can finish after the window loses focus.

Disconnect is available in the connection switcher and the active connection's context menu. It closes the workspace pool without deleting the saved connection or SQL drafts; existing results remain available read-only. Staged edits require confirmation before being discarded, and an open transaction must be committed or rolled back first.

Query cancellation signals the owning operation without waiting on network I/O under the cancellation registry lock. The operation waits at most five seconds for both the query response and cancel sender to finish; an unresponsive connection is closed and removed from the pool, never recycled. Cancellation during connection setup also interrupts the wait. When cancellation cannot be confirmed, the error explicitly warns that the outcome of writes or a commit is unknown and must be checked before retrying.

### Workspace design

The [design reference](https://claude.ai/artifact/4bKN8Lr1j6Y2w7HECPaFVk#readme) is implemented in the existing Svelte/Monaco workspace, preserving query execution, editing safeguards, transaction controls, saved tabs, paging, and precise client-side query timings.

- Tuxedo, Snow and System themes are available in Settings. IBM Plex Sans, JetBrains Mono and the Silkscreen wordmark are bundled locally. The pixel-cat SVG also supplies native app icons and the browser favicon.
- On macOS, the workspace header shares a single row with the real native traffic-light controls instead of sitting below a separate title bar. Empty header space and the brand drag the window; double-click toggles zoom. Search and connection controls remain clickable. Other platforms retain their standard window decorations.
- Each connection can have a display accent and a short environment label under Edit connection → Appearance; the Connection tab holds its database fields. Appearance previews immediately in the workspace for the selected connection; closing without saving restores the previous appearance. Editing an inactive connection does not recolor the workspace. Appearance-only saves do not switch connections, reset sessions or discard results/staged edits. These preferences are stored on this device; they do not imply database permissions. Health colors remain independent of the display accent. Workspace Settings keeps the app-wide theme.
- Connection settings use a fixed-height dialog with an independently scrolling body and stationary tabs/actions. Editing or pasting a valid connection URL updates address/SSL fields immediately, preserving the name, options, and password. The URL contains only a read-only password mask; change credentials in the Password field. Pasted URL passwords are ignored. Incomplete URLs block Save/Test until corrected or replaced by edits to the individual connection fields.
- Edit connection → Options stores per-connection connection/pool-wait timeout (1–300 seconds), PostgreSQL statement and lock timeouts (0–86400 seconds; 0 preserves server defaults), and Read-only by default. Existing connections retain the previous 8-second connection timeout and server query/lock/access defaults. Options are copied when duplicating a connection, preserved when importing its URL, and applied as startup parameters without extra setup round trips. Recycled pool clients reset these defaults after SQL overrides. Read-only defaults make PostgreSQL reject writes and disable cell editing, but explicit SQL can override them: use database-role permissions for an enforced security boundary. Saving connection/options resets affected results and sessions, requires confirmation for staged edits, and refuses an open transaction.
- Selected navigation, connection rows and editor tabs share a flat accent tint, accent foreground and square corners. A 2px accent edge sits on the left for vertical navigation/lists and on top for horizontal editor tabs; hover preserves this state. Keyboard focus remains a separate outline.
- Cmd/Ctrl+K searches open and closed query tabs, the active connection's reflected tables/views, full stored SQL/error history, and workspace commands. History lookup is debounced and loads the 20 newest matching executions from the database (not only the sidebar's first page); refine the search to find older matches. Selecting history opens its read-only snapshot or SQL without executing it. Each group has a separate limit so a large schema cannot hide history or commands. Choosing a table opens a quoted SELECT draft without running it. The connection chip has its own searchable switcher.
- The tab strip keeps Run/Cancel, Find, Format SQL and Explain visible. Format SQL formats the selection or the entire editable draft as PostgreSQL, in one undoable edit. Unsupported syntax reports an error without modifying the draft.
- The result grid uses 28px rows, flat surfaces, inline compact type labels and PK/FK icons, while keeping detailed tooltips and the existing cell inspector/edit flow. The timing dialog adds proportional phase bars and Copy as text without reducing timing precision.

Deferred: Explain is intentionally disabled with a forthcoming-dialog tooltip. A custom integrated native title bar and server-version/access-role badges are not implemented; existing native chrome and real connection health remain available. The native title bar follows the selected theme. No server-version or read-only claims are inferred from an environment label.

Right-click a query tab or press Shift+F10 for Close, Duplicate and Rename. Double-click or F2 also renames; blur or Enter saves, Escape cancels. A duplicate opens next to its source with a unique `copy` suffix, the same SQL, connection and editor position, but no results or staged changes. Duplicating a read-only reference opens its SQL as an editable query.

Drag tab labels to reorder, or use Alt+Shift+Left/Right on a focused tab. The strip shows an insertion indicator and auto-scrolls at its edges; Escape cancels a drag. Order is saved with the workspace without switching the active query or losing results/staged edits.

Cmd/Ctrl+C on a focused tab copies a tab reference, and Cmd/Ctrl+V on a tab duplicates that open draft with a `copy` suffix. Clipboard references contain only the tab ID, not SQL or database data. Copy/paste in the SQL editor or rename input remains normal text editing.

SQL autocomplete resolves table aliases: `p.` offers only the source table's columns. In expressions such as `SELECT`, `WHERE`, `ON`, `GROUP BY` and `ORDER BY`, suggestions are limited to available table names/aliases and their columns. Aliases replace original table names, and ambiguous column names are offered with their table/alias prefix. `FROM` and `JOIN` still search the full schema. After a `JOIN table alias`, a space or newline offers complete `ON` conditions from declared foreign keys. Composite keys include every column pair, either join direction works, and multiple relationships remain separate choices. No relationship is guessed from matching column names; derived tables and ambiguous references do not get inferred predicates.

Edit cells requires a complete primary key or usable unique key from one source table in the selected columns. The disabled control explains what is missing. Nullable unique keys require non-null values in every loaded row unless the index uses NULLS NOT DISTINCT; partial, expression, invalid and deferred indexes are not row identities. Updates use the original key plus stale-value checks for changed cells, not every selected column. Expressions, generated columns and unsupported value types remain read-only.

The cell editor has a Set NULL shortcut next to Reset, without a separate checkbox. It changes the draft only; Stage change is still required, and non-nullable columns disable the shortcut. For non-FK cells, Edit value replaces a NULL draft with an editable value.

Foreign-key cells are picker-only: choose an existing row or use Set NULL when allowed, without free-form value input. Compact single-line choices show a row preview and muted key, with full details in the tooltip. Lookups use the result's saved connection, re-check the actual constraint, and run in an isolated, time-limited read-only transaction without replacing results or adding query history. The first 50 matches are shown; search keys or preview text to narrow the list. Composite picks fill/stage all projected components together; incomplete selections explain what is missing. Previews show committed rows, not uncommitted changes in a manual transaction.

Run (Cmd/Ctrl+Enter) executes selected SQL, or the statement under the cursor when nothing is selected. In addition to semicolons, the editor recognizes independent queries, INSERT/UPDATE/DELETE/MERGE statements, and transaction commands separated by a blank line. CTEs stay attached to their main query or write; insert sources, upserts, row locks, MERGE actions, nested queries and UNION/INTERSECT/EXCEPT operands stay together. Use semicolons for other statement boundaries; this editor convenience does not change explicitly selected SQL or PostgreSQL batch syntax.
