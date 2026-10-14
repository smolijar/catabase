# Catabase

Catabase is a desktop PostgreSQL query workspace built with Rust, Tauri 2, Svelte 5, Vite, and Monaco.

## Prerequisites

- Rust toolchain with Cargo
- Node.js 24+
- pnpm 10+
- Tauri CLI: `cargo install tauri-cli --version '^2'`
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
cargo tauri dev
```

Equivalent frontend-only dev server:

```sh
pnpm --dir ui dev
```

## Build

```sh
pnpm --dir ui build
cargo tauri build
```

## Verify

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir ui run check
node --experimental-strip-types --test ui/tests/*.test.ts
pnpm --dir ui run build
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
- OpenCode-backed Monaco inline AI completions
- Svelte/Monaco UI shell with sidebar, tabs, editor, results, history, settings
- independent query tabs with saved drafts, connection choice, and editor position
- virtualized result grid with column resizing, loaded-row sorting, range selection, and copying
- typed cell inspector distinguishing SQL NULL, empty text, and JSON values
- result provenance and read-only history snapshots; paging remains bound to the original SQL and connection
- opaque keyboard-accessible dropdowns and action menus
- pnpm install age gate

Query results and staged edits stay in memory only; restarting restores SQL drafts, not cached data. Sorting is limited to loaded rows. Loading more rows or requesting a larger export re-runs the original SQL in a read-only transaction, so it is unavailable while a manual transaction is open. Commit or roll back that transaction first. Cell edits made inside a manual transaction remain pending until you commit it.

Schema reflection includes table OIDs, column IDs/types and complete usable primary/unique keys. The backend reuses it to map result columns (including aliases and joins) without per-column catalog queries. Missing or expired metadata is refreshed for all affected tables in a batch; cache entries expire after 60 seconds to bound external DDL staleness. Explicit schema refresh and pool/configuration changes replace the cache. Non-read statements conservatively invalidate it. Manual transactions have separate caches; their schema reflection uses the held client, and finishing a transaction invalidates shared metadata. Applying edits still validates fresh catalog columns and keys, never trusting the cache for writes.

Click a successful query's status-bar message to inspect its timing breakdown (automatic paging reports the latest batch). The existing duration measures preparation through row decoding, not PostgreSQL EXPLAIN execution time. The dialog also shows connection/setup, history saving, estimated response/IPC overhead and result state/DOM update time, plus reflection cache-hit/refresh information. Server execution and network row transfer remain one combined phase; browser paint is not measured. Monotonic timestamps add no database requests, payload copies, or per-cell instrumentation.

History snapshots borrow query results and stop encoding at the 256 KiB limit. A conservative raw-size preflight skips already-oversized row payloads without scanning or escaping their strings. Oversized entries retain SQL, duration and row count, but omit the discarded snapshot and its unknown exact byte size. Small snapshots still round-trip normally and retain their exact encoded size.

SELECT-like queries combine fresh preparation, result description and execution into one extended-protocol request, removing the separate prepare round trip without reusing potentially stale column origins after DDL or search-path changes. Result metadata is enriched after fetching. Zero-row results use a description-only fallback to retain column headers; SQL is never executed twice. Transaction commands, other SQL and compound batches retain their existing execution paths. The timing dialog explicitly labels combined preparation and the empty-result fallback.

Connections are single-line rows showing the bold name followed by the username in parentheses; the list scrolls independently beneath its header. Option/Alt-click, right-click or Shift+F10 opens connection actions without a visible menu button. Double-click a connection or press Enter to select and immediately test it. The toolbar pill shows testing progress, turns green after success, or turns red with the error on failure without reverting the selection. Click the pill to retry. Testing the already-selected connection preserves its results, staged edits and transaction.

Right-click a query tab or press Shift+F10 for Close, Duplicate and Rename. Double-click or F2 also renames; blur or Enter saves, Escape cancels. A duplicate opens next to its source with a unique `copy` suffix, the same SQL, connection and editor position, but no results or staged changes. Duplicating a read-only reference opens its SQL as an editable query.

Drag tab labels to reorder, or use Alt+Shift+Left/Right on a focused tab. The strip shows an insertion indicator and auto-scrolls at its edges; Escape cancels a drag. Order is saved with the workspace without switching the active query or losing results/staged edits.

Cmd/Ctrl+C on a focused tab copies a tab reference, and Cmd/Ctrl+V on a tab duplicates that open draft with a `copy` suffix. Clipboard references contain only the tab ID, not SQL or database data. Copy/paste in the SQL editor or rename input remains normal text editing.

SQL autocomplete resolves table aliases: `p.` offers only the source table's columns. In expressions such as `SELECT`, `WHERE`, `ON`, `GROUP BY` and `ORDER BY`, suggestions are limited to available table names/aliases and their columns. Aliases replace original table names, and ambiguous column names are offered with their table/alias prefix. `FROM` and `JOIN` still search the full schema. After a `JOIN table alias`, a space or newline offers complete `ON` conditions from declared foreign keys. Composite keys include every column pair, either join direction works, and multiple relationships remain separate choices. No relationship is guessed from matching column names; derived tables and ambiguous references do not get inferred predicates.

Edit cells requires a complete primary key or usable unique key from one source table in the selected columns. The disabled control explains what is missing. Nullable unique keys require non-null values in every loaded row unless the index uses NULLS NOT DISTINCT; partial, expression, invalid and deferred indexes are not row identities. Updates use the original key plus stale-value checks for changed cells, not every selected column. Expressions, generated columns and unsupported value types remain read-only.

The cell editor has a Set NULL shortcut next to Reset, without a separate checkbox. It changes the draft only; Stage change is still required, and non-nullable columns disable the shortcut. For non-FK cells, Edit value replaces a NULL draft with an editable value.

Foreign-key cells are picker-only: choose an existing row or use Set NULL when allowed, without free-form value input. Compact single-line choices show a row preview and muted key, with full details in the tooltip. Lookups use the result's saved connection, re-check the actual constraint, and run in an isolated, time-limited read-only transaction without replacing results or adding query history. The first 50 matches are shown; search keys or preview text to narrow the list. Composite picks fill/stage all projected components together; incomplete selections explain what is missing. Previews show committed rows, not uncommitted changes in a manual transaction.

Run (Cmd/Ctrl+Enter) executes selected SQL, or the statement under the cursor when nothing is selected. In addition to semicolons, the editor recognizes independent SELECTs separated by a blank line. CTEs stay attached to their main query, and nested queries and UNION/INTERSECT/EXCEPT operands stay together. Use semicolons for other statement boundaries; this editor convenience does not change PostgreSQL batch syntax.
