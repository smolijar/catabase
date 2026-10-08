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

## Current Scope

Implemented:

- Tauri desktop shell
- Rust core crate with SQLite persistence
- connection CRUD, URL import/sync, password fallback/keyring attempt
- PostgreSQL query execution and schema introspection
- result column metadata enrichment from `pg_catalog`
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

Connections are single-line rows showing the bold name followed by the username in parentheses; the list scrolls independently beneath its header. Option/Alt-click, right-click or Shift+F10 opens connection actions without a visible menu button. Double-click a connection or press Enter to select and immediately test it. The toolbar pill shows testing progress, turns green after success, or turns red with the error on failure without reverting the selection. Click the pill to retry. Testing the already-selected connection preserves its results, staged edits and transaction.

SQL autocomplete resolves table aliases: `p.` offers only the source table's columns. In expressions such as `SELECT`, `WHERE`, `ON`, `GROUP BY` and `ORDER BY`, suggestions are limited to available table names/aliases and their columns. Aliases replace original table names, and ambiguous column names are offered with their table/alias prefix. `FROM` and `JOIN` still search the full schema. After a `JOIN table alias`, a space or newline offers complete `ON` conditions from declared foreign keys. Composite keys include every column pair, either join direction works, and multiple relationships remain separate choices. No relationship is guessed from matching column names; derived tables and ambiguous references do not get inferred predicates.

Edit cells requires a complete primary key or usable unique key from one source table in the selected columns. The disabled control explains what is missing. Nullable unique keys require non-null values in every loaded row unless the index uses NULLS NOT DISTINCT; partial, expression, invalid and deferred indexes are not row identities. Updates use the original key plus stale-value checks for changed cells, not every selected column. Expressions, generated columns and unsupported value types remain read-only.

Run (Cmd/Ctrl+Enter) executes selected SQL, or the statement under the cursor when nothing is selected. In addition to semicolons, the editor recognizes independent SELECTs separated by a blank line. CTEs stay attached to their main query, and nested queries and UNION/INTERSECT/EXCEPT operands stay together. Use semicolons for other statement boundaries; this editor convenience does not change PostgreSQL batch syntax.
