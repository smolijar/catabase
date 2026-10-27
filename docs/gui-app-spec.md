# gui-app Specification

## Purpose
Catabase GUI provides PostgreSQL query workspace: connection management, schema exploration, SQL editing, result viewing, history, export, inline edits, transactions, and AI-assisted SQL completion.

## Requirements

### Requirement: Application shell and navigation
GUI MUST present persistent toolbar, sidebar icon strip, sidebar content, tabbed SQL editor, results panel, and status bar.

#### Scenario: Initial launch
- **WHEN** user opens GUI
- **THEN** app displays Catabase toolbar, Run and Clear controls, sidebar with Connections selected, Playground tab, SQL editor seeded with default SQL, empty results panel, and status bar.

#### Scenario: Sidebar tab switch
- **WHEN** user selects Connections, Schema, History, or Settings icon
- **THEN** matching sidebar panel becomes visible
- **AND** selected icon is marked active
- **AND** editor layout recalculates.

#### Scenario: Collapse active sidebar
- **WHEN** user selects already-active sidebar icon or presses `Cmd/Ctrl+B`
- **THEN** sidebar content collapses or expands without destroying active tab state.

### Requirement: Connection management
GUI MUST let user create, edit, test, activate, and delete PostgreSQL connections.

#### Scenario: Open new connection modal
- **WHEN** user chooses add connection
- **THEN** modal opens in create mode
- **AND** name, host, port, database, username, password, SSL, and connection URL inputs are available.

#### Scenario: Import connection URL
- **WHEN** user enters parseable `postgres://` or `postgresql://` URL
- **THEN** host, port, database, username, password, and SSL fields populate from URL
- **AND** feedback reports URL imported.

#### Scenario: Sync fields to connection URL
- **WHEN** user changes host, port, database, username, password, or SSL
- **THEN** connection URL field updates to equivalent PostgreSQL URL when required fields exist.

#### Scenario: Test connection from modal
- **WHEN** user clicks Test with connection fields
- **THEN** GUI disables Test and shows testing state
- **AND** displays success or error feedback from backend connection test.

#### Scenario: Save new connection
- **WHEN** user submits valid new connection
- **THEN** backend stores connection
- **AND** new connection becomes active
- **AND** modal closes
- **AND** sidebar and status bar refresh.

#### Scenario: Edit existing connection
- **WHEN** user clicks Edit on connection
- **THEN** modal opens in edit mode with saved values including resolved password
- **AND** successful submit updates connection, resets SQL client, closes modal, and refreshes app state.

#### Scenario: Activate connection
- **WHEN** user clicks Use on connection
- **THEN** selected connection becomes only active connection
- **AND** SQL client and transaction state reset
- **AND** schema refresh and connection stats polling start.

#### Scenario: Delete connection
- **WHEN** user confirms deletion
- **THEN** connection is removed
- **AND** saved password is removed
- **AND** active SQL client closes if deleted connection was active.

### Requirement: Settings and status
GUI MUST expose pool size and AI model settings, plus live status.

#### Scenario: Save pool size
- **WHEN** user submits pool size in Settings
- **THEN** backend normalizes and persists pool size
- **AND** active SQL client resets
- **AND** status flash confirms saved size.

#### Scenario: Select AI model
- **WHEN** user selects AI model
- **THEN** model value is validated and persisted
- **AND** status flash shows selected model or default.

#### Scenario: Status bar render
- **WHEN** app state changes
- **THEN** status bar shows active connection or no connection, pool stats or pool size, schema state, AI state, and transient flash message.

#### Scenario: Connection stats polling
- **WHEN** active connection exists
- **THEN** GUI polls connection stats every 5 seconds
- **AND** status bar shows idle and active counts when available.

### Requirement: SQL editor execution target
GUI MUST choose query to execute from editor selection, current statement, nearest statement, or full buffer.

#### Scenario: Non-empty selection
- **WHEN** user runs query with one or more non-empty selections
- **THEN** selected SQL texts are trimmed and joined with blank line
- **AND** selected ranges are highlighted.

#### Scenario: Cursor in statement
- **WHEN** user runs query without selection and cursor is inside SQL statement
- **THEN** only current statement executes.

#### Scenario: Cursor between statements
- **WHEN** user runs query without selection and cursor is between statements
- **THEN** nearest previous or next statement executes.

#### Scenario: No parsed statements
- **WHEN** SQL splitter finds no statement chunks
- **THEN** full buffer is execution target.

### Requirement: Query execution lifecycle
GUI MUST run SQL against active PostgreSQL connection and render progress, success, cancellation, and error states.

#### Scenario: Run from Playground
- **WHEN** user clicks Run or presses `Cmd/Ctrl+Enter` from Playground
- **THEN** GUI sends execution target to backend
- **AND** Run button changes to Cancel
- **AND** results status updates running duration.

#### Scenario: Run outside Playground
- **WHEN** user tries to run query from non-Playground tab
- **THEN** GUI does not execute query
- **AND** status flash asks user to switch to Playground.

#### Scenario: Empty query
- **WHEN** execution target trims to empty string
- **THEN** GUI displays `No query to execute.` error.

#### Scenario: Cancel running query
- **WHEN** user clicks Cancel while query token is active
- **THEN** GUI stops local waiting state
- **AND** clears highlights
- **AND** shows Cancelled status
- **AND** warns server query may still finish in background.

#### Scenario: Successful query
- **WHEN** backend returns success
- **THEN** GUI displays row count, duration, and auto-limited marker if applicable
- **AND** renders results table when rows exist
- **AND** shows no-row success message when no rows exist
- **AND** records last query context for export and inline edit.

#### Scenario: Query error
- **WHEN** backend returns error
- **THEN** GUI displays message plus severity, detail, hint, code, and position when present
- **AND** marks editor error position when position can be mapped to executed range.

### Requirement: Results pagination
GUI MUST auto-page limitable SELECT-like queries and allow loading more rows.

#### Scenario: Auto-limited result has more rows
- **WHEN** backend returns first page with `autoLimited=true` and `hasMore=true`
- **THEN** GUI shows loaded rows, marks count with `+`, and displays load-more affordance.

#### Scenario: Load more rows by scroll or button
- **WHEN** user scrolls near bottom or clicks Load more rows
- **THEN** GUI requests same query with next offset and page size
- **AND** appends rows to existing table
- **AND** updates loaded count and footer.

#### Scenario: Load-more failure
- **WHEN** backend returns error while loading next page
- **THEN** GUI stops pagination and flashes failure message.

### Requirement: Transaction controls
GUI MUST reflect and control active SQL transactions using dedicated backend transaction state.

#### Scenario: Begin transaction through query
- **WHEN** user executes transaction start command
- **THEN** backend reserves transaction connection
- **AND** GUI shows transaction banner with timer.

#### Scenario: Commit transaction
- **WHEN** user clicks Commit or executes commit command
- **THEN** backend commits active transaction
- **AND** GUI hides transaction banner and flashes committed message.

#### Scenario: Rollback transaction
- **WHEN** user clicks Rollback or executes rollback command
- **THEN** backend rolls back active transaction
- **AND** GUI hides transaction banner and flashes rolled-back message.

#### Scenario: Active connection changes
- **WHEN** active connection changes or client resets
- **THEN** transaction state resets and banner hides.

### Requirement: Schema browser
GUI MUST introspect active PostgreSQL schema and render schema tree with DDL navigation.

#### Scenario: Active connection selected
- **WHEN** active connection becomes available
- **THEN** GUI refreshes schema automatically.

#### Scenario: Manual schema refresh
- **WHEN** user clicks Refresh in Schema panel
- **THEN** GUI requests schema introspection
- **AND** shows loading, success, or error state.

#### Scenario: Retry schema refresh
- **WHEN** schema refresh fails or returns empty and retry limit not reached
- **THEN** GUI retries up to 3 times after 3 seconds.

#### Scenario: Render schema tree
- **WHEN** schema data exists
- **THEN** GUI displays schemas, tables/views, columns, primary-key marker, generated marker, data type, and nullable marker.

#### Scenario: Open DDL tab
- **WHEN** user clicks table or view summary in schema tree
- **THEN** GUI requests DDL
- **AND** opens or updates read-only DDL tab for `schema.table`.

### Requirement: Query history
GUI MUST persist query history and let user search, restore, open, delete, and purge entries.

#### Scenario: Query execution recorded
- **WHEN** query completes or errors
- **THEN** backend records query, duration, row count, result size, error flag, error message, connection id, and timestamp.

#### Scenario: Open History panel
- **WHEN** user opens History sidebar
- **THEN** GUI loads recent history page and stats.

#### Scenario: Search history
- **WHEN** user types in history search
- **THEN** GUI debounces 300ms and reloads matching entries.

#### Scenario: Restore history query
- **WHEN** user clicks Restore on history entry
- **THEN** query text replaces Playground content
- **AND** Playground tab becomes active.

#### Scenario: Open history tab
- **WHEN** user clicks Open on non-error history entry with stored result
- **THEN** GUI opens read-only history tab
- **AND** displays stored rows in results panel.

#### Scenario: Delete or purge history
- **WHEN** user deletes one entry or purges history
- **THEN** backend removes matching records
- **AND** GUI refreshes history list and stats.

### Requirement: Result export
GUI MUST export visible or full result sets as CSV, TSV, or JSON.

#### Scenario: Copy visible rows
- **WHEN** user chooses Copy CSV, TSV, or JSON
- **THEN** GUI serializes visible result rows in selected format
- **AND** copies text to clipboard
- **AND** flashes success or failure.

#### Scenario: Download visible rows
- **WHEN** user chooses Download visible CSV, TSV, or JSON
- **THEN** GUI downloads file named with timestamp and selected extension.

#### Scenario: Full export
- **WHEN** user chooses full export
- **THEN** GUI re-executes last query with large page size
- **AND** downloads full returned rows in selected format.

#### Scenario: No exportable data
- **WHEN** export is requested without rows or query context
- **THEN** GUI flashes no data or no query message and does not download.

### Requirement: Inline result editing
GUI MUST provide opt-in inline edit mode for table result sets and generate SQL updates.

#### Scenario: Enable edit mode
- **WHEN** user clicks Edit after result table exists
- **THEN** GUI activates edit mode, marks table editable, and shows edit bar.

#### Scenario: No result set
- **WHEN** user clicks Edit without result table
- **THEN** GUI flashes `No result set to edit. Run a query first.`.

#### Scenario: Edit text or numeric cell
- **WHEN** user clicks editable text or numeric cell
- **THEN** input appears with current value
- **AND** Enter or blur commits change
- **AND** Escape cancels edit
- **AND** invalid numeric input is rejected.

#### Scenario: Edit boolean cell
- **WHEN** user clicks boolean cell
- **THEN** value cycles true, false, nullable null when allowed, then true.

#### Scenario: Generated column
- **WHEN** user clicks generated column
- **THEN** edit is blocked and status flash explains column cannot be edited.

#### Scenario: Preview updates
- **WHEN** user clicks Preview SQL with pending changes
- **THEN** GUI opens or updates read-only update-preview tab containing generated UPDATE statements.

#### Scenario: Apply updates
- **WHEN** user clicks Apply with pending changes and detected table
- **THEN** GUI executes generated UPDATE statements
- **AND** clears pending changes on success.

#### Scenario: Discard updates
- **WHEN** user clicks Discard
- **THEN** GUI restores original cell values and clears pending changes.

#### Scenario: Disable with pending changes
- **WHEN** user turns off edit mode with pending changes
- **THEN** GUI asks confirmation before discarding changes.

### Requirement: Tabs and draft persistence
GUI MUST manage Playground, DDL, update-preview, and history tabs with Monaco models.

#### Scenario: Playground tab
- **WHEN** app starts
- **THEN** non-closable Playground tab is active and writable.

#### Scenario: Read-only tabs
- **WHEN** DDL, update-preview, or history tab opens
- **THEN** editor switches to read-only model for that tab.

#### Scenario: Close tab
- **WHEN** user closes closable tab
- **THEN** it closes without a confirmation dialog
- **AND** editable queries are archived under Tabs → Closed before their models are disposed, and remain searchable/reopenable
- **AND** a storage failure keeps the tab open and displays an error
- **AND** read-only references close without being archived
- **AND** the neighboring tab becomes active, or a new blank query is created if none remain.

#### Scenario: Draft auto-save
- **WHEN** Playground content changes
- **THEN** GUI debounces 1200ms and saves draft to backend.

#### Scenario: Draft load
- **WHEN** editor initializes and saved draft exists
- **THEN** GUI loads draft if current Playground content is empty or default SQL.

#### Scenario: Before unload
- **WHEN** window unloads
- **THEN** GUI sends current Playground content to draft save endpoint.

### Requirement: SQL completions and AI completion
GUI MUST provide local schema-aware SQL completions and opt-in Supermaven inline completion using the official Neovim agent protocol.

#### Scenario: Schema completion update
- **WHEN** schema refresh succeeds
- **THEN** GUI updates Monaco SQL completions from cached schemas.

#### Scenario: AI status check
- **WHEN** app starts and every 2 seconds after
- **THEN** GUI reads local agent status without making a database request
- **AND** Settings shows off, starting/connecting, activation, ready or error.

#### Scenario: Explicit enable and activation
- **WHEN** the user enables Supermaven in Settings (off by default)
- **THEN** the backend downloads the official agent, verifies its SHA-256 checksum and starts it outside project directories
- **AND** any required sign-in or free-tier action is user initiated.

#### Scenario: AI completion request
- **WHEN** an enabled, focused, editable SQL tab triggers inline completion
- **THEN** GUI debounces the active draft and translates the UTF-16 cursor offset to agent UTF-8 bytes
- **AND** a bounded active-connection schema reference supplies table/column types, primary/unique keys and foreign keys in a neutral virtual document, never executed SQL
- **AND** no connection credentials, result rows or other tabs are added
- **AND** Tab accepts ghost text and Escape dismisses it
- **AND** cancelled, stale, unsupported destructive edits and provably unknown new table/column references are ignored; local schema completion remains independent.

### Requirement: Keyboard shortcuts and commands
GUI MUST support primary keyboard shortcuts.

#### Scenario: Run shortcut
- **WHEN** user presses `Cmd/Ctrl+Enter`
- **THEN** GUI runs execution target if Playground tab is active.

#### Scenario: Toggle sidebar shortcut
- **WHEN** user presses `Cmd/Ctrl+B`
- **THEN** sidebar content collapses or expands.

#### Scenario: Clear results shortcut
- **WHEN** user presses `Cmd/Ctrl+Shift+L`
- **THEN** results panel resets to Ready and flashes results cleared.

#### Scenario: Clear editor button
- **WHEN** user clicks Clear while Playground is active
- **THEN** Playground content becomes empty and editor focuses.

### Requirement: Pane resizing
GUI MUST allow resizing sidebar and results panes.

#### Scenario: Resize sidebar
- **WHEN** user drags vertical sidebar handle
- **THEN** sidebar width changes within configured bounds
- **AND** editor layout recalculates.

#### Scenario: Resize results panel
- **WHEN** user drags horizontal results handle
- **THEN** results height changes within configured bounds
- **AND** editor layout recalculates.

### Requirement: Local persistence
GUI MUST persist settings, connections, script drafts, and query history in local SQLite storage.

#### Scenario: Storage migration
- **WHEN** app starts
- **THEN** required tables and columns exist for settings, connections, script drafts, and query history
- **AND** default pool size is present when missing.

#### Scenario: Password storage
- **WHEN** connection password is saved
- **THEN** backend persists password via platform password mechanism when available
- **AND** resolves password before connection test, edit, and query execution.
