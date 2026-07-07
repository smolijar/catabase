# Catabase GUI — Rust Implementation Plan

Implements [gui-app-spec.md](./gui-app-spec.md). Desktop PostgreSQL query workspace.

## 1. Architecture decision

**Shell: Tauri 2.x.** The spec mandates Monaco editor models ("Tabs and draft persistence … with Monaco models"), which requires a webview-based frontend — pure-Rust GUI toolkits (egui, iced, gpui, Slint) are ruled out because none can host Monaco. Tauri gives a Rust backend process, a system webview frontend, typed IPC commands (the spec's "backend endpoints"), an event system, and small binaries.

```
┌────────────────────────────── Tauri window ──────────────────────────────┐
│  Frontend (TypeScript + Svelte 5 + Monaco, built by Vite)                │
│  toolbar · sidebar (connections/schema/history/settings) · tabbed editor │
│  results panel · status bar · tx banner · modals                         │
└──────────────────────────────── invoke/events ───────────────────────────┘
┌──────────────────────────── src-tauri (app crate) ───────────────────────┐
│  IPC command handlers · AppState · Tauri plugins (clipboard, dialog)     │
└───────────────────────────────── depends on ─────────────────────────────┘
┌──────────────────────────── crates/catabase-core ────────────────────────┐
│  pg engine (pool, exec, cancel, tx, introspect, DDL)                     │
│  sql analysis (split, classify, auto-limit)                              │
│  storage (SQLite: settings, connections, drafts, history)                │
│  secrets (keyring) · conn URL parse/format · OpenCode client             │
└───────────────────────────────────────────────────────────────────────────┘
```

Core logic lives in `catabase-core` (no Tauri dependency) so it is unit-testable with plain `cargo test`.

## 2. Library choices

| Concern | Crate | Why |
|---|---|---|
| App shell | `tauri` 2.x + plugins `clipboard-manager`, `dialog`, `window-state` | Webview shell for Monaco; IPC; native clipboard/save-dialog for export |
| Async runtime | `tokio` | Tauri's runtime; required by tokio-postgres |
| PostgreSQL driver | `tokio-postgres` | Arbitrary runtime SQL, `Row::columns()` type introspection for rendering/edit-mode typing, `CancelToken` for query cancellation, full `DbError` fields (severity, detail, hint, code, position) that the spec requires in error display. `sqlx` rejected: optimized for compile-time-checked app queries, weaker fit for a dynamic query tool |
| Connection pool | `deadpool-postgres` | Configurable pool size (Settings requirement), pool status for status bar, ability to detach a dedicated connection for transactions |
| TLS | `tokio-postgres-rustls` + `rustls` + `rustls-native-certs` | SSL toggle per connection; no OpenSSL build headaches |
| SQL parsing | `pg_query` (libpg_query bindings) | Real PostgreSQL parser: statement splitting with byte offsets (execution targeting), statement classification (SELECT-like → auto-limit; BEGIN/COMMIT/ROLLBACK → transaction state), LIMIT detection via AST. Fallback if the C build becomes a problem: `sqlparser` + hand-rolled splitter (must handle dollar-quoting, comments, strings) |
| Local storage | `rusqlite` (`bundled`) + `rusqlite_migration` | Spec mandates SQLite; bundled avoids system dependency; migrations for the "Storage migration" scenario |
| Password storage | `keyring` 3.x | Platform mechanism per spec: macOS Keychain / Windows Credential Manager / Secret Service. Graceful fallback to an SQLite column when unavailable (spec: "when available") |
| URL handling | `url` + custom builder | Parse `postgres://`/`postgresql://` import (incl. `sslmode` query param); format fields → URL for the sync scenario |
| OpenCode client | `reqwest` (rustls) | HTTP client to local OpenCode server: health, provider/model list, completion |
| Serialization | `serde`, `serde_json` | IPC payloads; JSONB/row value transport; stored history results |
| Errors | `thiserror` in core, structured `AppError` serialized to frontend | PG errors must carry severity/detail/hint/code/position to the UI |
| IDs / time | `uuid`, `chrono` | Query tokens for cancel; history timestamps |
| Frontend | TypeScript, Vite, Svelte 5, `monaco-editor` | Svelte runes/stores fit the heavy state fan-out (status bar reacts to everything) with minimal bundle; Monaco per spec. Completion + inline-completion providers built on Monaco APIs |

## 3. Backend design (catabase-core)

### 3.1 Module layout

```
crates/catabase-core/src/
  lib.rs
  error.rs          # CoreError (thiserror); PgErrorInfo {message, severity, detail, hint, code, position}
  conn_url.rs       # parse postgres:// URL ↔ ConnectionFields; sslmode mapping
  secrets.rs        # keyring set/get/delete with SQLite fallback
  storage/
    mod.rs          # Storage handle: rusqlite on dedicated thread, channel-based API
    migrations.rs   # rusqlite_migration set; seeds default pool size
    settings.rs     # get/set pool_size (normalized 1..=32), ai_model
    connections.rs  # CRUD; active flag (single active enforced in one tx)
    drafts.rs       # script draft per tab-kind (playground)
    history.rs      # insert, page, search (LIKE), stats, delete, purge; capped result JSON
  sql/
    split.rs        # pg_query scanner → Vec<StatementRange {start, end, text}>; None → full buffer
    classify.rs     # StatementKind: SelectLike | TxBegin | TxCommit | TxRollback | Other
    auto_limit.rs   # AST has no LIMIT/FETCH → rewrite with LIMIT n+1 OFFSET m (deparse); parse failure → no auto-limit
  pg/
    pool.rs         # build deadpool from ConnectionFields + resolved password + pool size; status()
    execute.rs      # run query, map rows → JSON values + column meta {name, type_oid, type_name}
    cancel.rs       # registry: query_token → CancelToken; cancel issues server-side cancel
    transaction.rs  # TxManager: holds dedicated pooled connection while tx active; routes queries; commit/rollback/reset
    introspect.rs   # schemas, tables/views/matviews, columns (type, not-null, generated, pk) from pg_catalog
    ddl.rs          # CREATE TABLE composed from catalog; views via pg_get_viewdef; indexes via pg_get_indexdef
    stats.rs        # pg_stat_activity idle/active counts for current db + pool status
  opencode.rs       # health, models, complete(prefix, suffix, language, position, schema_context) → raw text
```

### 3.2 Query execution flow

1. Frontend resolves execution target (see 4.3), sends `execute_query { sql, query_token, offset, page_size }`.
2. Backend: if a transaction is active → route to the reserved tx connection; else check out from pool.
3. `sql/classify` on the (single) statement:
   - `TxBegin` → TxManager reserves a dedicated connection, executes BEGIN on it, emits tx-state event.
   - `TxCommit`/`TxRollback` → delegate to TxManager, release connection, emit event.
   - `SelectLike` without own LIMIT → auto-limit rewrite: request `page_size + 1` rows at `offset`; return `page_size`, set `has_more`, `auto_limited: true`.
   - Multi-statement or non-limitable → execute verbatim via `simple_query`/`query`.
4. Register `CancelToken` under `query_token` before awaiting; deregister after.
5. Convert rows to JSON (text/num/bool/json/timestamp/uuid/arrays → sensible JSON; unknown types → text via `::text` fallback cast on render error). Return `{ columns, rows, row_count, duration_ms, auto_limited, has_more }`.
6. Record history (both success and error paths): query, duration, row count, serialized result size, error flag/message, connection id, timestamp. Store result JSON capped (e.g. 256 KB) to power "Open history tab".

Cancellation: `cancel_query { query_token }` fires `CancelToken::cancel_query` on a fresh socket (best-effort server-side) and the frontend simultaneously drops its waiting state — matching the spec's "server query may still finish" wording even when the server cancel lands.

Pool reset triggers (spec): pool size change, connection edit, connection activate/delete → drop pool, reset TxManager (rollback if active), emit tx-reset event.

### 3.3 SQLite schema (v1 migration)

```sql
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- seeded: ('pool_size', '5')

CREATE TABLE connections (
  id TEXT PRIMARY KEY,            -- uuid
  name TEXT NOT NULL,
  host TEXT NOT NULL, port INTEGER NOT NULL DEFAULT 5432,
  database TEXT NOT NULL, username TEXT NOT NULL,
  ssl INTEGER NOT NULL DEFAULT 0,
  password_fallback TEXT,         -- only when keyring unavailable
  is_active INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);

CREATE TABLE script_drafts (
  tab_kind TEXT PRIMARY KEY,      -- 'playground'
  content TEXT NOT NULL, updated_at TEXT NOT NULL
);

CREATE TABLE query_history (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  connection_id TEXT,
  query TEXT NOT NULL,
  duration_ms INTEGER NOT NULL,
  row_count INTEGER,
  result_size INTEGER,
  is_error INTEGER NOT NULL DEFAULT 0,
  error_message TEXT,
  result_json TEXT,               -- capped; NULL when over cap or error
  executed_at TEXT NOT NULL
);
CREATE INDEX idx_history_executed ON query_history(executed_at DESC);
```

### 3.4 IPC command inventory

| Group | Commands |
|---|---|
| Connections | `list_connections`, `save_connection` (create/update; stores password via secrets), `delete_connection`, `activate_connection`, `test_connection`, `parse_connection_url`, `build_connection_url` |
| Settings | `get_settings`, `set_pool_size`, `set_ai_model` |
| Query | `execute_query`, `cancel_query`, `split_sql` (statement ranges for execution targeting), `connection_stats` |
| Transaction | `tx_status`, `tx_commit`, `tx_rollback` |
| Schema | `introspect_schema`, `get_ddl { schema, name }` |
| History | `history_page { search, offset, limit }`, `history_stats`, `history_get_result { id }`, `history_delete { id }`, `history_purge` |
| Drafts | `save_draft`, `load_draft` |
| AI | `ai_health`, `ai_models`, `ai_complete` |
| Export | `export_write_file { path, content }` (paired with dialog plugin save-as) |

Events (backend → frontend): `tx-changed { active, started_at }`, `pool-reset`.

`AppState` (managed by Tauri): `Storage` handle, `RwLock<Option<ActivePool>>`, `TxManager`, cancel registry (`DashMap<Uuid, CancelToken>`), OpenCode client.

## 4. Frontend design

### 4.1 Structure

```
ui/src/
  main.ts, App.svelte
  lib/ipc.ts                  # typed invoke wrappers mirroring command inventory
  stores/                     # Svelte runes-based state
    connections.ts, settings.ts, schema.ts, history.ts,
    query.ts (running token, results, pagination), tx.ts, status.ts (flash)
  components/
    Toolbar.svelte            # Run/Cancel, Clear
    Sidebar/{IconStrip, ConnectionsPanel, SchemaPanel, HistoryPanel, SettingsPanel}.svelte
    ConnectionModal.svelte    # create/edit, URL import/sync, Test
    EditorTabs.svelte         # tab strip; Playground non-closable
    SqlEditor.svelte          # Monaco host; one model per tab; layout() on resizes
    ResultsPanel.svelte       # table, status line, load-more, export menu, edit bar
    TxBanner.svelte, StatusBar.svelte, SplitPane.svelte
  editor/
    monaco.ts                 # setup, keybindings (Cmd/Ctrl+Enter, B, Shift+L)
    completions.ts            # schema-aware CompletionItemProvider
    ai-inline.ts              # InlineCompletionsProvider → ai_complete, idle-triggered
    exec-target.ts            # selection → join; else split_sql ranges → cursor-in/nearest; else full buffer
  results/
    export.ts                 # CSV/TSV/JSON serializers; clipboard (plugin); save dialog + export_write_file
    edit-mode.ts              # cell editing state machine, pending changes, UPDATE generation
```

### 4.2 Tabs and Monaco models

One `ITextModel` per tab keyed by tab id: `playground` (writable, sql), `ddl:<schema.table>` (read-only), `update-preview` (read-only), `history:<id>` (read-only). Tab close disposes the model; activation falls to the right neighbor, else Playground. Read-only enforced via `editor.updateOptions({ readOnly })` on tab switch.

Draft persistence: `onDidChangeModelContent` on playground → 1200 ms debounce → `save_draft`; Tauri `onCloseRequested` (webview `beforeunload` is unreliable in webviews) → synchronous final `save_draft`. On init, `load_draft` applies only when playground content is empty or equals the seeded default SQL.

### 4.3 Execution target resolution

1. Non-empty Monaco selections → trim each, join with blank line, add highlight decorations.
2. Else call `split_sql(buffer)`; map byte offsets → Monaco positions; cursor inside a range → that statement; between ranges → nearest previous, else next.
3. No ranges → full buffer.
4. Result trims to empty → render `No query to execute.` error, skip IPC.

Error position mapping: PG `position` is 1-based character offset into the *executed* text; map through the executed range's start offset into buffer coordinates and set a Monaco marker/decoration.

### 4.4 Inline edit mode

- Enabled only when a result table with detected single source table exists (detected from column metadata `table_oid` — tokio-postgres exposes it — resolved to `schema.table` via the schema cache; ambiguous/absent → Apply disabled, Preview still works with a placeholder table warning).
- Cell typing from column `type_name`: text input, numeric input with validation, boolean cycle (true → false → null-if-nullable → true). Generated columns (from introspection `attgenerated`) blocked with flash.
- Pending changes keyed by (row index, column). UPDATE generation: `UPDATE schema.table SET col = $lit WHERE pk1 = $v AND pk2 = $v;` using primary-key columns from schema cache; PK missing → fall back to all original-value columns in WHERE with a warning. Literals escaped conservatively (E'' strings, NULL, numeric passthrough after validation).
- Preview → read-only `update-preview` tab; Apply → execute statements sequentially through normal query path (honors open transaction); success clears pending; Discard restores originals; disabling with pending changes asks confirmation.

### 4.5 Polling and health loops

- Connection stats: 5 s interval while a connection is active; stopped on deactivate.
- OpenCode health: on start + 30 s interval; drives AI on/error/off status chip; on transition to connected, fetch models and fill Settings select.
- Schema auto-refresh on activation; manual Refresh button; on failure/empty retry ≤ 3 times with 3 s delay.

## 5. Implementation phases

Each phase is demoable and roughly ordered by dependency.

1. **Scaffold** — Workspace (`crates/catabase-core`, `src-tauri`, `ui`); Tauri 2 + Vite + Svelte + Monaco booting with static layout (toolbar, icon strip, panes, status bar, seeded editor); split-pane resizing with `editor.layout()`; keybindings wired. CI: fmt, clippy, test, `vite build`.
2. **Storage + settings** — rusqlite thread + migrations; settings get/set with pool-size normalization; Settings panel; status flash plumbing.
3. **Connections** — CRUD commands, keyring integration, URL parse/format + modal with import/sync/Test; activate/delete semantics (pool reset, active flag); status bar connection display.
4. **Query engine core** — pool build, `execute_query` happy path, results table render, duration/row-count status, error rendering with PG fields, history recording (write-only for now).
5. **Execution targeting + cancel** — `split_sql`, exec-target resolution, selection highlights, error-position mapping, cancel flow with token registry, Run↔Cancel button.
6. **Auto-limit pagination** — classify + AST rewrite, `has_more`/`auto_limited`, load-more via scroll/button, `+` count marker, load-more failure handling.
7. **Schema browser** — introspection, tree render with pk/generated/type/nullable markers, auto-refresh + retry, DDL generation and read-only DDL tabs; schema-aware Monaco completions.
8. **History UI** — panel with paging, 300 ms debounced search, stats, restore, open-as-tab (stored results), delete/purge.
9. **Transactions** — TxManager, BEGIN/COMMIT/ROLLBACK detection, banner with timer, Commit/Rollback buttons, reset-on-activation semantics.
10. **Export** — CSV/TSV/JSON serializers, clipboard copy, save-dialog download with timestamped filenames, full export via re-execution with large page size, no-data guards.
11. **Inline editing** — edit mode, cell editors, pending-change tracking, UPDATE generation, preview tab, apply, discard, disable-confirmation.
12. **AI** — OpenCode client (health/models/complete), model setting validation, inline completion provider with idle trigger, AI status chip.
13. **Drafts + polish** — draft autosave/load/close-save, tab lifecycle edge cases, `Cmd/Ctrl+Shift+L` clear-results, Clear button, empty states, packaging (`tauri bundler`; macOS signing/notarization if distributing).

## 6. Testing strategy

- **Core unit tests** (pure Rust, fast): `conn_url` round-trips incl. sslmode and percent-encoding; `sql/split` against comments, dollar-quotes, strings; `classify` for tx and select-like; `auto_limit` rewrites incl. CTEs, existing LIMIT, parse failures; history capping; UPDATE-literal escaping (if generation moves to core).
- **Postgres integration tests**: `testcontainers` (Postgres container), feature-gated `--features it`; cover execute/cancel/transaction routing/introspection/DDL against a fixture schema.
- **Frontend unit tests**: Vitest for export serializers, exec-target resolution (mock `split_sql`), edit-mode state machine.
- **Smoke e2e** (optional, later): `tauri-driver` + WebdriverIO covering launch → add connection → run query → see rows.

## 7. Risks and mitigations

| Risk | Mitigation |
|---|---|
| `pg_query` C build (long compile, platform quirks) | Isolated behind `sql/` module trait; fallback path with `sqlparser` + custom splitter kept feasible |
| Auto-limit rewrite alters semantics (e.g. locking clauses, `FOR UPDATE`) | Classifier excludes statements with locking clauses/INTO; any parse/deparse doubt → execute verbatim, no auto-limit |
| keyring unavailable (headless Linux, locked keychain) | SQLite fallback column; spec explicitly allows "when available" |
| Source-table detection for inline edit unreliable on joins/expressions | Use `table_oid` column metadata; require single table + PK for Apply, otherwise disable with explanation |
| Webview differences (WKWebView vs WebView2) for Monaco, clipboard, beforeunload | Use Tauri plugins for clipboard/dialog instead of web APIs; `onCloseRequested` instead of `beforeunload`; test both platforms early (phase 1) |
| Large result sets freeze the DOM table | Auto-limit default page (200–500 rows); virtualized table rows if profiling shows need |
| OpenCode API surface changes | Thin client module, base URL configurable, health check degrades to "AI off" |

## 8. Repository layout

```
catabase/
  Cargo.toml                # workspace: crates/catabase-core, src-tauri
  crates/catabase-core/
  src-tauri/                # tauri app crate, tauri.conf.json, capabilities
  ui/                       # vite + svelte + monaco (frontendDist → src-tauri config)
  docs/
    gui-app-spec.md
    rust-implementation-plan.md
```
