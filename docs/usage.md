# Using Catabase

Catabase is a desktop PostgreSQL workspace for writing SQL, inspecting results, reviewing cell edits and revisiting query history. For downloads and source builds, start with the [README](../README.md).

## Connections

Add a connection in the Connections sidebar. Click its row or press Enter to use it and immediately test it. Double-click opens its settings; F2 edits without activating it. Right-click, Option/Alt-click or Shift+F10 opens connection actions. Connections can be duplicated from this menu or by copying and pasting a focused connection row.

The connection name in the toolbar and status bar shares the same health status. Click either to test again; the toolbar arrow opens the searchable connection switcher. Checks show success, an error, or a stale state after one minute. While a query is running or cancelling, they show that activity and do not start another test. A failed test leaves the selected connection in place.

The active tab's connection is checked automatically when the app gains focus or remains in the foreground and its last check is stale. Background windows do not start checks. Queries, transactions, schema loading, connection dialogs and cell editing defer these checks until the workspace is idle. Running a query refreshes the health timestamp without an extra test.

Connection settings have three tabs:

| Tab | Controls |
| --- | --- |
| Connection | Name, host, port, database, username, password, SSL and connection URL. |
| Appearance | Connection accent and environment label, with an immediate preview for the selected connection. Closing without saving restores the previous appearance. |
| Options | Connection/pool-wait timeout, PostgreSQL statement and lock timeouts, and read-only defaults. |

Editing a valid connection URL updates its address and SSL fields as you type. The URL masks the password; edit credentials in the Password field. Pasted URL passwords are ignored. An incomplete URL blocks Save and Test until corrected or replaced by changes to the individual fields.

Connection/pool-wait timeout accepts 1–300 seconds. Statement and lock timeouts accept 0–86400 seconds; zero preserves the server defaults. Read-only defaults make PostgreSQL reject writes and disable cell editing, but explicit SQL can override those defaults. Use database-role permissions to enforce access restrictions. Connection colors and environment labels are visual preferences, not permission indicators.

Saving connection details or options resets affected sessions/results, asks before discarding staged edits, and refuses an open transaction. Appearance-only saves preserve results and sessions. The status-bar pool item opens workspace settings for the maximum connection count.

**Disconnect** closes the workspace pool without deleting the saved connection or SQL drafts. Existing results remain available read-only. Commit or roll back an open transaction first; staged edits require confirmation before being discarded.

## Write and run SQL

**Run** (`Cmd/Ctrl+Enter`) executes the selection. With no selection, it executes the statement under the cursor. Besides semicolons, the editor recognizes independent queries, writes and transaction commands separated by blank lines. CTEs remain attached to their main query or write; nested queries, insert sources, upserts, row locks, MERGE actions and set-operation operands stay together. Use semicolons for other boundaries. Explicit selections are sent as selected.

The editor includes Find/Replace, PostgreSQL formatting and font-size shortcuts. Find overlays the editor rather than pushing SQL down. Formatting operates on the selection or the full editable draft as one undoable change; unsupported syntax leaves the draft intact.

### Schema-aware completion

Completion uses reflected table names, columns, keys and relationships:

- `FROM` and `JOIN` search the full reflected schema.
- After an alias such as `p.`, only that source table's columns are offered.
- Expressions in `SELECT`, `WHERE`, `ON`, `GROUP BY` and `ORDER BY` use available tables/aliases and columns. Ambiguous names are qualified.
- After `JOIN table alias`, a space or newline offers complete `ON` clauses from declared foreign keys, including composite keys and either relationship direction.

Relationships are not inferred from similarly named columns. Derived tables and ambiguous references do not receive guessed join predicates. Refresh the schema when you need to see recent database changes.

### Transactions and cancellation

Manual `BEGIN`, `COMMIT`/`END` and `ROLLBACK` use the held transaction connection. The transaction banner shows elapsed time. Statements enter history as they finish, including transaction commands and updates applied through the cell-edit review. Related entries share a transaction ID and can be expanded as a group.

Cancel sends PostgreSQL's cancellation signal. An unresponsive connection is closed rather than returned to the pool. If the app cannot confirm cancellation, it reports an unknown write/commit outcome; check the database before retrying that operation.

## Results and cell editing

Results show their connection, execution time and executed SQL. The grid supports column resizing, cell/range selection, copying and sorting of loaded rows. Scrolling to the end requests the next batch when auto-limited results have more rows. Paging re-runs the original query in a read-only transaction and remains tied to its original connection and SQL.

Column headers show compact type information and primary/foreign-key indicators, with details in tooltips. The cell inspector distinguishes SQL NULL, empty text, JSON, arrays and scalar values. PostgreSQL array values use array literals so dimensions, lower bounds and NULL elements can be preserved.

Export offers CSV, TSV and JSON. You can export loaded rows or explicitly re-run a read-only export up to 10,000 rows. Paging and re-run exports are unavailable during a manual transaction; commit or roll back first.

### Review updates before applying

**Edit cells** requires a complete primary key or usable unique key for an unambiguous source table in the selected columns. A disabled button explains the missing requirement. Expressions, generated columns, unsupported types and read-only history results cannot be edited.

Stage values in the cell inspector, then review the generated SQL before applying. Updates identify rows by their original primary/unique key and check the original values of changed cells to detect stale data. The backend revalidates column/key metadata for writes. **Set NULL** changes the draft only; you still need to stage it, and non-nullable columns disable the shortcut.

Foreign-key cells use a searchable picker of existing rows rather than free-form input. Choices include a compact row preview and key; the first 50 matches are shown. Composite picks stage all projected components together. An incomplete composite selection explains which columns are missing. These previews show committed rows, not uncommitted changes in a manual transaction.

Applying changes is atomic. Inside a manual transaction, applied edits remain pending until you commit. Running a new query with staged edits asks before discarding them.

### Query timing

Click a successful query's status-bar duration to inspect the timing breakdown: SQL selection, connection/setup, preparation, metadata, execution/fetch, row decoding, history saving, response/IPC and UI update. Automatic paging reports the latest batch.

These are client-side wall-clock spans. The status-bar duration covers preparation through row decoding; it is not PostgreSQL's `EXPLAIN` execution time. Server execution and network transfer share one span, response/IPC is an estimated residual, and the UI span stops after DOM updates rather than browser paint.

The **Explain** button is currently a disabled placeholder; the query-plan dialog is not implemented.

## Tabs and search

Each editable tab saves its SQL, name, connection and editor position. Right-click or Shift+F10 offers Close, Duplicate and Rename. Double-click or F2 also renames; Enter or blur saves, and Escape cancels.

Drag labels to reorder tabs, or use `Alt+Shift+Left/Right` on a focused tab. Order is saved. Copy/paste on a focused tab duplicates that open draft with a `copy` suffix. Duplicates keep SQL, connection and editor position but start without results or staged edits. Duplicating a read-only reference creates an editable query.

Closing an editable tab saves it under **Tabs → Closed** without confirmation. If saving fails, the tab stays open. The archive contains the draft and editor position, not result rows or staged edits. Read-only history/schema tabs are not archived. Reopening a closed tab does not execute SQL, and closing a tab does not commit or roll back a transaction.

`Cmd/Ctrl+K` searches open/closed tabs, the active connection's reflected tables/views, stored SQL/error history and workspace commands. Choosing a table creates a quoted SELECT draft without executing it. Choosing history opens its read-only snapshot or SQL. History search returns the 20 newest matches; refine the query to find older entries.

## History and local data

History records individual completed statements and errors, including statements inside manual transactions and reviewed cell updates. Transaction groups can be expanded. **Open** displays the saved SQL and result/error without running it. Click the SQL preview to copy it; **Delete** removes the entry. **Open in new query tab** makes an editable copy.

Result snapshots are capped at 256 KiB. Larger entries retain their SQL, duration and row count but have no saved result snapshot. Exact repeated SQL and identical response snapshots share storage while each execution remains visible with its own timestamp and timing information.

History's storage control reports table/index disk usage, unique SQL/snapshot sizes and avoided duplicate bytes. Per-entry sizes describe referenced content, which may be shared with other entries. Deleting history can free reusable SQLite pages without shrinking the database file.

Live tab results and staged edits remain in memory; restarting restores SQL drafts, not those live result sets. History snapshots are stored separately. Local SQLite state can contain saved connections, SQL drafts, history snapshots and fallback passwords; OS keyring integration is also attempted.

## Appearance and shortcuts

Workspace settings offer **Tuxedo**, **Snow** and **System** themes. Connection accents and environment labels live in each connection's Appearance tab. The sidebar and result drawer can be resized.

On macOS, the workspace header shares a row with the native traffic-light controls. Drag empty header space or the brand to move the window; double-click to toggle zoom. Other platforms retain their normal window decorations.

| Shortcut | Action |
| --- | --- |
| `Cmd/Ctrl+Enter` | Run the selection or statement at the cursor. |
| `Cmd/Ctrl+K` | Search the workspace. |
| `Cmd/Ctrl+F` | Find in the SQL editor. |
| `Cmd/Ctrl+Alt+F` | Find and replace; `Ctrl+H` also works. |
| `Cmd/Ctrl++`, `Cmd/Ctrl+-`, `Cmd/Ctrl+0` | Increase, decrease or reset SQL editor font size. |
| `Cmd/Ctrl+T` | New query tab. |
| `Cmd/Ctrl+W` | Close the current tab. |
| `F2` | Rename a focused tab or edit a focused connection. |
| `Shift+F10` | Open the focused tab/connection context menu. |
| `Alt+Shift+Left/Right` | Reorder a focused tab. |
| `Cmd/Ctrl+C`, `Cmd/Ctrl+V` | Copy/paste a focused tab or connection to duplicate it. |

Editor zoom changes only the SQL font, from 8 to 48 px, and is saved locally. Option/Alt+Left/Right retains word navigation in the editor. Ordinary text copy/paste continues to work inside SQL and rename inputs.

## App updates

**Settings → App updates** shows the installed version, checks for a newer release and displays its release notes. Builds with an embedded updater signing key check automatically while the workspace is idle and in the foreground, at most once every four hours. Turn off **Check automatically** to use manual checks only. A failed check leaves the app usable and follows the same retry interval.

**Update & restart** downloads and verifies the signed package, installs it, then reopens Catabase. It first saves SQL tabs and editor positions. Finish running operations, commit or roll back transactions, close connection dialogs, and finish cell editing in every tab before updating. Apply or discard any staged edits first. Live result grids are cleared by the restart; saved history remains available. A save or download failure keeps the current workspace open.

Source builds and releases without an updater signing key show why in-app updates are unavailable, with a button to open GitHub Releases. Update signatures and Apple Developer ID/notarization are separate; see [distribution](distribution.md) for setup and the signing status of current releases.

## Optional Supermaven suggestions

Supermaven is off by default. Local schema completion works independently. Enabling Supermaven in Settings downloads and launches its official agent using the [Neovim plugin's protocol](https://github.com/supermaven-inc/supermaven-nvim). Suggestions depend on a working provider connection and account; enabling the integration does not guarantee service availability.

The integration supplies the active editable SQL draft, including comments and literals, its cursor position and a bounded reference of the active connection's reflected schema. It does not add connection credentials, result rows or unopened tabs. Schema context is limited to 96 KiB, prioritizing referenced/prefix-matched tables; drafts above 1 MiB are not submitted. The reference never changes the real SQL draft.

Ghost-text suggestions use Tab to accept, Escape to dismiss and Option/Alt+`\` to request explicitly. Requests are debounced and cancelled when their context changes. Local grounding rejects provably unknown table/column references, but is not a full SQL validator. Table/column completions stop at the identifier rather than appending filters or IDs.

Settings distinguishes agent startup problems from service disconnections. Retry restarts the agent. After a disconnect, editing or explicitly requesting a suggestion lets the agent reconnect; there is no continuous background retry. Disabling Supermaven stops the agent and cancels pending suggestions.

