<script lang="ts">
  import { copyText, readClipboardText } from './lib/clipboard';
  import { save } from '@tauri-apps/plugin-dialog';
  import { isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount, tick } from 'svelte';
  import Dropdown from './components/Dropdown.svelte';
  import ActionMenu from './components/ActionMenu.svelte';
  import ConnectionCard from './components/ConnectionCard.svelte';
  import ConnectionEditor from './components/ConnectionEditor.svelte';
  import ConnectionPill from './components/ConnectionPill.svelte';
  import ResultsGrid from './components/ResultsGrid.svelte';
  import Modal from './components/Modal.svelte';
  import QueryTimingDialog from './components/QueryTimingDialog.svelte';
  import HistoryStorageDialog from './components/HistoryStorageDialog.svelte';
  import HistoryEntryCard from './components/HistoryEntryCard.svelte';
  import HistoryTransactionGroup from './components/HistoryTransactionGroup.svelte';
  import ActivityPanel from './components/ActivityPanel.svelte';
  import AppUpdates from './components/AppUpdates.svelte';
  import DiagnosticDialog from './components/DiagnosticDialog.svelte';
  import ResumeConnectionDialog from './components/ResumeConnectionDialog.svelte';
  import type { DiagnosticReport } from './lib/diagnostics';
  import CommitModeSelect from './components/CommitModeSelect.svelte';
  import { commitFeedbackLabel, commitModeLabels, transactionCompletionLabel } from './lib/commitMode';
  import ExplainDialog from './components/ExplainDialog.svelte';
  import { createExplainWorkflow, initialExplainState } from './lib/explain';
  import type { ExplainOptions, ExplainProgress } from './lib/explain';
  import Icon from './components/Icon.svelte';
  import SqlEditor from './components/SqlEditor.svelte';
  import SqlPreview from './components/SqlPreview.svelte';
  import Switch from './components/Switch.svelte';
  import PaneResizer from './components/PaneResizer.svelte';
  import QueryTab from './components/QueryTab.svelte';
  import TabsPanel from './components/TabsPanel.svelte';
  import Cat from './components/Cat.svelte';
  import WorkspacePicker from './components/WorkspacePicker.svelte';
  import { APPEARANCE_KEY, THEME_STORAGE_KEY, connectionAppearanceDraft, readThemePreference, resolveThemePreference, workspaceAppearance, restoreAppearance, shortType } from './lib/appearance';
  import { connectionOptions, connectionTestTimeout } from './lib/connectionOptions';
  import { hasNativeMacTitlebar } from './lib/nativeTitlebar';
  import { sameConnectionSettings } from './lib/connectionEditor';
  import { LAST_CONNECTION_KEY, restoreLastConnectionId, resumeConnection } from './lib/connectionRun';
  import { CONNECTION_SCOPE_KEY, GLOBAL_CONNECTION_KEY, connectionTargets, newTabConnection, restoreConnectionScope, switchTabConnection } from './lib/connectionScope';
  import type { ConnectionScope } from './lib/connectionScope';
  import type { ConnectionAppearance, ThemePreference } from './lib/appearance';
  import { serializeRows } from './lib/export';
  import { api, errorMessage } from './lib/ipc';
  import { startActivity } from './lib/activity';
  import { formatExecutionLog } from './lib/executionLog';
  import { AUTO_UPDATE_CHECK_KEY, createUpdateChecker, initialUpdateState, updateInstallBlock } from './lib/appUpdates';
  import type { UpdateProgress } from './lib/appUpdates';
  import { formatRunningDuration } from './lib/status';
  import { literalTextAttributes } from './lib/textInput';
  import { canRequestSupermaven } from './lib/supermavenInline';
  import { createHistorySearch, groupHistory } from './lib/history';
  import type { HistorySearchState } from './lib/history';
  import { editorSearchShortcut, workspaceSearchShortcut } from './lib/editorSearch';
  import { DEFAULT_EDITOR_FONT_SIZE, EDITOR_FONT_STORAGE_KEY, fontSizeShortcut, resizeEditorFont, restoreEditorFontSize } from './lib/editorFont';
  import type { QueryTimingDetails } from './lib/queryTiming';
  import { createConnectionTester } from './lib/connectionTest';
  import { createConnectionHealthcheck, createWindowFocusTracker } from './lib/connectionHealthcheck';
  import type { ConnectionTestState } from './lib/connectionTest';
  import { parseConnectionReference, serializeConnectionReference } from './lib/connectionClipboard';
  import { CONNECTION_ORDER_KEY, orderConnections, restoreConnectionOrder } from './lib/connectionOrder';
  import { parseTabReference, serializeTabReference } from './lib/tabClipboard';
  import { clampSidebarWidth, DEFAULT_SIDEBAR_WIDTH, restoreSidebarWidth, sidebarWidthBounds } from './lib/sidebarLayout';
  import { listDropTarget, listScrollDelta, reorderList } from './lib/listDrag';
  import type { ListDrop } from './lib/listDrag';
  import { assessRowEditing, buildRowUpdates } from './lib/rowEditing';
  import { sameCellValue } from './lib/resultsGrid';
  import type { EditTarget } from './lib/rowEditing';
  import { archiveTab, canUseResult, createDraftWriter, createHistoryTab, createQueryTab, duplicateQueryTab, reopenArchivedTab, restoreWorkspace, serializeWorkspace } from './lib/workspace';
  import type { ArchivedTab, PendingChange, ResultSession, WorkspaceTab } from './lib/workspace';
  import type {
    SupermavenStatus,
    AppError,
    CommitMode,
    ConnectionInput,
    ConnectionRecord,
    ConnectionStats,
    ExecutionTarget,
    HistoryEntry,
    HistoryStats,
    HistoryStorageStats,
    HistoryTransaction,
    ForeignKeyLookup,
    QueryResult,
    RowUpdate,
    SchemaItem,
    Settings,
    TxCompletion,
    TxStatus
  } from './lib/types';

  const DEFAULT_SQL = `-- Welcome to Catabase\n-- Add a PostgreSQL connection, then run SQL with Cmd/Ctrl+Enter.\nselect now() as connected_at;`;
  const PAGE_SIZE = 200;
  const VIM_MODE_STORAGE_KEY = 'catabase.vimMode';

  type SidebarTab = 'connections' | 'schema' | 'history' | 'activity' | 'tabs' | 'settings';
  type EditorHandle = {
    formatSql: () => void;
    find: (replace?: boolean) => void;
    getExecutionTarget: (splitSql: typeof api.splitSql) => Promise<ExecutionTarget>;
    clearHighlights: () => void;
    markError: (offset: number, message: string) => void;
    getViewState: () => WorkspaceTab['viewState'];
    getSnapshot: () => { tabId: string; sql: string; viewState: WorkspaceTab['viewState'] };
  };
  type SchemaGroup = { name: string; items: SchemaItem[] };
  type Operation = { token: string; tabId: string; connectionId: string; kind: 'query' | 'load' | 'export' | 'apply' | 'explain'; startedAt: number; cancelling: boolean };
  const operationLabels = { query: 'Running', load: 'Loading rows', export: 'Exporting', apply: 'Applying edits', explain: 'Explaining' };

  export let onWorkspaceReady: () => void = () => {};

  let sidebar: SidebarTab = 'connections';
  let nativeTitlebar = false;
  let sidebarOpen = true;
  let sidebarElement: HTMLElement | undefined;
  let sidebarWidth = DEFAULT_SIDEBAR_WIDTH;
  let workspaceWidth = 1280;
  let workspaceElement: HTMLElement;
  let resizingSidebar = false;
  let sql = DEFAULT_SQL;
  let activeTab = 'playground';
  let tabs: WorkspaceTab[] = [createQueryTab('playground', 'Query 1', DEFAULT_SQL)];
  let archivedTabs: ArchivedTab[] = [];
  let tabCloseTask: Promise<void> | null = null;
  let tabKind: WorkspaceTab['kind'] = 'query';
  let tabConnectionId: string | null = null;
  let connectionScope: ConnectionScope = 'tab';
  let globalConnectionId: string | null = null;
  let editorViewState: WorkspaceTab['viewState'] = null;
  let renamingTab = '';
  let renameValue = '';
  let tabBar: HTMLElement;
  let draggingTab = '';
  let tabDrop: ListDrop | null = null;
  let tabDragPointer: { x: number; y: number } | null = null;
  let tabDragFrame = 0;
  let workspaceLoaded = false;
  let editorRef: EditorHandle | undefined;
  let editorFontSize = DEFAULT_EDITOR_FONT_SIZE;
  let vimModeEnabled = false;
  let theme: ThemePreference = 'tuxedo';
  let systemLight = false;
  let appearances: Record<string, ConnectionAppearance> = {};
  let picker: 'commands' | 'connections' | null = null;
  let pickerHistoryQuery = '';
  let cursorLine = 1;
  let cursorColumn = 1;
  let running = false;
  let nowMs = Date.now();
  let operation: Operation | null = null;
  let explainSession: { query: string; tabId: string; connectionId: string; connectionName: string } | null = null;
  let explainOpening = false;
  let explainState = initialExplainState();
  let explainEventsReady: Promise<void> | null = null;
  let explainEventsError = '';
  const explainWorkflow = createExplainWorkflow({
    begin: connectionId => {
      const op = beginOperation('explain', connectionId);
      if (explainSession) op.tabId = explainSession.tabId;
      return op.token;
    },
    finish: token => { if (operation?.token === token) finishOperation(operation); },
    run: request => connectionTester.query(request.connectionId, () => api.explainQuery(request)),
    share: api.shareExplain,
    cancel: api.cancelQuery
  }, state => {
    explainState = state;
    if (operation?.kind === 'explain' && operation.token === state.token) {
      operation = { ...operation, cancelling: state.cancelling };
    }
  }, errorMessage);
  let resultSession: ResultSession | null = null;
  let result: QueryResult | null = null;
  let originalRows: Record<string, unknown>[] = [];
  let queryError: AppError | null = null;
  let statusFlash = 'Ready';
  let statusTiming: QueryTimingDetails | null = null;
  let timingDetails: QueryTimingDetails | null = null;
  let toastMessage = '';
  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  let toastError = false;
  let mounted = false;
  let diagnosticReport: DiagnosticReport | null = null;
  let diagnosticsOpen = false;
  let diagnosticsPending = false;
  let diagnosticsLoading = false;
  let diagnosticsRetryTimer: ReturnType<typeof setTimeout> | null = null;
  let txStatus: TxStatus = { active: false, startedAt: null };
  let txStatusKnown = false;
  let txCompletions = new Map<string, TxCompletion>();
  let editMode = false;
  let pendingChanges: PendingChange[] = [];

  let connections: ConnectionRecord[] = [];
  let lastConnectionId: string | null = null;
  let queryPreparing = false;
  let reconnectReview: { connectionId: string; target: ExecutionTarget; resolve: (connection: ConnectionRecord | null) => void } | null = null;
  let connectionOrder: string[] = [];
  let connectionList: HTMLUListElement | undefined;
  let connectionDrag: { id: string; x: number; y: number } | null = null;
  let connectionDrop: ListDrop | null = null;
  let connectionDragFrame = 0;
  let connectionTests: Record<string, ConnectionTestState | null> = {};
  let connectedIds = new Set<string>();
  let failedConnectionIds: string[] = [];
  const connectionTester = createConnectionTester(
    (id, state) => { connectionTests = { ...connectionTests, [id]: state }; },
    errorMessage,
    Date.now,
    id => { if (connectedIds.has(id) && !failedConnectionIds.includes(id)) failedConnectionIds = [...failedConnectionIds, id]; }
  );
  let settings: Settings = { poolSize: 5, supermavenEnabled: false, commitMode: 'smart' };
  let poolStats: ConnectionStats | null = null;
  let poolStatsConnectionId: string | null = null;
  let poolStatsRequest = 0;
  let poolSizeInput = 5;
  let supermaven: SupermavenStatus = { enabled: false, state: 'off', message: 'Supermaven is disabled.' };
  let supermavenBusy = false;
  let schemaItems: SchemaItem[] = [];
  let schemaSearch = '';
  let schemaGroups: SchemaGroup[] = [];
  let schemaLoading = false;
  let schemaError = '';
  let history: HistoryEntry[] = [];
  let historyStats: HistoryStats = { total: 0, errors: 0, lastExecutedAt: null };
  let historySearch = '';
  let historyTotal = 0;
  let historyLoading = false;
  let historyRevision = 0;
  let historyRequest = 0;
  let historySaveError = '';
  let historyRefreshTimer: ReturnType<typeof setTimeout> | null = null;
  let historyTimer: ReturnType<typeof setTimeout> | null = null;
  let historyStorage: { entry: HistoryEntry | null; stats: HistoryStorageStats | null; error: string } | null = null;
  let pickerHistory: HistorySearchState = { entries: [], total: 0, loading: false, error: '' };
  const pickerHistorySearch = createHistorySearch(
    (search) => api.historyPage(search || null, 0, 20),
    (state) => pickerHistory = state,
    errorMessage
  );
  let resultsHeight = 360;
  let resizingResults = false;
  let showUpdatePreview = false;
  let checkOriginalValues = true;
  let updateReviewError = '';
  let discardRunConfirmation: { changeCount: number; connectionName?: string; globalSelection?: boolean; resolve: (confirmed: boolean) => void } | null = null;
  let schemaRequest = 0;
  let schemaRetryRequest = 0;

  let showConnectionModal = false;
  let connectionForm: ConnectionInput = emptyConnection();
  let connectionAppearanceForm = connectionAppearanceDraft({});
  let modalFeedback = '';
  let testingConnection = false;
  let savingConnection = false;
  let connectionMutation: string | null = null;
  let txStatusRequest = 0;
  let draftTimer: ReturnType<typeof setTimeout> | null = null;
  let bootstrapTask = Promise.resolve();
  let closingWorkspace = false;
  let updateState = initialUpdateState();
  let automaticUpdates = true;
  let updateFocused = false;
  let updateInstalling = false;
  let updateInstallError = '';
  let updateProgress: UpdateProgress | null = null;
  const updateChecker = createUpdateChecker(
    { status: api.updaterStatus, check: api.checkForUpdate },
    state => { updateState = state; }, errorMessage
  );
  let draftStatus: 'pending' | 'saving' | 'saved' | 'error' = 'saved';
  let draftRevision = 0;
  const writeWorkspaceDraft = createDraftWriter((content) => api.saveDraft('workspace.v1', content));
  const nativeCleanup: Array<() => void> = [];

  $: savedTabConnection = connections.find(connection => connection.id === tabConnectionId) ?? null;
  $: activeConnection = connectedIds.has(tabConnectionId ?? '') ? savedTabConnection : null;
  $: tabConnectionTest = savedTabConnection ? connectionTests[savedTabConnection.id] ?? null : null;
  $: statusConnection = activeConnection ?? (tabConnectionTest?.status === 'testing'
    || (tabConnectionTest?.status === 'error' && tabConnectionTest.origin === 'test') ? savedTabConnection : null);
  $: connectionLabels = Object.fromEntries(connections.map(connection =>
    [connection.id, [connection.name, appearances[connection.id]?.environment].filter(Boolean).join(' · ')]
  ));
  $: liveConnectionId = activeConnection?.id ?? null;
  $: focusedConnectionLocked = tabs.find(tab => tab.id === activeTab)?.connectionLocked === true;
  $: lockedConnectionName = savedTabConnection?.name ?? 'Unavailable connection';
  $: connectionAppearance = workspaceAppearance(appearances, activeConnection?.id ?? null,
    showConnectionModal ? { id: connectionForm.id, appearance: connectionAppearanceForm } : undefined);
  $: resolvedTheme = resolveThemePreference(theme, systemLight);
  $: themeKey = `${resolvedTheme}:${connectionAppearance.color}`;
  $: if (mounted) {
    document.documentElement.dataset.theme = resolvedTheme;
    document.documentElement.dataset.conn = connectionAppearance.color;
  }
  $: if (mounted && isTauri()) {
    void getCurrentWindow().setTheme(theme === 'system' ? null : theme === 'snow' ? 'light' : 'dark')
      .catch(error => console.warn('Could not update the native window theme', error));
  }
  $: connectionPickerItems = connections.map(connection => {
    const appearance = appearances[connection.id];
    const account = `${connection.username} · ${connection.database}`;
    return {
      id: `connection:${connection.id}`,
      label: connection.name,
      detail: focusedConnectionLocked ? 'Unlock the focused tab to switch' : account,
      searchText: `${account} ${appearance?.environment ?? ''}`,
      appearance,
      group: 'Connections',
      active: connection.id === activeConnection?.id,
      disabled: focusedConnectionLocked || running || Boolean(connectionMutation)
    };
  });
  $: pickerItems = picker === 'connections'
    ? [
      ...connectionPickerItems,
      ...(activeConnection ? [{ id: 'command:disconnect', label: `Disconnect ${activeConnection.name}`, detail: 'Keep saved connection & drafts', group: 'Actions', disabled: running || Boolean(connectionMutation) }] : [])
    ]
    : [
      ...tabs.map(t => ({ id: `tab:${t.id}`, label: t.title, title: t.id === activeTab ? sql : t.sql, searchText: t.id === activeTab ? sql : t.sql, detail: `${t.kind === 'reference' ? 'Read-only reference' : 'Saved query'} · ${connections.find(c => c.id === t.connectionId)?.name ?? 'No connection'}`, group: 'Open tabs' })),
      ...archivedTabs.map(t => ({ id: `closed-tab:${t.id}`, label: t.title, title: t.sql, searchText: t.sql, detail: `Reopen · ${connections.find(c => c.id === t.connectionId)?.name ?? 'No connection'} · ${formatHistoryTime(t.closedAt)}`, group: 'Closed tabs' })),
      ...connectionPickerItems,
      ...schemaItems.map(item => ({ id: `table:${item.schema}.${item.name}`, label: `${item.schema}.${item.name}`, detail: `${item.columns.length} columns`, group: 'Tables & views' })),
      ...pickerHistory.entries.map(entry => ({ id: `history:${entry.id}`, label: entry.query.replace(/\s+/g, ' ').slice(0, 200), title: entry.query, detail: `#${entry.id} · ${entry.durationMs} ms · ${connections.find(c => c.id === entry.connectionId)?.name ?? 'Unavailable connection'} · ${formatHistoryTime(entry.executedAt)} · ${entry.isError ? 'Error' : entry.hasResult ? 'Snapshot' : 'SQL only'}`, group: 'History', matched: true })),
      ...(['new', 'connections', 'schema', 'history', 'activity', 'tabs', 'settings', 'find', 'format'] as const).map(id => ({ id: `command:${id}`, label: ({ new: 'New query', connections: 'Switch connection', schema: 'Browse schema', history: 'Query history', activity: 'Activity log', tabs: 'Browse open & closed tabs', settings: 'Settings', find: 'Find in SQL', format: 'Format SQL' })[id], detail: id === 'new' ? '⌘T' : id === 'find' ? '⌘F' : id === 'connections' && focusedConnectionLocked ? 'Unlock the focused tab first' : '', disabled: id === 'connections' && focusedConnectionLocked, group: 'Commands' })),
      ...(statusTiming ? [{ id: 'command:query-timing', label: 'Show query timing', detail: '', group: 'Commands' }] : [])
    ];
  $: if (picker !== 'commands') pickerHistorySearch.cancel();
  $: running = operation !== null;
  $: updateBlocked = updateInstallBlock({
    ready: workspaceLoaded,
    busy: Boolean(operation || connectionMutation || queryPreparing || showConnectionModal || schemaLoading || showUpdatePreview || explainSession || explainOpening || discardRunConfirmation || tabCloseTask || closingWorkspace),
    transactionKnown: txStatusKnown,
    transactionActive: txStatus.active,
    stagedEdits: tabs.reduce((count, tab) => count + (tab.id === activeTab ? pendingChanges.length : tab.pendingChanges.length), 0),
    cellEditing: tabs.some(tab => tab.id === activeTab ? editMode : tab.editMode)
  });
  $: visibleSidebarWidth = clampSidebarWidth(sidebarWidth, workspaceWidth);
  $: activeOperation = operation?.tabId === activeTab ? operation : null;
  $: operationText = operation
    ? `${operation.cancelling ? 'Cancelling' : operationLabels[operation.kind]} · ${formatRunningDuration(operation.startedAt, nowMs, 'seconds')}`
    : '';
  $: transactionDuration = formatRunningDuration(txStatus.startedAt, nowMs);
  $: currentPoolStats = poolStatsConnectionId === activeConnection?.id ? poolStats : null;
  $: historyContext = tabs.find(tab => tab.id === activeTab)?.historyContext ?? null;
  $: resultContext = resultSession ?? historyContext;
  $: resultCommitLabel = result?.commitFeedback
    ? commitFeedbackLabel(result.commitFeedback, txCompletions.get(result.commitFeedback.transactionId ?? ''))
    : '';
  $: resultUsable = Boolean(activeConnection) && canUseResult(resultSession, tabConnectionId) && tabKind === 'query';
  $: editEligibility = assessRowEditing(result, editMode ? originalRows : result?.rows);
  $: editableColumns = editEligibility.target?.editableColumns.map((column) => column.name) ?? [];
  $: reviewedUpdates = showUpdatePreview && resultUsable && editEligibility.target
    && pendingChanges.every((change) => editableColumns.includes(change.column))
    ? buildRowUpdates(editEligibility.target, originalRows, pendingChanges, checkOriginalValues)
    : [];
  $: editDisabledReason = running || connectionMutation ? 'Wait for the current operation to finish before editing.'
    : queryError ? 'Show previous results before editing.'
    : activeConnection?.options?.readOnly ? 'Cell editing is disabled for this read-only connection.'
    : historyContext ? 'History snapshots are read-only. Open a new query to run this SQL again.'
    : resultSession?.snapshot ? 'History snapshots are read-only. Re-run the query to edit.'
    : result && !resultUsable ? 'Re-run this query on its original connection before editing.'
    : editEligibility.reason;
  $: canLoadMore = Boolean(result?.autoLimited && result.hasMore && resultUsable && !txStatus.active && !running && !connectionMutation && !pendingChanges.length);
  $: schemaGroups = groupSchemaItems(schemaItems, schemaSearch);
  $: if (workspaceLoaded) scheduleDraftSave(sql, activeTab, tabConnectionId, tabs, archivedTabs);
  $: if (workspaceLoaded) void refreshSchemaForConnection(liveConnectionId);
  $: if (workspaceLoaded) void loadConnectionStats(activeConnection?.id ?? null);
  $: exportItems = [
    { id: 'copy-csv', label: 'Copy as CSV' },
    { id: 'copy-tsv', label: 'Copy as TSV' },
    { id: 'copy-json', label: 'Copy as JSON' },
    { id: 'save-csv', label: 'Save CSV…' },
    { id: 'save-tsv', label: 'Save TSV…' },
    { id: 'save-json', label: 'Save JSON…' },
    { id: 'more-csv', label: 'Re-run & save up to 10k (CSV)…', disabled: !resultUsable || !result?.autoLimited || txStatus.active || running || Boolean(connectionMutation) },
    { id: 'more-json', label: 'Re-run & save up to 10k (JSON)…', disabled: !resultUsable || !result?.autoLimited || txStatus.active || running || Boolean(connectionMutation) }
  ];

  onMount(() => {
    mounted = true;
    nativeTitlebar = hasNativeMacTitlebar(isTauri(), navigator.platform);
    const media = window.matchMedia('(prefers-color-scheme: light)');
    systemLight = media.matches;
    const onSystemTheme = (event: MediaQueryListEvent) => { systemLight = event.matches; };
    media.addEventListener('change', onSystemTheme);
    theme = readThemePreference();
    try {
      appearances = restoreAppearance(localStorage.getItem(APPEARANCE_KEY));
    } catch { /* Appearance controls remain usable without storage. */ }
    try { connectionOrder = restoreConnectionOrder(localStorage.getItem(CONNECTION_ORDER_KEY)); } catch { /* Keep the order in memory. */ }
    try { lastConnectionId = restoreLastConnectionId(localStorage.getItem(LAST_CONNECTION_KEY)); } catch { /* A tab's saved connection remains available. */ }
    try {
      connectionScope = restoreConnectionScope(localStorage.getItem(CONNECTION_SCOPE_KEY));
      globalConnectionId = restoreLastConnectionId(localStorage.getItem(GLOBAL_CONNECTION_KEY));
    } catch { /* Connection switching defaults to the focused tab. */ }
    try { editorFontSize = restoreEditorFontSize(localStorage.getItem(EDITOR_FONT_STORAGE_KEY)); } catch { /* Font controls still work without storage. */ }
    try { vimModeEnabled = localStorage.getItem(VIM_MODE_STORAGE_KEY) === 'true'; } catch { /* Keep the default for this session. */ }
    try { automaticUpdates = localStorage.getItem(AUTO_UPDATE_CHECK_KEY) !== 'false'; } catch { /* Keep the default for this session. */ }
    try {
      const savedResultsHeight = Number(localStorage.getItem('catabase.resultsHeight'));
      if (Number.isFinite(savedResultsHeight) && savedResultsHeight >= 180) resultsHeight = savedResultsHeight;
      sidebarWidth = restoreSidebarWidth(localStorage.getItem('catabase.sidebarWidth'));
    } catch { /* Use the default pane sizes. */ }
    const layoutObserver = new ResizeObserver(() => {
      workspaceWidth = workspaceElement.getBoundingClientRect().width;
    });
    workspaceWidth = workspaceElement.getBoundingClientRect().width;
    layoutObserver.observe(workspaceElement);
    bootstrapTask = bootstrap();
    void registerNativeLifecycle();
    void registerHistoryEvents();
    explainEventsReady = registerExplainEvents();
    window.addEventListener('keydown', captureWorkspaceSearch, true);
    window.addEventListener('keydown', handleKeydown);
    window.addEventListener('beforeunload', flushDraftOnUnload);
    const statsInterval = window.setInterval(() => {
      if (activeConnection && !closingWorkspace) void loadConnectionStats();
    }, 5000);
    let nextRunningStatsAt = 0;
    const runningInterval = window.setInterval(() => {
      if (!running && !txStatus.active) return;
      nowMs = Date.now();
      if (running && activeConnection && nowMs >= nextRunningStatsAt) {
        nextRunningStatsAt = nowMs + 1000;
        void loadConnectionStats();
      }
    }, 100);
    const aiInterval = window.setInterval(() => { if (!closingWorkspace) void checkSupermaven(); }, 2000);
    const healthcheck = createConnectionHealthcheck(() => ({
      connectionId: activeConnection?.id ?? null,
      state: activeConnection ? connectionTests[activeConnection.id] ?? null : null,
      ready: mounted && workspaceLoaded && !closingWorkspace,
      busy: Boolean(operation || connectionMutation || queryPreparing || showConnectionModal || explainSession || explainOpening || schemaLoading || editMode || discardRunConfirmation || tabCloseTask),
      transactionIdle: txStatusKnown && !txStatus.active
    }), (id) => {
      const connection = connections.find(candidate => candidate.id === id);
      if (connection) void testSavedConnection(connection, true);
    });
    const nativeFocus = createWindowFocusTracker(() => getCurrentWindow().isFocused(), focused => {
      updateFocused = focused;
      healthcheck.setFocused(focused);
      pollAppUpdates();
    });
    // Native window focus is authoritative: a background WebView may still
    // report itself visible/focused. DOM blur also closes the event-delivery gap.
    // A returning WebView may regain DOM focus without a new native-window
    // event (for example after native menus). Validate that focus natively too.
    const onWindowFocus = () => { if (isTauri()) void nativeFocus.refresh(); else nativeFocus.set(true); };
    const onWindowBlur = () => nativeFocus.set(false);
    const onVisibility = () => healthcheck.setVisible(document.visibilityState === 'visible');
    window.addEventListener('focus', onWindowFocus);
    window.addEventListener('blur', onWindowBlur);
    document.addEventListener('visibilitychange', onVisibility);
    onVisibility();
    if (isTauri()) {
      void (async () => {
        try {
          const window = getCurrentWindow();
          const unlisten = await window.onFocusChanged(({ payload }) => nativeFocus.set(payload));
          if (!mounted) { unlisten(); return; }
          nativeCleanup.push(unlisten);
          await nativeFocus.refresh();
        } catch (error) {
          nativeFocus.set(false);
          console.warn('Automatic connection checks unavailable: could not determine window focus', error);
        }
      })();
    } else nativeFocus.set(document.hasFocus());
    const healthInterval = window.setInterval(() => {
      nowMs = Date.now();
      healthcheck.poll();
      pollAppUpdates();
    }, 1000);

    return () => {
      flushDraftOnUnload();
      mounted = false;
      healthcheck.dispose();
      updateChecker.dispose();
      explainWorkflow.dispose();
      nativeFocus.dispose();
      window.removeEventListener('focus', onWindowFocus);
      window.removeEventListener('blur', onWindowBlur);
      document.removeEventListener('visibilitychange', onVisibility);
      media.removeEventListener('change', onSystemTheme);
      layoutObserver.disconnect();
      for (const cleanup of nativeCleanup) cleanup();
      window.removeEventListener('keydown', captureWorkspaceSearch, true);
      window.removeEventListener('keydown', handleKeydown);
      window.removeEventListener('beforeunload', flushDraftOnUnload);
      window.clearInterval(statsInterval);
      window.clearInterval(runningInterval);
      window.clearInterval(aiInterval);
      window.clearInterval(healthInterval);
      if (toastTimer) window.clearTimeout(toastTimer);
      if (diagnosticsRetryTimer) window.clearTimeout(diagnosticsRetryTimer);
      if (draftTimer) window.clearTimeout(draftTimer);
      if (historyTimer) window.clearTimeout(historyTimer);
      if (historyRefreshTimer) window.clearTimeout(historyRefreshTimer);
      pickerHistorySearch.cancel();
      stopResultsResize();
    };
  });

  async function bootstrap() {
    await Promise.allSettled([
      loadSettings(),
      loadConnections(),
      refreshHistory(),
      checkSupermaven(),
      refreshTxStatus(),
      updateChecker.initialize()
    ]);
    await loadDraft();
    workspaceLoaded = true;
    await tick();
    if (mounted) onWorkspaceReady();
    if (mounted && isTauri()) {
      void loadDiagnostics(true);
      // macOS may finish writing its crash log after a quick relaunch.
      diagnosticsRetryTimer = setTimeout(() => {
        if (mounted && document.visibilityState === 'visible' && !closingWorkspace) void loadDiagnostics(true);
      }, 15_000);
    }
  }

  async function loadDiagnostics(automatic = false) {
    if (diagnosticsLoading || (automatic && (diagnosticsOpen || diagnosticsPending))) return;
    diagnosticsLoading = true;
    try {
      const report = await api.diagnosticsReport(!automatic);
      if (!mounted) return;
      if (automatic && !report.newCrash) return;
      diagnosticReport = report;
      if (automatic) diagnosticsPending = true;
      else diagnosticsOpen = true;
    } catch (error) {
      if (!automatic) flash(`Could not load diagnostics: ${errorMessage(error)}`, true);
    } finally { diagnosticsLoading = false; }
  }

  $: if (diagnosticsPending && workspaceLoaded && !running && !connectionMutation && !closingWorkspace
      && !showConnectionModal && !showUpdatePreview && !discardRunConfirmation && !timingDetails
      && !picker && !explainSession && !explainOpening && !updateInstalling && !historyStorage && !queryPreparing && !reconnectReview) {
    diagnosticsPending = false;
    diagnosticsOpen = true;
  }

  function closeDiagnostics() {
    diagnosticsOpen = false;
    const id = diagnosticReport?.id;
    if (id) void api.acknowledgeDiagnostics(id).catch(error => flash(`Could not save the report review: ${errorMessage(error)}`, true));
  }

  $: if (failedConnectionIds.length && workspaceLoaded && !operation && !connectionMutation && !queryPreparing
      && !txStatus.active && !closingWorkspace && !showConnectionModal) {
    void disconnectFailedConnection(failedConnectionIds[0]);
  }

  async function disconnectFailedConnection(id: string) {
    failedConnectionIds = failedConnectionIds.filter(candidate => candidate !== id);
    if (!connectedIds.has(id) || connectionTests[id]?.status !== 'error') return;
    connectionMutation = 'Disconnecting…';
    try {
      await api.disconnectConnection(id);
      connectedIds = new Set([...connectedIds].filter(candidate => candidate !== id));
      // Preserve the connection association, previous results and staged edits.
      // They remain read-only until the user deliberately reconnects.
      await loadConnections();
    } catch (error) { flash(`Could not disconnect: ${errorMessage(error)}`, true); }
    finally { connectionMutation = null; }
  }

  async function registerExplainEvents() {
    try {
      const unlisten = await listen<ExplainProgress>('explain-progress', event => explainWorkflow.progress(event.payload));
      if (mounted) nativeCleanup.push(unlisten); else unlisten();
    } catch (error) { explainEventsError = errorMessage(error); }
  }

  async function registerHistoryEvents() {
    if (!isTauri()) return;
    const cleanups = await Promise.all([
      listen('history-changed', () => {
        // Coalesce bursts without waiting for the query/transaction to finish.
        if (historyRefreshTimer) return;
        historyRefreshTimer = setTimeout(() => {
          historyRefreshTimer = null;
          void refreshHistory();
          if (picker === 'commands') searchPickerHistory(pickerHistoryQuery);
        }, 50);
      }),
      listen<string>('history-error', event => {
        historySaveError = event.payload;
        flash(event.payload, true);
      }),
      listen<TxStatus>('tx-changed', event => {
        // An automatically opened transaction is visible while the triggering
        // statement is still running. Ignore older in-flight status responses.
        txStatusRequest++;
        updateTransactionStatus(event.payload);
      })
    ]);
    if (mounted) nativeCleanup.push(...cleanups);
    else cleanups.forEach(cleanup => cleanup());
  }

  async function registerNativeLifecycle() {
    if (!isTauri()) return;
    try {
      const unlistenClose = await getCurrentWindow().onCloseRequested(async (event) => {
        if (closingWorkspace) { event.preventDefault(); return; }
        closingWorkspace = true;
        try {
          await flushWorkspaceForClose();
          // Tauri waits for this callback before destroying the window.
        } catch (error) {
          event.preventDefault();
          closingWorkspace = false;
          flash(`Could not save drafts. The app is still open: ${errorMessage(error)}`, true);
        }
      });
      if (!mounted) { unlistenClose(); return; }
      nativeCleanup.push(unlistenClose);
      const unlistenExit = await listen('catabase:request-exit', async () => {
        if (closingWorkspace) return;
        closingWorkspace = true;
        try {
          await flushWorkspaceForClose();
          await api.finishExit();
        } catch (error) {
          closingWorkspace = false;
          flash(`Could not save drafts. The app is still open: ${errorMessage(error)}`, true);
        }
      });
      if (!mounted) { unlistenExit(); return; }
      nativeCleanup.push(unlistenExit);
      const unlistenUpdate = await listen<UpdateProgress>('catabase:update-progress', event => {
        if (updateInstalling) updateProgress = event.payload;
      });
      if (!mounted) { unlistenUpdate(); return; }
      nativeCleanup.push(unlistenUpdate);
      await api.enableExitGuard();
    } catch (error) {
      flash(`Draft protection on close is unavailable: ${errorMessage(error)}`, true);
    }
  }

  async function flushWorkspaceForClose() {
    await bootstrapTask;
    await tabCloseTask;
    if (renamingTab) finishRename(false);
    await tick();
    await persistDraftNow();
  }

  function pollAppUpdates() {
    updateChecker.poll({ automatic: automaticUpdates, foreground: updateFocused && document.visibilityState === 'visible', idle: !updateBlocked });
  }

  function setAutomaticUpdates(enabled: boolean) {
    automaticUpdates = enabled;
    try { localStorage.setItem(AUTO_UPDATE_CHECK_KEY, String(enabled)); } catch { /* Preference still applies in this session. */ }
    pollAppUpdates();
  }

  async function installAppUpdate() {
    if (updateInstalling || updateBlocked || !updateState.update || updateState.checking) return;
    const version = updateState.update.version;
    // Freeze editor shortcuts and workspace controls before taking the final
    // draft snapshot. Native code checks activity again under an exclusive gate.
    closingWorkspace = true;
    updateInstalling = true;
    updateInstallError = '';
    updateProgress = null;
    try {
      await flushWorkspaceForClose();
      await api.installUpdate(version);
    } catch (error) {
      updateInstallError = errorMessage(error);
      updateInstalling = false;
      closingWorkspace = false;
      updateProgress = null;
    }
  }

  function flushDraftOnUnload() {
    if (!closingWorkspace) void persistDraftNow().catch(() => {});
  }

  function snapshotActive(): WorkspaceTab {
    const tab = tabs.find((candidate) => candidate.id === activeTab)!;
    const editor = editorRef?.getSnapshot();
    const ownsEditor = editor?.tabId === activeTab;
    return { ...tab, sql: ownsEditor ? editor.sql : sql, connectionId: tabConnectionId, viewState: ownsEditor ? editor.viewState : editorViewState,
      session: resultSession ? { ...resultSession, data: result! } : null, error: queryError,
      originalRows, pendingChanges, editMode };
  }

  function captureActive() {
    const captured = snapshotActive();
    tabs = tabs.map((tab) => tab.id === activeTab ? captured : tab);
  }

  function displayTab(tab: WorkspaceTab) {
    activeTab = tab.id;
    sql = tab.sql;
    tabKind = tab.kind;
    tabConnectionId = tab.connectionId;
    editorViewState = tab.viewState;
    resultSession = tab.session;
    result = tab.session?.data ?? null;
    queryError = tab.error;
    originalRows = tab.originalRows;
    pendingChanges = tab.pendingChanges;
    editMode = tab.editMode;
    showUpdatePreview = false;
  }

  function switchTab(id: string) {
    if (id === activeTab) return;
    captureActive();
    const next = tabs.find((tab) => tab.id === id);
    if (next) displayTab(next);
  }

  function patchTab(id: string, patch: Partial<WorkspaceTab>) {
    if (id === activeTab) captureActive();
    tabs = tabs.map((tab) => tab.id === id ? { ...tab, ...patch } : tab);
    if (id === activeTab) {
      const tab = tabs.find((tab) => tab.id === id)!;
      // Keep the live editor/cursor untouched when an async result arrives.
      tabConnectionId = tab.connectionId;
      resultSession = tab.session;
      result = tab.session?.data ?? null;
      queryError = tab.error;
      originalRows = tab.originalRows;
      pendingChanges = tab.pendingChanges;
      editMode = tab.editMode;
    }
  }

  function defaultTabConnection() {
    return newTabConnection(connectionScope, globalConnectionId, tabConnectionId, focusedConnectionLocked);
  }

  function newQuery(content = '', connectionId = defaultTabConnection(), title?: string) {
    captureActive();
    const tab = createQueryTab(crypto.randomUUID(), title ?? `Query ${tabs.filter((tab) => tab.kind === 'query').length + 1}`, content, connectionId);
    tabs = [...tabs, tab];
    displayTab(tab);
  }

  function closeTab(id: string) {
    if (!workspaceLoaded || closingWorkspace || tabCloseTask) return;
    if (operation?.tabId === id) { flash('Cancel or wait for this query before closing its tab.'); return; }
    if (renamingTab) finishRename(false);
    tabCloseTask = saveAndCloseTab(id).finally(() => {
      tabCloseTask = null;
      // Restore focus after the brief archive-save interaction lock is lifted.
      focusTabLabel(activeTab);
    });
  }

  async function saveAndCloseTab(id: string) {
    captureActive();
    const tab = tabs.find(item => item.id === id);
    if (!tab) return;
    const index = tabs.indexOf(tab);
    const remaining = tabs.filter(item => item.id !== id);
    if (!remaining.length) remaining.push(createQueryTab(crypto.randomUUID(), 'Query 1', '', defaultTabConnection()));
    const nextId = activeTab === id ? remaining[Math.min(index, remaining.length - 1)].id : activeTab;
    const archived = archiveTab(tab, new Date().toISOString());
    const nextArchive = archived ? [archived, ...archivedTabs.filter(item => item.id !== id)] : archivedTabs;
    if (draftTimer) { clearTimeout(draftTimer); draftTimer = null; }
    ++draftRevision;
    draftStatus = 'saving';
    try {
      // Commit the archive before removing the live tab. On failure the editor,
      // result and staged changes remain untouched and closing can be retried.
      await writeWorkspaceDraft(serializeWorkspace(remaining, nextId, nextArchive));
      archivedTabs = nextArchive;
      // Keep async results that may have arrived on a different open tab.
      tabs = tabs.filter(item => item.id !== id);
      if (!tabs.length) tabs = remaining;
      if (activeTab === id) displayTab(tabs.find(item => item.id === nextId)!);
      draftStatus = 'saved';
      flash(archived ? `${tab.title} archived. Reopen it from Tabs or search.` : `${tab.title} closed.`);
    } catch (error) {
      draftStatus = 'error';
      flash(`Could not save the closed tab. Nothing was closed: ${errorMessage(error)}`, true);
    }
  }

  function reopenTab(id: string) {
    if (!workspaceLoaded || closingWorkspace || tabCloseTask) return;
    const archived = archivedTabs.find(tab => tab.id === id);
    if (!archived) return;
    captureActive();
    const tab = tabs.find(tab => tab.id === id) ?? reopenArchivedTab(archived);
    if (!tabs.some(tab => tab.id === id)) tabs = [...tabs, tab];
    archivedTabs = archivedTabs.filter(tab => tab.id !== id);
    displayTab(tab);
    void persistDraftNow().catch(() => {});
    focusTabLabel(id);
  }

  function duplicateTab(id: string) {
    if (closingWorkspace || connectionMutation) return;
    if (renamingTab) finishRename(false);
    captureActive();
    const index = tabs.findIndex((tab) => tab.id === id);
    if (index < 0) { flash('This tab is no longer open.'); return; }
    const copy = duplicateQueryTab(tabs[index], crypto.randomUUID(), tabs);
    tabs = [...tabs.slice(0, index + 1), copy, ...tabs.slice(index + 1)];
    displayTab(copy);
    focusTabLabel(copy.id);
    flash(`${copy.title} created.`);
  }

  async function copyTab(id: string) {
    const tab = tabs.find((tab) => tab.id === id);
    if (!tab || closingWorkspace) return;
    try {
      await copyText(() => serializeTabReference(tab), 'Copy tab reference', tab.title);
      flash(`${tab.title} copied. Paste on a tab to duplicate.`);
    } catch (error) {
      flash(`Could not copy tab: ${errorMessage(error)}`, true);
    }
  }

  async function pasteTab(text?: string) {
    if (closingWorkspace || connectionMutation) return;
    try {
      const reference = text ?? await readClipboardText();
      const id = parseTabReference(reference);
      if (!id) { flash('Copy an open tab first, then paste on a tab to duplicate.'); return; }
      duplicateTab(id);
    } catch (error) {
      flash(`Could not paste tab: ${errorMessage(error)}`, true);
    }
  }

  async function startRename(id = activeTab) {
    if (renamingTab) finishRename(false);
    if (!tabs.some((tab) => tab.id === id)) return;
    renamingTab = id;
    renameValue = tabs.find((tab) => tab.id === id)?.title ?? '';
    await tick();
    if (renamingTab === id) {
      const input = tabBar?.querySelector<HTMLInputElement>('.tab-rename input');
      input?.focus();
      input?.select();
    }
  }

  function finishRename(restoreFocus = true) {
    const id = renamingTab;
    // Removing the input after Enter or Escape can also fire blur. A completed
    // or cancelled rename must not be saved again by that trailing event.
    if (!id) return;
    if (renameValue.trim()) tabs = tabs.map((tab) => tab.id === id ? { ...tab, title: renameValue.trim() } : tab);
    renamingTab = '';
    if (restoreFocus) focusTabLabel(id);
  }

  function handleRenameKeydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.key !== 'Enter' && event.key !== 'Escape') return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === 'Enter') finishRename();
    else {
      const id = renamingTab;
      renamingTab = '';
      focusTabLabel(id);
    }
  }

  function focusTabLabel(id: string) {
    void tick().then(() => tabBar?.querySelector<HTMLButtonElement>(`[data-tab-id="${CSS.escape(id)}"]`)?.focus());
  }

  function startTabDrag(id: string, x: number, y: number) {
    draggingTab = id;
    updateTabDrag(x, y);
    tabDragFrame = requestAnimationFrame(scrollTabDrag);
  }

  function updateTabDrag(x: number, y: number) {
    tabDragPointer = { x, y };
    const bar = tabBar.getBoundingClientRect();
    if (y < bar.top - 16 || y > bar.bottom + 16) { tabDrop = null; return; }
    const bounds = Array.from(tabBar.querySelectorAll<HTMLElement>('[data-tab-chip-id]'))
      .filter((element) => element.dataset.tabChipId !== draggingTab)
      .map((element) => {
        const { left, right } = element.getBoundingClientRect();
        return { id: element.dataset.tabChipId!, start: left, end: right };
      });
    tabDrop = listDropTarget(bounds, x);
  }

  function scrollTabDrag() {
    if (!draggingTab || !tabDragPointer) return;
    const { x, y } = tabDragPointer;
    const bar = tabBar.getBoundingClientRect();
    if (y >= bar.top - 16 && y <= bar.bottom + 16) {
      const delta = listScrollDelta(bar.left, bar.right, x);
      if (delta) { tabBar.scrollLeft += delta; updateTabDrag(x, y); }
    }
    tabDragFrame = requestAnimationFrame(scrollTabDrag);
  }

  function finishTabDrag(cancelled: boolean) {
    if (tabDragFrame) cancelAnimationFrame(tabDragFrame);
    tabDragFrame = 0;
    if (!cancelled) {
      captureActive();
      tabs = reorderList(tabs, draggingTab, tabDrop);
    }
    draggingTab = '';
    tabDrop = null;
    tabDragPointer = null;
  }

  function reorderTabWithKeyboard(id: string, direction: number) {
    const neighbor = tabs[tabs.findIndex((tab) => tab.id === id) + direction];
    if (!neighbor) return;
    captureActive();
    tabs = reorderList(tabs, id, { id: neighbor.id, side: direction < 0 ? 'before' : 'after' });
    focusTabLabel(id);
    flash('Tab order updated.');
  }

  function emptyConnection(): ConnectionInput {
    return {
      id: null,
      name: '',
      host: 'localhost',
      port: 5432,
      database: '',
      username: '',
      password: '',
      ssl: false,
      options: connectionOptions()
    };
  }

  function selectSidebar(tab: SidebarTab) {
    if (sidebar === tab) {
      sidebarOpen = !sidebarOpen;
    } else {
      sidebar = tab;
      sidebarOpen = true;
    }
  }

  async function openAppUpdates() {
    sidebar = 'settings';
    sidebarOpen = true;
    await tick();
    if (sidebarElement) sidebarElement.scrollTop = 0;
  }

  function groupSchemaItems(items: SchemaItem[], search: string): SchemaGroup[] {
    const term = search.trim().toLowerCase();
    const filtered = term
      ? items.filter(
          (item) =>
            item.schema.toLowerCase().includes(term) ||
            item.name.toLowerCase().includes(term) ||
            item.columns.some(
              (column) => column.name.toLowerCase().includes(term) || column.typeName.toLowerCase().includes(term)
            )
        )
      : items;
    const groups = new Map<string, SchemaItem[]>();
    for (const item of filtered) groups.set(item.schema, [...(groups.get(item.schema) ?? []), item]);
    return Array.from(groups, ([name, groupItems]) => ({ name, items: groupItems }));
  }

  function captureWorkspaceSearch(event: KeyboardEvent) {
    // Monaco consumes Cmd/Ctrl+K as the first key of an editor chord.
    if (event.defaultPrevented || !workspaceSearchShortcut(event)) return;
    handleKeydown(event);
    if (event.defaultPrevented) event.stopPropagation();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.defaultPrevented || !workspaceLoaded || closingWorkspace) return;
    if (tabCloseTask) { event.preventDefault(); return; }
    if (picker || explainSession || explainOpening || diagnosticsOpen || reconnectReview || queryPreparing || connectionMutation) return;
    const fontAction = fontSizeShortcut(event);
    if (fontAction) {
      event.preventDefault();
      editorFontSize = resizeEditorFont(editorFontSize, fontAction);
      try { localStorage.setItem(EDITOR_FONT_STORAGE_KEY, String(editorFontSize)); } catch { /* Keep the current session's preference. */ }
      return;
    }
    if (showConnectionModal || showUpdatePreview || discardRunConfirmation || timingDetails) return;
    const command = event.metaKey || event.ctrlKey;
    if (!command) return;
    if (workspaceSearchShortcut(event)) {
      event.preventDefault();
      picker = 'commands';
      return;
    }
    const searchAction = editorSearchShortcut(event);
    if (searchAction) {
      event.preventDefault();
      editorRef?.find(searchAction === 'replace');
      return;
    }
    if (event.key === 'Enter') {
      const target = event.target;
      if (target instanceof Element && target.closest('.find-widget')) return;
      if (target instanceof Element && target.closest('input, textarea, [contenteditable="true"]') && !target.closest('.monaco-editor')) return;
      event.preventDefault();
      void runQuery();
    } else if (event.key.toLowerCase() === 'b') {
      event.preventDefault();
      sidebarOpen = !sidebarOpen;
    } else if (event.key.toLowerCase() === 'l' && event.shiftKey) {
      event.preventDefault();
      clearResults();
    } else if (event.key.toLowerCase() === 't') {
      event.preventDefault();
      newQuery();
    } else if (event.key.toLowerCase() === 'w') {
      event.preventDefault();
      closeTab(activeTab);
    }
  }

  function preventWorkspaceSelection(event: Event) {
    // WebKit also sends selectstart from the document body for Edit → Select All.
    const target = event.target;
    const element = target instanceof Element ? target : target instanceof Node ? target.parentElement : null;
    if (!element?.closest('input, textarea, [contenteditable]:not([contenteditable="false"]), pre, [data-selectable-text]')) {
      event.preventDefault();
    }
  }

  function setTheme(next: ThemePreference) {
    theme = next;
    try { localStorage.setItem(THEME_STORAGE_KEY, next); } catch { /* Session preference still applies. */ }
  }

  function setVimMode(enabled: boolean) {
    vimModeEnabled = enabled;
    try { localStorage.setItem(VIM_MODE_STORAGE_KEY, String(enabled)); } catch { /* Session preference still applies. */ }
  }

  function saveConnectionAppearance(id: string, { color, environment }: ConnectionAppearance) {
    appearances = { ...appearances, [id]: { color, environment: environment.trim().slice(0, 16) } };
    try { localStorage.setItem(APPEARANCE_KEY, JSON.stringify(appearances)); } catch { /* Session preference still applies. */ }
  }

  function setConnectionScope(scope: ConnectionScope) {
    if (operation || connectionMutation || queryPreparing) return;
    connectionScope = scope;
    // A preference change never silently reassigns existing tabs.
    if (scope === 'global' && !focusedConnectionLocked && tabConnectionId) rememberGlobalConnection(tabConnectionId);
    try { localStorage.setItem(CONNECTION_SCOPE_KEY, scope); } catch { /* Session preference still applies. */ }
  }

  function rememberGlobalConnection(id: string) {
    globalConnectionId = id;
    try { localStorage.setItem(GLOBAL_CONNECTION_KEY, id); } catch { /* Session preference still applies. */ }
  }

  function toggleConnectionLock(id: string) {
    if (operation || connectionMutation || queryPreparing) return;
    const tab = id === activeTab ? snapshotActive() : tabs.find(tab => tab.id === id);
    if (tab && (tab.connectionId || tab.connectionLocked)) patchTab(id, { connectionLocked: !tab.connectionLocked });
  }

  function confirmConnectionChange(targets: WorkspaceTab[], id: string, alreadyReviewedTab?: string): Promise<boolean> {
    const edits = targets.filter(tab => tab.connectionId !== id && tab.id !== alreadyReviewedTab)
      .reduce((count, tab) => count + tab.pendingChanges.length, 0);
    if (!edits) return Promise.resolve(true);
    return new Promise(resolve => {
      discardRunConfirmation = { changeCount: edits, connectionName: connections.find(connection => connection.id === id)?.name ?? connectionForm.name ?? 'another connection', globalSelection: connectionScope === 'global', resolve };
    });
  }

  function applyConnectionSelection(id: string, targetIds: string[]) {
    for (const tab of tabs.filter(tab => targetIds.includes(tab.id) && tab.connectionId !== id && !tab.connectionLocked)) {
      patchTab(tab.id, switchTabConnection(tab, id));
    }
    if (connectionScope === 'global' && targetIds.length) rememberGlobalConnection(id);
  }

  function formatSql() {
    if (!workspaceLoaded || tabKind !== 'query') return;
    try { editorRef?.formatSql(); } catch (error) { flash(`Could not format SQL: ${errorMessage(error)}`, true); }
  }

  function selectPickerItem(id: string) {
    const mode = picker;
    picker = null;
    if (id.startsWith('connection:')) {
      void activateConnection(id.slice('connection:'.length));
      return;
    }
    if (mode === 'connections') {
      if (id === 'command:disconnect' && tabConnectionId) void disconnectConnection(tabConnectionId);
      return;
    }
    if (id.startsWith('tab:')) { switchTab(id.slice(4)); return; }
    if (id.startsWith('closed-tab:')) { reopenTab(id.slice(11)); return; }
    if (id.startsWith('history:')) {
      const entry = pickerHistory.entries.find(entry => `history:${entry.id}` === id);
      if (entry) void openHistory(entry);
      return;
    }
    if (id.startsWith('table:')) {
      const item = schemaItems.find(t => `${t.schema}.${t.name}` === id.slice(6));
      if (item) newQuery(`select * from ${quoteIdent(item.schema)}.${quoteIdent(item.name)};`, tabConnectionId);
      return;
    }
    const command = id.slice(8);
    if (command === 'new') newQuery();
    else if (command === 'connections' && !focusedConnectionLocked) picker = 'connections';
    else if (command === 'find') editorRef?.find();
    else if (command === 'format') formatSql();
    else if (command === 'query-timing') timingDetails = statusTiming;
    else if (command === 'schema' || command === 'history' || command === 'activity' || command === 'tabs' || command === 'settings') { sidebar = command; sidebarOpen = true; }
  }

  async function loadSettings() {
    try {
      settings = await api.getSettings();
      poolSizeInput = settings.poolSize;
    } catch (error) {
      flash(`Settings unavailable: ${errorMessage(error)}`);
    }
  }

  async function loadConnections() {
    try {
      const records = await withTimeout(api.listConnections(), 5000, 'Loading connections timed out.');
      setConnections(orderConnections(records, connectionOrder));
    } catch (error) {
      flash(`Connections unavailable: ${errorMessage(error)}`);
    }
  }

  function setConnections(next: ConnectionRecord[]) {
    connections = next;
    const order = next.map((connection) => connection.id);
    if (order.length === connectionOrder.length && order.every((id, index) => id === connectionOrder[index])) return;
    connectionOrder = order;
    try { localStorage.setItem(CONNECTION_ORDER_KEY, JSON.stringify(order)); } catch { /* Session order still applies. */ }
  }

  function rememberConnection(id: string) {
    lastConnectionId = id;
    try { localStorage.setItem(LAST_CONNECTION_KEY, id); } catch { /* Keep the last choice in memory. */ }
  }

  function resolveReconnectReview(connection: ConnectionRecord | null) {
    const review = reconnectReview;
    reconnectReview = null;
    review?.resolve(connection);
  }

  function focusConnectionRow(id: string) {
    void tick().then(() => connectionList?.querySelector<HTMLButtonElement>(`[data-connection-id="${CSS.escape(id)}"]`)?.focus());
  }

  function startConnectionDrag(id: string, x: number, y: number) {
    connectionDrag = { id, x, y };
    updateConnectionDrag(x, y);
    connectionDragFrame = requestAnimationFrame(scrollConnectionDrag);
  }

  function updateConnectionDrag(x: number, y: number) {
    const drag = connectionDrag;
    if (!connectionList || !drag) return;
    drag.x = x;
    drag.y = y;
    const { left, right, top, bottom } = connectionList.getBoundingClientRect();
    if (x < left || x > right || y < top || y > bottom) {
      connectionDrop = null;
      return;
    }
    const bounds = Array.from(connectionList.querySelectorAll<HTMLLIElement>('[data-connection-row-id]'))
      .filter((row) => row.dataset.connectionRowId !== drag.id)
      .map((row) => {
        const { top, bottom } = row.getBoundingClientRect();
        return { id: row.dataset.connectionRowId!, start: top, end: bottom };
      });
    connectionDrop = listDropTarget(bounds, y);
  }

  function scrollConnectionDrag() {
    if (!connectionList || !connectionDrag) return;
    if (connectionDrop) {
      const { top, bottom } = connectionList.getBoundingClientRect();
      const { x, y } = connectionDrag;
      const previousScroll = connectionList.scrollTop;
      connectionList.scrollTop += listScrollDelta(top, bottom, y);
      if (connectionList.scrollTop !== previousScroll) updateConnectionDrag(x, y);
    }
    connectionDragFrame = requestAnimationFrame(scrollConnectionDrag);
  }

  function finishConnectionDrag(cancelled: boolean) {
    cancelAnimationFrame(connectionDragFrame);
    if (!cancelled && connectionDrag) reorderConnection(connectionDrag.id, connectionDrop);
    connectionDrag = null;
    connectionDrop = null;
  }

  function reorderConnection(id: string, drop: ListDrop | null) {
    const next = reorderList(connections, id, drop);
    if (next === connections) return;
    setConnections(next);
    focusConnectionRow(id);
  }

  function reorderConnectionWithKeyboard(id: string, direction: number) {
    const neighbor = connections[connections.findIndex((connection) => connection.id === id) + direction];
    if (!neighbor) return;
    reorderConnection(id, { id: neighbor.id, side: direction < 0 ? 'before' : 'after' });
    flash('Connection order updated.');
  }

  async function loadDraft() {
    try {
      const restored = restoreWorkspace(await api.loadDraft('workspace.v1') ?? '');
      if (restored) {
        tabs = restored.tabs;
        archivedTabs = restored.archivedTabs;
        displayTab(tabs.find((tab) => tab.id === restored.activeId)!);
        return;
      }
      const draft = await api.loadDraft('playground');
      if (draft !== null) sql = draft;
    } catch {
      // Browser-only dev mode has no Tauri IPC; keep seeded SQL.
    }
    tabConnectionId = connections.find((connection) => connection.isActive)?.id ?? null;
    captureActive();
  }

  function scheduleDraftSave(_sql: string, _activeId: string, _connectionId: string | null, _tabs: WorkspaceTab[], _archivedTabs: ArchivedTab[]) {
    if (tabCloseTask) return;
    if (draftTimer) clearTimeout(draftTimer);
    draftRevision++;
    draftStatus = 'pending';
    draftTimer = setTimeout(() => void persistDraftNow().catch(() => {}), 1200);
  }

  async function persistDraftNow() {
    if (!mounted || !workspaceLoaded) return;
    if (tabCloseTask) await tabCloseTask;
    if (draftTimer) { clearTimeout(draftTimer); draftTimer = null; }
    const revision = ++draftRevision;
    draftStatus = 'saving';
    const captured = snapshotActive();
    const workspace = tabs.map((tab) => tab.id === activeTab ? captured : tab);
    try {
      await writeWorkspaceDraft(serializeWorkspace(workspace, activeTab, archivedTabs));
      if (revision === draftRevision) draftStatus = 'saved';
    } catch (error) {
      if (revision === draftRevision) draftStatus = 'error';
      flash(`Draft save failed: ${errorMessage(error)}`, true);
      throw error;
    }
  }

  function beginOperation(kind: Operation['kind'], connectionId: string): Operation {
    const next = { token: crypto.randomUUID(), tabId: activeTab, connectionId, kind, startedAt: Date.now(), cancelling: false };
    operation = next;
    nowMs = next.startedAt;
    return next;
  }

  function finishOperation(op: Operation) {
    if (operation?.token !== op.token) return;
    operation = null;
    void refreshHistory();
    void loadConnectionStats();
    void refreshTxStatus();
  }

  function isCurrentOperation(op: Operation) {
    return operation?.token === op.token;
  }

  function makeSession(data: QueryResult, execution: ExecutionTarget, connectionId: string, snapshot = false): ResultSession {
    return { data, execution, connectionId, connectionName: connections.find((connection) => connection.id === connectionId)?.name ?? 'Unavailable connection', executedAt: new Date().toISOString(), historyEntryIds: data.historyEntryIds, snapshot };
  }

  function resultPatch(session: ResultSession | null, error: AppError | null = null): Partial<WorkspaceTab> {
    return { session, originalRows: session ? cloneRows(session.data.rows) : [], pendingChanges: [], editMode: false, error };
  }

  function confirmDiscardForRun(): Promise<boolean> {
    return new Promise((resolve) => {
      discardRunConfirmation = { changeCount: pendingChanges.length, resolve };
    });
  }

  function resolveDiscardForRun(confirmed: boolean) {
    const confirmation = discardRunConfirmation;
    discardRunConfirmation = null;
    confirmation?.resolve(confirmed);
  }

  async function openExplain() {
    if (!workspaceLoaded || closingWorkspace || operation || connectionMutation || explainSession || explainOpening || tabKind !== 'query') return;
    if (!activeConnection) { sidebar = 'connections'; sidebarOpen = true; flash('Choose a connection for this query.'); return; }
    const tabId = activeTab;
    const connectionId = activeConnection.id;
    const connectionName = activeConnection.name;
    const editor = editorRef;
    const fallback = sql.trim();
    explainOpening = true;
    try {
      await explainEventsReady;
      if (explainEventsError) throw new Error(`Explain progress is unavailable: ${explainEventsError}`);
      const target = editor ? await editor.getExecutionTarget(api.splitSql) : { sql: fallback };
      if (!target.sql.trim()) throw new Error('No query to explain.');
      if (closingWorkspace || operation || connectionMutation || tabId !== activeTab || connectionId !== tabConnectionId) return;
      explainWorkflow.reset();
      explainSession = { query: target.sql, tabId, connectionId, connectionName };
    } catch (error) { flash(errorMessage(error), true); }
    finally { explainOpening = false; }
  }

  async function runExplain(options: ExplainOptions) {
    const session = explainSession;
    if (!session || operation || connectionMutation || closingWorkspace) return;
    statusTiming = null;
    statusFlash = 'Running explain…';
    await explainWorkflow.run({ ...options, query: session.query, connectionId: session.connectionId });
    updateExplainStatus();
  }

  async function shareExplain(title: string, comments: string) {
    if (!explainSession || operation || connectionMutation || closingWorkspace) return;
    statusFlash = 'Sharing explain plan…';
    await explainWorkflow.retryShare(explainSession.connectionId, title, comments);
    updateExplainStatus();
  }

  function updateExplainStatus() {
    const { result, error } = explainState;
    statusFlash = error || (result?.shareError ? 'Explain ready. Sharing did not complete.' : result?.url ? 'Explain ready. Shared on Dalibo.' : 'Explain ready.');
  }

  function closeExplain() {
    if (explainState.busy) return;
    explainSession = null;
    explainWorkflow.reset();
  }

  async function runQuery() {
    if (closingWorkspace || !workspaceLoaded || explainOpening || explainSession) return;
    if (discardRunConfirmation || reconnectReview || queryPreparing) return;
    if (connectionMutation) { flash(connectionMutation); return; }
    if (operation) {
      await cancelQuery();
      return;
    }
    if (tabKind !== 'query') {
      flash('Open this SQL in a query tab to run it.');
      return;
    }
    const tabId = activeTab;
    const initialAssociation = tabConnectionId;
    const draftSql = sql;
    let connectionId = activeConnection?.id ?? null;
    let reconnecting: ConnectionRecord | null = null;
    let reviewedTarget: ExecutionTarget | null = null;
    const editor = editorRef;
    const stillCurrent = (association = initialAssociation) => activeTab === tabId && tabConnectionId === association && sql === draftSql && !operation && !closingWorkspace;
    queryPreparing = true;
    try {
      if (pendingChanges.length) {
        if (!await confirmDiscardForRun()) return;
        await tick();
        if (!stillCurrent()) return;
      }
      if (!connectionId) {
        const candidate = resumeConnection(connections, initialAssociation, connectionScope === 'global' ? globalConnectionId ?? lastConnectionId : lastConnectionId, focusedConnectionLocked);
        if (!candidate) {
          if (focusedConnectionLocked) throw new Error('This tab’s locked connection is unavailable. Unlock it from the tab menu to choose another.');
          picker = 'connections';
          return;
        }
        reviewedTarget = editor ? await editor.getExecutionTarget(api.splitSql) : { sql: draftSql.trim(), start: 0, end: draftSql.length };
        if (!reviewedTarget.sql.trim()) throw new Error('No query to execute.');
        if (!stillCurrent()) return;
        const reviewed = await new Promise<ConnectionRecord | null>(resolve => {
          reconnectReview = { connectionId: candidate.id, target: reviewedTarget!, resolve };
        });
        if (!reviewed || !stillCurrent()) return;
        if (focusedConnectionLocked && reviewed.id !== initialAssociation) throw new Error('Unlock this tab before changing its connection.');
        const current = connections.find(connection => connection.id === reviewed.id);
        if (!current || !sameConnectionSettings(current, reviewed)) throw new Error('Connection settings changed. Run again to review the current connection.');
        captureActive();
        // Reconnecting the same assigned database is not a global selection.
        const targets = reviewed.id === initialAssociation ? [] : connectionTargets(tabs, tabId, connectionScope);
        if (!await confirmConnectionChange(targets, reviewed.id, tabId) || !stillCurrent()) return;
        connectionMutation = 'Connecting…';
        reconnecting = reviewed;
        const connected = await api.activateConnection(reviewed.id);
        reconnecting = null;
        if (!sameConnectionSettings(connected, reviewed)) throw new Error('Connection settings changed. Run again to review the current connection.');
        if (!stillCurrent()) return;
        connectedIds = new Set([...connectedIds, connected.id]);
        applyConnectionSelection(connected.id, targets.map(tab => tab.id));
        rememberConnection(connected.id);
        connectionId = connected.id;
        await loadConnections();
        if (!stillCurrent(connectionId)) return;
      }
    } catch (error) {
      if (activeTab === tabId && !closingWorkspace) {
        setStatus('');
        const failure = normalizeAppError(error);
        patchTab(tabId, { error: reconnecting ? connectBeforeRunError(failure, reconnecting.name) : failure });
      }
      return;
    } finally { queryPreparing = false; connectionMutation = null; }
    if (!connectionId) return;
    const op = beginOperation('query', connectionId);
    const timingStart = performance.now();
    let target: ExecutionTarget = { sql: sql.trim(), start: 0, end: sql.length };
    setStatus('');
    try {
      target = reviewedTarget ?? (editor ? await editor.getExecutionTarget(api.splitSql) : target);
      if (!target.sql.trim()) throw new Error('No query to execute.');
      if (op.cancelling) { setStatus('Query cancelled before execution.'); return; }
      patchTab(op.tabId, { error: null });
      void loadConnectionStats();
      const invokeStart = performance.now();
      rememberConnection(op.connectionId);
      const next = await connectionTester.query(op.connectionId, () => api.executeQuery(target.sql, op.token, 0, PAGE_SIZE, op.connectionId));
      const invokeEnd = performance.now();
      if (!isCurrentOperation(op)) return;
      const session = makeSession(next, target, op.connectionId);
      patchTab(op.tabId, resultPatch(session));
      await tick();
      const uiEnd = performance.now();
      if (!isCurrentOperation(op)) return;
      const summary = next.commitFeedback
        ? `${commitFeedbackLabel(next.commitFeedback)} · ${next.durationMs} ms`
        : `Loaded ${next.rowCount}${next.hasMore ? '+' : ''} ${next.rowCount === 1 ? 'row' : 'rows'} in ${next.durationMs} ms.`;
      setStatus(`${op.cancelling ? 'Query completed before cancellation. ' : ''}${summary}`, {
        result: next, connectionName: session.connectionName, executedAt: session.executedAt, batch: false,
        client: { targetMs: invokeStart - timingStart, invokeMs: invokeEnd - invokeStart, uiMs: uiEnd - invokeEnd, totalMs: uiEnd - timingStart }
      });
    } catch (error) {
      if (!isCurrentOperation(op)) return;
      const normalized = normalizeAppError(error);
      if (normalized.code === '57014') {
        setStatus('Query cancelled.');
      } else if (normalized.connectionFailure === 'connect') {
        const connectionName = connections.find(connection => connection.id === op.connectionId)?.name ?? 'the database';
        patchTab(op.tabId, { error: connectBeforeRunError(normalized, connectionName) });
      } else {
        patchTab(op.tabId, resultPatch(null, normalized));
        if (normalized.position && activeTab === op.tabId) editorRef?.markError(target.start + normalized.position - 1, normalized.message);
      }
    } finally {
      finishOperation(op);
    }
  }

  async function cancelQuery() {
    const op = operation;
    if (!op || op.cancelling) return;
    if (op.kind === 'explain') { await explainWorkflow.cancel(); return; }
    op.cancelling = true;
    operation = op;
    try {
      await api.cancelQuery(op.token);
    } catch (error) {
      if (isCurrentOperation(op)) { op.cancelling = false; operation = op; flash(`Cancellation failed: ${errorMessage(error)}`, true); }
    }
  }

  async function loadMoreRows() {
    if (!result || !result.autoLimited || !result.hasMore || !resultSession || !resultUsable || txStatus.active || operation || connectionMutation || !tabConnectionId) return;
    if (pendingChanges.length) { flash('Apply or discard staged edits before loading more rows.'); return; }
    const session = { ...resultSession, data: result };
    const op = beginOperation('load', tabConnectionId);
    setStatus('');
    try {
      const invokeStart = performance.now();
      const next = await connectionTester.query(op.connectionId, () => api.executeQuery(session.execution.sql, op.token, session.data.rows.length, PAGE_SIZE, session.connectionId!, true));
      const invokeEnd = performance.now();
      if (!isCurrentOperation(op)) return;
      const merged = {
        ...next,
        rows: [...session.data.rows, ...next.rows],
        rowCount: session.data.rows.length + next.rows.length
      };
      patchTab(op.tabId, resultPatch({ ...session, data: merged }));
      await tick();
      const uiEnd = performance.now();
      if (!isCurrentOperation(op)) return;
      statusFlash = `Loaded ${merged.rowCount}${next.hasMore ? '+' : ''} rows.`;
      statusTiming = {
        result: next, connectionName: session.connectionName, executedAt: new Date().toISOString(), batch: true,
        client: { targetMs: 0, invokeMs: invokeEnd - invokeStart, uiMs: uiEnd - invokeEnd, totalMs: uiEnd - invokeStart }
      };
    } catch (error) {
      if (isCurrentOperation(op)) flash(normalizeAppError(error).code === '57014' ? 'Loading cancelled.' : `Load more failed: ${errorMessage(error)}`, normalizeAppError(error).code !== '57014');
    } finally {
      finishOperation(op);
    }
  }

  function clearResults() {
    if (operation?.tabId === activeTab || connectionMutation) return;
    if (pendingChanges.length && !confirm('Discard staged cell edits and clear results?')) return;
    patchTab(activeTab, resultPatch(null));
    editorRef?.clearHighlights();
    flash('Results cleared.');
  }

  function openNewConnection() {
    if (operation || connectionMutation) return;
    connectionForm = emptyConnection();
    connectionAppearanceForm = connectionAppearanceDraft({});
    modalFeedback = '';
    showConnectionModal = true;
  }

  function editConnection(connection: ConnectionRecord) {
    if (operation || connectionMutation) return;
    connectionForm = { ...connection, options: connectionOptions(connection.options) };
    connectionAppearanceForm = connectionAppearanceDraft(appearances, connection.id);
    modalFeedback = '';
    showConnectionModal = true;
  }

  async function copyConnection(connection: ConnectionRecord) {
    try {
      await copyText(() => serializeConnectionReference(connection), 'Copy connection reference', connection.name);
      flash(`${connection.name} copied. Paste in the connection list to duplicate.`);
    } catch (error) {
      flash(`Could not copy connection: ${errorMessage(error)}`, true);
    }
  }

  async function pasteConnection(text?: string) {
    if (operation || connectionMutation || closingWorkspace || showConnectionModal) return;
    try {
      const reference = text ?? await readClipboardText();
      const id = parseConnectionReference(reference);
      if (!id) { flash('Copy a saved connection first, then paste in the connection list.'); return; }
      await duplicateConnection(id);
    } catch (error) {
      flash(`Could not paste connection: ${errorMessage(error)}`, true);
    }
  }

  async function duplicateConnection(id: string) {
    if (operation || connectionMutation || closingWorkspace || showConnectionModal) return;
    connectionMutation = 'Duplicating connection…';
    try {
      const saved = await api.duplicateConnection(id);
      const appearance = appearances[id];
      if (appearance) saveConnectionAppearance(saved.id, appearance);
      setConnections(reorderList([...connections, saved], saved.id, { id, side: 'after' }));
      focusConnectionRow(saved.id);
      flash(`${saved.name} created.`);
    } catch (error) {
      flash(`Could not duplicate connection: ${errorMessage(error)}`, true);
    } finally {
      connectionMutation = null;
    }
  }

  async function testConnection() {
    testingConnection = true;
    modalFeedback = 'Testing connection...';
    try {
      await probeConnection(connectionForm);
      modalFeedback = 'Connection test succeeded.';
    } catch (error) {
      modalFeedback = errorMessage(error);
    } finally {
      testingConnection = false;
    }
  }

  async function saveConnection() {
    if (operation || connectionMutation || queryPreparing) return;
    const existing = connections.find(connection => connection.id === connectionForm.id);
    if (existing && sameConnectionSettings(existing, connectionForm)) {
      saveConnectionAppearance(existing.id, connectionAppearanceForm);
      showConnectionModal = false;
      flash(`${existing.name} appearance saved.`);
      return;
    }
    const initiatingTab = activeTab;
    captureActive();
    const targets = connectionTargets(tabs, initiatingTab, connectionScope);
    const previousId = connectionForm.id;
    if (previousId && tabs.some(tab => tab.connectionId === previousId && (tab.id === activeTab ? pendingChanges : tab.pendingChanges).length)
      && !confirm('Changing connection settings clears affected results and staged edits. Discard these edits and save?')) return;
    connectionMutation = 'Saving connection…';
    savingConnection = true;
    try {
      if (!await confirmConnectionChange(targets.filter(tab => tab.connectionId !== previousId), previousId ?? 'new-connection')) return;
      // Keep the lock until the actual write finishes; timing out an IPC promise
      // would leave its uncancelled write racing a subsequent query.
      const saved = await api.saveConnection(connectionForm);
      saveConnectionAppearance(saved.id, connectionAppearanceForm);
      connectionTester.invalidate(saved.id);
      await loadConnections();
      // A reused ID may now point at a different database. Never apply or replay
      // results obtained using its previous configuration.
      if (previousId) {
        captureActive();
        for (const tab of tabs.filter((item) => item.connectionId === previousId)) {
          patchTab(tab.id, { session: null, originalRows: [], pendingChanges: [], editMode: false, error: null });
        }
      }
      applyConnectionSelection(saved.id, targets.map(tab => tab.id));
      rememberConnection(saved.id);
      connectedIds = new Set([...connectedIds, saved.id]);
      showConnectionModal = false;
      flash(`${saved.name} saved and ready.`);
      void refreshTxStatus();
      void testSavedConnection(saved);
    } catch (error) {
      modalFeedback = errorMessage(error);
    } finally {
      savingConnection = false;
      connectionMutation = null;
    }
  }

  async function activateConnection(id: string) {
    if (operation || connectionMutation || queryPreparing) { flash('Wait for the current operation before switching connections.'); return; }
    if (focusedConnectionLocked) { flash('Connection locked. Right-click the focused tab to unlock.'); return; }
    const connection = connections.find((candidate) => candidate.id === id);
    if (!connection) return;
    captureActive();
    const targets = connectionTargets(tabs, activeTab, connectionScope);
    // A click on the current connection is a retry, not a switch: keep
    // its result set, staged edits, pool and manual transaction intact.
    if (activeConnection?.id === id && targets.every(tab => tab.connectionId === id)) { void testSavedConnection(connection); return; }
    connectionMutation = 'Switching connection…';
    try {
      if (!await confirmConnectionChange(targets, id)) return;
      const active = await api.activateConnection(id);
      rememberConnection(id);
      connectedIds = new Set([...connectedIds, id]);
      applyConnectionSelection(id, targets.map(tab => tab.id));
      await loadConnections();
      poolStats = null;
      flash(`Using ${active.name}.`);
      void refreshTxStatus();
      void loadConnectionStats();
      void testSavedConnection(active);
    } catch (error) {
      flash(errorMessage(error), true);
    } finally {
      connectionMutation = null;
    }
  }

  async function probeConnection(connection: ConnectionInput, automatic = false) {
    const title = automatic ? 'Check connection automatically' : 'Check connection';
    const finish = startActivity('connection', title, connectionLabels[connection.id ?? ''] ?? (connection.name || 'Unsaved connection'));
    try {
      await withTimeout(api.testConnection(connection), connectionTestTimeout(connection.options), 'Connection test timed out.');
      finish('completed');
    } catch (error) {
      finish('failed', errorMessage(error));
      throw error;
    }
  }

  function testSavedConnection(connection: ConnectionRecord, automatic = false) {
    const id = connection.id;
    // A routine check on a healthy connection shouldn't re-reflect an empty database.
    const retrySchema = !automatic || connectionTests[id]?.status !== 'success';
    return connectionTester.test(id, () => probeConnection(connection, automatic),
      retrySchema ? () => retryMissingSchema(id) : undefined);
  }

  function testCurrentConnection() {
    if (statusConnection && !connectionMutation && !operation) void testSavedConnection(statusConnection);
  }

  async function disconnectConnection(id: string) {
    if (operation || connectionMutation) return;
    captureActive();
    const affected = tabs.filter(tab => tab.connectionId === id);
    const count = affected.reduce((total, tab) => total + tab.pendingChanges.length, 0);
    if (count && !confirm(`Disconnecting will discard ${count} staged edit${count === 1 ? '' : 's'} across this connection’s tabs. Continue?`)) return;
    connectionMutation = 'Disconnecting…';
    try {
      await api.disconnectConnection(id);
      connectionTester.invalidate(id);
      connectedIds = new Set([...connectedIds].filter(candidate => candidate !== id));
      for (const tab of affected) patchTab(tab.id, {
        connectionId: tab.connectionLocked ? tab.connectionId : null, editMode: false, pendingChanges: [],
        session: tab.session ? { ...tab.session, data: { ...tab.session.data, rows: cloneRows(tab.originalRows) } } : null
      });
      await loadConnections();
      void refreshTxStatus();
      void loadConnectionStats();
      flash('Disconnected');
    } catch (error) {
      flash(errorMessage(error), true);
    } finally {
      connectionMutation = null;
    }
  }

  async function deleteConnection(id: string) {
    if (operation || connectionMutation) return;
    if (!confirm('Delete this connection?')) return;
    connectionMutation = 'Deleting connection…';
    try {
      await api.deleteConnection(id);
      connectionTester.invalidate(id);
      await loadConnections();
      connectedIds = new Set([...connectedIds].filter(candidate => candidate !== id));
      captureActive();
      for (const tab of tabs.filter(tab => tab.connectionId === id && !tab.connectionLocked)) patchTab(tab.id, { connectionId: null });
      if (tabConnectionId === id || !tabConnectionId) poolStats = null;
      void refreshTxStatus();
      if (connections.some((connection) => connection.isActive)) {
        void refreshSchema();
      } else {
        schemaItems = [];
      }
      flash('Connection deleted.');
    } catch (error) {
      flash(errorMessage(error));
    } finally {
      connectionMutation = null;
    }
  }

  async function setCommitMode(mode: CommitMode) {
    if (!workspaceLoaded || closingWorkspace || operation || connectionMutation || mode === settings.commitMode) return;
    connectionMutation = 'Saving commit mode…';
    try {
      const saved = await api.setCommitMode(mode);
      settings = { ...settings, commitMode: saved };
      flash(`${commitModeLabels[saved]} selected.`);
    } catch (error) {
      flash(`Could not save commit mode. ${errorMessage(error)}`, true);
    } finally {
      connectionMutation = null;
    }
  }

  async function savePoolSize() {
    if (operation || connectionMutation) return;
    connectionMutation = 'Updating connection pool…';
    try {
      const normalized = await api.setPoolSize(poolSizeInput);
      settings = { ...settings, poolSize: normalized };
      poolSizeInput = normalized;
      void loadConnectionStats();
      flash(`Pool size saved: ${normalized}.`);
    } catch (error) {
      flash(errorMessage(error));
    } finally {
      connectionMutation = null;
    }
  }

  async function setSupermaven(enabled: boolean) {
    if (supermavenBusy) return;
    supermavenBusy = true;
    try {
      supermaven = await api.setSupermavenEnabled(enabled);
      settings = { ...settings, supermavenEnabled: enabled };
    } catch (error) {
      flash(errorMessage(error), true);
      await checkSupermaven();
    } finally {
      supermavenBusy = false;
    }
  }

  async function checkSupermaven() {
    try {
      supermaven = await api.supermavenStatus();
    } catch {
      supermaven = { enabled: false, state: 'error', message: 'Supermaven status is unavailable.' };
    }
  }

  async function supermavenAccount(action: 'activate' | 'free') {
    try {
      if (action === 'activate') await api.supermavenOpenActivation();
      else await api.supermavenUseFree();
      await checkSupermaven();
    } catch (error) { flash(errorMessage(error), true); }
  }

  async function loadConnectionStats(connectionId: string | null = tabConnectionId) {
    const request = ++poolStatsRequest;
    if (!connectionId || !connections.some((connection) => connection.id === connectionId)) {
      poolStats = null;
      poolStatsConnectionId = null;
      return;
    }
    const next = await api.connectionStats(connectionId).catch(() => null);
    if (request === poolStatsRequest && connectionId === tabConnectionId) {
      poolStats = next;
      poolStatsConnectionId = connectionId;
    }
  }

  function updateTransactionStatus(next: TxStatus) {
    if (next.completion) {
      const liveIds = new Set(tabs.map(tab => (tab.id === activeTab ? result : tab.session?.data)?.commitFeedback?.transactionId));
      txCompletions = new Map([...txCompletions].filter(([id]) => liveIds.has(id))).set(next.completion.id, next.completion);
    }
    txStatus = next;
    txStatusKnown = true;
  }

  async function refreshTxStatus() {
    const request = ++txStatusRequest;
    txStatusKnown = false;
    try {
      const next = await api.txStatus();
      if (request === txStatusRequest) updateTransactionStatus(next);
    } catch {
      // Unknown is not idle. Keep the last known transaction visible and do
      // not start automatic probes after a failed status refresh.
    }
  }

  async function commitTransaction() {
    if (operation || connectionMutation) return;
    connectionMutation = 'Committing transaction…';
    const request = ++txStatusRequest;
    txStatusKnown = false;
    try {
      const next = await api.txCommit();
      if (request === txStatusRequest) updateTransactionStatus(next);
      const outcome = next.completion?.outcome ?? 'committed';
      flash(outcome === 'rolledBack' ? 'Transaction rolled back after an error.' : `${transactionCompletionLabel(outcome)}.`, outcome !== 'committed');
    } catch (error) {
      flash(errorMessage(error), true);
    } finally {
      connectionMutation = null;
    }
  }

  async function rollbackTransaction() {
    if (operation || connectionMutation) return;
    connectionMutation = 'Rolling back transaction…';
    const request = ++txStatusRequest;
    txStatusKnown = false;
    try {
      const next = await api.txRollback();
      if (request === txStatusRequest) updateTransactionStatus(next);
      flash('Transaction rolled back.');
    } catch (error) {
      flash(errorMessage(error));
    } finally {
      connectionMutation = null;
    }
  }

  async function refreshSchemaForConnection(connectionId: string | null) {
    schemaItems = [];
    schemaError = '';
    if (!connectionId) { schemaRequest++; schemaLoading = false; return; }
    await refreshSchema();
  }

  function retryMissingSchema(connectionId: string) {
    if (!workspaceLoaded || closingWorkspace || connectionMutation || operation || txStatus.active || connectionId !== tabConnectionId) return;
    // If reflection started before the connection check passed, let it finish first.
    if (schemaLoading) schemaRetryRequest = schemaRequest;
    else if (schemaError || !schemaItems.length) void refreshSchema();
  }

  async function refreshSchema() {
    const connectionId = activeConnection?.id;
    if (!connectionId) return;
    const request = ++schemaRequest;
    schemaLoading = true;
    schemaError = '';
    const finish = startActivity('schema', 'Load schema', connectionLabels[connectionId]);
    try {
      const tree = await api.introspectSchema(connectionId);
      const count = tree.schemas.length;
      finish('completed', `${count} schema ${count === 1 ? 'object' : 'objects'}`);
      if (request === schemaRequest && connectionId === tabConnectionId) schemaItems = tree.schemas;
    } catch (error) {
      finish('failed', errorMessage(error));
      if (request === schemaRequest && connectionId === tabConnectionId) schemaError = errorMessage(error);
    } finally {
      if (request === schemaRequest) {
        schemaLoading = false;
        if (request === schemaRetryRequest) retryMissingSchema(connectionId);
      }
    }
  }

  async function openDdl(item: SchemaItem) {
    const connectionId = tabConnectionId;
    if (!connectionId) return;
    try {
      const ddl = await api.getDdl(item.schema, item.name, connectionId);
      openReadonlyTab(`ddl:${connectionId}:${item.schema}.${item.name}`, `${item.schema}.${item.name}`, ddl, connectionId);
    } catch (error) {
      flash(errorMessage(error));
    }
  }

  function displayReadonlyTab(tab: WorkspaceTab) {
    captureActive();
    tabs = tabs.some((item) => item.id === tab.id) ? tabs.map((item) => item.id === tab.id ? tab : item) : [...tabs, tab];
    displayTab(tab);
  }

  function openReadonlyTab(id: string, title: string, content: string, connectionId = tabConnectionId) {
    displayReadonlyTab({ ...createQueryTab(id, title, content, connectionId), kind: 'reference' });
  }

  function browseTable(item: SchemaItem) {
    newQuery(`select *\nfrom ${quoteIdent(item.schema)}.${quoteIdent(item.name)};`, tabConnectionId, item.name);
  }

  async function copyHistorySql(entry: HistoryEntry) {
    try {
      await copyText(entry.query, 'Copy SQL from history');
      flash('SQL copied to clipboard.');
    } catch (error) {
      flash(`Could not copy SQL: ${errorMessage(error)}`, true);
    }
  }

  async function copyActivity(text: string) {
    try {
      await copyText(text, 'Copy activity log');
      flash('Log copied to clipboard.');
    } catch (error) {
      flash(`Could not copy log. ${errorMessage(error)}`, true);
    }
  }

  function executionLogText(entry: HistoryEntry) {
    return formatExecutionLog(entry, connectionLabels[entry.connectionId ?? ''] ?? 'Unavailable connection');
  }

  function copyHistoryLog(entry: HistoryEntry) {
    return copyActivity(executionLogText(entry));
  }

  async function copyResultLog(ids: number[]) {
    try {
      const entries = await api.historyEntries(ids);
      if (entries.length !== ids.length) throw new Error('Some statements are no longer in history.');
      await copyActivity(entries.map(executionLogText).join('\n\n'));
    } catch (error) {
      flash(`Could not open the execution log. ${errorMessage(error)}`, true);
    }
  }

  async function copyTransactionLog(transaction: HistoryTransaction) {
    try {
      const entries: HistoryEntry[] = [];
      while (entries.length < transaction.statementCount) {
        const page = await api.historyTransactionPage(transaction.id, entries.length, Math.min(200, transaction.statementCount - entries.length));
        if (!page.entries.length) break;
        entries.push(...page.entries);
      }
      if (!entries.length) throw new Error('This transaction has no statements in history.');
      await copyActivity(entries.map(executionLogText).join('\n\n'));
    } catch (error) {
      flash(`Could not open the transaction log. ${errorMessage(error)}`, true);
    }
  }

  async function openHistory(entry: HistoryEntry) {
    try {
      // Errors already carry their saved message; they have no row snapshot.
      const result = entry.hasResult && !entry.isError ? (await api.historyGetResult(entry.id)).result : null;
      if (entry.hasResult && !entry.isError && !result) {
        flash('No stored result for this entry. Showing its saved SQL.');
      }
      displayReadonlyTab(createHistoryTab(entry, result,
        connections.find(connection => connection.id === entry.connectionId)?.name ?? 'Unavailable connection'));
    } catch (error) {
      flash(errorMessage(error));
    }
  }

  function queueHistorySearch() {
    if (historyTimer) clearTimeout(historyTimer);
    historyTimer = setTimeout(() => void refreshHistory(), 300);
  }

  function searchPickerHistory(search: string) {
    pickerHistoryQuery = search;
    if (picker === 'commands') pickerHistorySearch.search(search);
  }

  async function showHistoryStorage(entry: HistoryEntry | null = null) {
    const request = { entry, stats: null as HistoryStorageStats | null, error: '' };
    historyStorage = request;
    if (entry) return;
    try {
      const stats = await api.historyStorageStats();
      if (historyStorage === request) historyStorage = { ...request, stats };
    } catch (error) {
      if (historyStorage === request) historyStorage = { ...request, error: errorMessage(error) };
    }
  }

  async function refreshHistory(append = false) {
    const search = historySearch;
    const request = ++historyRequest;
    historyLoading = true;
    try {
      const page = await api.historyPage(search || null, append ? history.length : 0, 50);
      if (request !== historyRequest || search !== historySearch) return;
      history = append ? [...history, ...page.entries] : page.entries;
      historyTotal = page.total;
      historyRevision++;
      const stats = await api.historyStats();
      if (request === historyRequest) historyStats = stats;
    } catch {
      if (request === historyRequest && !append) history = [];
    } finally {
      if (request === historyRequest) historyLoading = false;
    }
  }

  async function deleteHistory(id: number) {
    await api.historyDelete(id).catch((error) => flash(errorMessage(error)));
    await refreshHistory();
  }

  async function purgeHistory() {
    if (!confirm('Purge all query history?')) return;
    await api.historyPurge().catch((error) => flash(errorMessage(error)));
    await refreshHistory();
  }

  async function copyResult(format: 'csv' | 'tsv' | 'json') {
    if (!result || !result.rows.length) {
      flash('No exportable data.');
      return;
    }
    const source = result;
    try {
      await copyText(() => serializeRows(source, format), `Copy results as ${format.toUpperCase()}`,
        `${source.rows.length} ${source.rows.length === 1 ? 'row' : 'rows'}`);
      flash(`${format.toUpperCase()} copied to clipboard.`);
    } catch (error) {
      flash(`Copy failed: ${errorMessage(error)}`);
    }
  }

  async function downloadResult(format: 'csv' | 'tsv' | 'json', full = false) {
    if (!result || !result.rows.length) {
      flash('No exportable data.');
      return;
    }
    if (full && txStatus.active) { flash('Commit or roll back before re-running an export.'); return; }
    if (full && (!resultUsable || !resultSession || operation || connectionMutation || !result.autoLimited)) {
      flash('Re-run this query on its original connection before requesting a larger export.');
      return;
    }
    const source = result;
    const session = resultSession;
    const sourceTabId = activeTab;
    let op: Operation | null = null;
    try {
      const path = await save({ defaultPath: `catabase-${timestamp()}.${format}` });
      if (!path) return;
      let exportResult = source;
      if (full && session?.connectionId) {
        if (txStatus.active) { flash('Commit or roll back before re-running an export.'); return; }
        if (operation || connectionMutation) { flash('Wait for the current operation before exporting.'); return; }
        const sourceTab = sourceTabId === activeTab ? snapshotActive() : tabs.find((tab) => tab.id === sourceTabId);
        if (!canUseResult(sourceTab?.session ?? null, sourceTab?.connectionId ?? null) || sourceTab?.session?.executedAt !== session.executedAt) {
          flash('The source results changed. Choose Export again.');
          return;
        }
        op = beginOperation('export', session.connectionId);
        op.tabId = sourceTabId;
        flash('Preparing export…');
        exportResult = await connectionTester.query(op.connectionId, () => api.executeQuery(session.execution.sql, op!.token, 0, 10_000, session.connectionId!, true));
        if (!isCurrentOperation(op)) return;
      }
      await api.exportWriteFile(path, serializeRows(exportResult, format));
      flash(`${format.toUpperCase()} saved (${exportResult.rows.length} rows${exportResult.hasMore ? ', more rows available' : ''}).`);
    } catch (error) {
      flash(normalizeAppError(error).code === '57014' ? 'Export cancelled.' : `Export failed: ${errorMessage(error)}`, normalizeAppError(error).code !== '57014');
    } finally {
      if (op) finishOperation(op);
    }
  }

  function handleExport(action: string) {
    const [kind, format] = action.split('-') as [string, 'csv' | 'tsv' | 'json'];
    if (kind === 'copy') void copyResult(format);
    else void downloadResult(format, kind === 'more');
  }

  function enableEditMode(announce = true) {
    if (closingWorkspace || operation || connectionMutation || editDisabledReason || !editEligibility.target || !result) return false;
    if (editMode) return true;
    const target = editEligibility.target;
    editMode = true;
    originalRows = cloneRows(result.rows);
    pendingChanges = [];
    if (announce) flash(`Edit mode enabled for ${target.schema}.${target.table} using ${target.key.primaryKey ? 'primary' : 'unique'} key (${target.key.columns.join(', ')}).`);
    return true;
  }

  function updateCells(rowIndex: number, values: Record<string, unknown>) {
    const columns = Object.keys(values);
    if (!result?.rows[rowIndex] || !resultUsable || !columns.length || columns.some((column) => !editableColumns.includes(column)) || !enableEditMode(false)) return false;
    result = {
      ...result,
      rows: result.rows.map((row, index) => (index === rowIndex ? { ...row, ...values } : row))
    };
    pendingChanges = pendingChanges.filter((change) => !(change.rowIndex === rowIndex && columns.includes(change.column)));
    for (const [column, newValue] of Object.entries(values)) {
      const oldValue = originalRows[rowIndex]?.[column];
      if (!sameCellValue(oldValue, newValue, result.columns.find((item) => item.name === column)!, result.jsonValuesAsText, true)) pendingChanges = [...pendingChanges, { rowIndex, column, oldValue, newValue }];
    }
    return true;
  }

  function lookupForeignKey(request: ForeignKeyLookup, token: string) {
    if (!tabConnectionId || !resultUsable || operation || connectionMutation) return Promise.reject(new Error('Wait for an idle, editable result on its original connection.'));
    return api.foreignKeyValues(tabConnectionId, token, request);
  }

  function previewUpdates() {
    if (!pendingChanges.length) {
      flash('No pending changes.');
      return;
    }
    checkOriginalValues = true;
    updateReviewError = '';
    showUpdatePreview = true;
  }

  async function applyUpdates() {
    if (!pendingChanges.length || operation || connectionMutation || !resultUsable || !tabConnectionId) {
      flash('No pending changes.');
      return;
    }
    if (!reviewedUpdates.length) {
      flash(`Apply disabled: ${editEligibility.reason || 'Re-run the query to refresh its row key.'}`);
      return;
    }

    updateReviewError = '';
    const op = beginOperation('apply', tabConnectionId);
    const editedResult = result!;
    const editedSession = resultSession!;
    const editedRows = cloneRows(editedResult.rows);
    try {
      const applied = await api.applyRowUpdates(op.connectionId, op.token, reviewedUpdates);
      if (!isCurrentOperation(op)) return;
      patchTab(op.tabId, {
        session: { ...editedSession, data: { ...editedResult, commitFeedback: applied.commitFeedback } },
        originalRows: editedRows, pendingChanges: [], editMode: false
      });
      showUpdatePreview = false;
      flash(commitFeedbackLabel(applied.commitFeedback), applied.commitFeedback.outcome === 'interrupted');
    } catch (error) {
      const cancelled = normalizeAppError(error).code === '57014';
      updateReviewError = cancelled ? 'Update cancelled. Staged edits retained.' : `Apply failed: ${errorMessage(error)}`;
      flash(updateReviewError, !cancelled);
    } finally {
      finishOperation(op);
    }
  }

  function discardUpdates() {
    if (result) result = { ...result, rows: cloneRows(originalRows) };
    if (pendingChanges.length) flash('Pending edits discarded.');
    pendingChanges = [];
    editMode = false;
  }

  function generateUpdateSql(target: EditTarget, updates: RowUpdate[]): string {
    const columnTypes = new Map([...target.keyColumns, ...target.editableColumns].map((column) => [column.sourceName, column.typeName]));
    return updates.map((update) => {
      const assignments = Object.entries(update.values).map(([column, value]) => `${quoteIdent(column)} = ${sqlLiteral(value)}`);
      const predicates = Object.entries(update.original).map(([column, value]) => {
        const name = quoteIdent(column);
        const literal = sqlLiteral(value);
        if (update.keyColumns.includes(column)) return value === null ? `${name} IS NULL` : `${name} = ${literal}`;
        const type = columnTypes.get(column);
        if (type === 'json' || type === 'jsonb') return `to_jsonb(${name}) IS NOT DISTINCT FROM to_jsonb(${literal}::${type})`;
        return `${name} IS NOT DISTINCT FROM ${literal}`;
      });
      return `UPDATE ${quoteIdent(update.schema)}.${quoteIdent(update.table)}\nSET ${assignments.join(',\n    ')}\nWHERE ${predicates.join('\n  AND ')};`;
    }).join('\n\n');
  }

  function detectEditTarget(): EditTarget | null {
    return resultUsable ? editEligibility.target : null;
  }

  function normalizeAppError(error: unknown): AppError {
    const appError = error as AppError;
    return { ...appError, message: appError?.message ?? errorMessage(error) };
  }

  function connectBeforeRunError(error: AppError, connectionName: string): AppError {
    return {
      ...error,
      message: `Could not connect to ${connectionName}. Query was not sent.`,
      detail: [error.message, error.detail].filter(Boolean).join('\n')
    };
  }

  function setStatus(message: string, timings: QueryTimingDetails | null = null) {
    statusFlash = message;
    statusTiming = timings;
  }

  function flash(message: string, isError = false) {
    setStatus(message);
    toastError = isError;
    toastMessage = message;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toastMessage = '';
      toastTimer = null;
    }, 3600);
  }

  function cloneRows(rows: Record<string, unknown>[]): Record<string, unknown>[] {
    return rows.map((row) => ({ ...row }));
  }

  function sqlLiteral(value: unknown): string {
    if (value === null || value === undefined) return 'NULL';
    if (typeof value === 'number' || typeof value === 'boolean') return String(value);
    if (typeof value === 'object') value = JSON.stringify(value);
    return `E'${String(value).replaceAll('\\', '\\\\').replaceAll("'", "''")}'`;
  }

  function quoteIdent(value: string): string {
    return `"${value.replaceAll('"', '""')}"`;
  }

  function timestamp(): string {
    return new Date().toISOString().replaceAll(':', '-').replaceAll('.', '-');
  }

  function formatHistoryTime(value: string): string {
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return value;
    return new Intl.DateTimeFormat(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit'
    }).format(date);
  }

  function startResultsResize(event: PointerEvent) {
    event.preventDefault();
    resizingResults = true;
    window.addEventListener('pointermove', resizeResults);
    window.addEventListener('pointerup', stopResultsResize, { once: true });
  }

  function persistSidebarWidth(width: number) {
    try {
      localStorage.setItem('catabase.sidebarWidth', String(width));
    } catch {
      // Resizing still works if browser preference storage is unavailable/full.
    }
  }

  function resizeResults(event: PointerEvent) {
    const pane = document.querySelector<HTMLElement>('.main-pane');
    if (!pane) return;
    const nextHeight = pane.getBoundingClientRect().bottom - event.clientY;
    resultsHeight = Math.round(Math.min(Math.max(nextHeight, 180), Math.max(180, pane.clientHeight - 260)));
  }

  function stopResultsResize() {
    if (resizingResults) localStorage.setItem('catabase.resultsHeight', String(resultsHeight));
    resizingResults = false;
    window.removeEventListener('pointermove', resizeResults);
    window.removeEventListener('pointerup', stopResultsResize);
  }

  function resizeResultsWithKeyboard(event: KeyboardEvent) {
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
    event.preventDefault();
    const delta = event.key === 'ArrowUp' ? 24 : -24;
    resultsHeight = Math.min(Math.max(resultsHeight + delta, 180), Math.max(180, window.innerHeight - 320));
    localStorage.setItem('catabase.resultsHeight', String(resultsHeight));
  }

  async function withTimeout<T>(promise: Promise<T>, ms: number, message: string): Promise<T> {
    let timeout: ReturnType<typeof setTimeout> | undefined;
    try {
      return await Promise.race([
        promise,
        new Promise<never>((_, reject) => {
          timeout = setTimeout(() => reject(new Error(message)), ms);
        })
      ]);
    } finally {
      if (timeout) clearTimeout(timeout);
    }
  }

</script>

<svelte:document on:contextmenu={(event) => event.preventDefault()} on:selectstart={preventWorkspaceSelection} />

<main class="app-shell" class:native-titlebar={nativeTitlebar} inert={closingWorkspace || Boolean(tabCloseTask) || queryPreparing || Boolean(connectionMutation)} aria-busy={Boolean(tabCloseTask) || queryPreparing || Boolean(connectionMutation)}>
  <header class="toolbar" data-tauri-drag-region={nativeTitlebar ? 'deep' : undefined}>
    <div class="brand" aria-label="catabase">
      <Cat mood={activeConnection ? 'awake' : 'asleep'} animate={false} />
      <span class="wordmark" aria-hidden="true"><b>CAT</b>ABASE<i></i></span>
    </div>
    {#if updateState.update}
      <button class="update-badge" title={`Catabase ${updateState.update.version} is available`} on:click={openAppUpdates}><Icon name="download" size={12} /><span>Update available</span></button>
    {/if}
    <button class="workspace-search" aria-label="Search workspace" title="Search workspace (Cmd/Ctrl+K)" on:click={() => picker = 'commands'}><Icon name="search" size={14} /><span>Search tables, queries, connections, history…</span><kbd>⌘K</kbd></button>
    {#if operation}<button class="operation-status" title="Go to the tab with this operation" on:click={() => switchTab(operation!.tabId)}>{operationText}</button>{/if}
    <ConnectionPill
      switcher
      activity={operation?.connectionId === tabConnectionId ? operation.cancelling ? 'cancelling' : 'running' : null}
      environment={connectionAppearance.environment}
      connection={statusConnection}
      online={Boolean(activeConnection)}
      connectionLocked={focusedConnectionLocked} {lockedConnectionName}
      state={tabConnectionTest}
      now={nowMs}
      disabled={running || Boolean(connectionMutation)}
      onTest={testCurrentConnection}
      onChoose={() => picker = 'connections'}
    />
    {#if connectionMutation}<span class="operation-status" role="status">{connectionMutation}</span>{/if}
  </header>

  <section bind:this={workspaceElement} class="workspace" class:sidebar-open={sidebarOpen} class:resizing-sidebar={resizingSidebar} style={`--sidebar-width: ${visibleSidebarWidth}px`}>
    <aside class="icon-strip" aria-label="Workspace navigation">
      <button
        class:active={sidebar === 'connections' && sidebarOpen}
        aria-label="Connections"
        aria-pressed={sidebar === 'connections' && sidebarOpen}
        data-tooltip="Connections"
        on:click={() => selectSidebar('connections')}
      ><Icon name="database" /></button>
      <button
        class:active={sidebar === 'schema' && sidebarOpen}
        aria-label="Schema"
        aria-pressed={sidebar === 'schema' && sidebarOpen}
        data-tooltip="Schema"
        on:click={() => selectSidebar('schema')}
      ><Icon name="schema" /></button>
      <button
        class:active={sidebar === 'history' && sidebarOpen}
        aria-label="History"
        aria-pressed={sidebar === 'history' && sidebarOpen}
        data-tooltip="History"
        on:click={() => selectSidebar('history')}
      ><Icon name="history" /></button>
      <button
        class:active={sidebar === 'activity' && sidebarOpen}
        aria-label="Activity"
        aria-pressed={sidebar === 'activity' && sidebarOpen}
        data-tooltip="Activity"
        on:click={() => selectSidebar('activity')}
      ><Icon name="activity" /></button>
      <button
        class:active={sidebar === 'tabs' && sidebarOpen}
        aria-label="Tabs"
        aria-pressed={sidebar === 'tabs' && sidebarOpen}
        data-tooltip="Open & closed tabs"
        on:click={() => selectSidebar('tabs')}
      ><Icon name="tabs" /></button>
      <button
        class:active={sidebar === 'settings' && sidebarOpen}
        aria-label="Settings"
        aria-pressed={sidebar === 'settings' && sidebarOpen}
        data-tooltip="Settings"
        on:click={() => selectSidebar('settings')}
      ><Icon name="settings" /></button>
    </aside>

    {#if sidebarOpen}
      <aside id="workspace-sidebar" bind:this={sidebarElement} class="sidebar" class:connections-sidebar={sidebar === 'connections'}>
        {#if sidebar === 'connections'}
          <div class="panel-head">
            <div>
              <h2>Connections</h2>
              <p>{connections.length} saved</p>
            </div>
            <button class="icon-label-button" disabled={running || Boolean(connectionMutation)} on:click={openNewConnection}><Icon name="plus" size={15} /> Add</button>
          </div>
          {#if connections.length === 0}
            <div class="empty-state">
              <span class="empty-state-icon"><Icon name="database" size={24} /></span>
              <strong>No connections yet</strong>
              <p>Add a PostgreSQL connection to start querying.</p>
              <button class="primary" disabled={running || Boolean(connectionMutation)} on:click={openNewConnection}><Icon name="plus" size={15} /> Add connection</button>
            </div>
          {:else}
            <ul bind:this={connectionList} class="connection-list" class:dragging={Boolean(connectionDrag)} aria-label="Saved connections">
              {#each connections as connection (connection.id)}
                <li
                  data-connection-row-id={connection.id}
                  class:dragging={connectionDrag?.id === connection.id}
                  class:drop-before={connectionDrop?.id === connection.id && connectionDrop.side === 'before'}
                  class:drop-after={connectionDrop?.id === connection.id && connectionDrop.side === 'after'}
                >
                  <ConnectionCard
                    {connection}
                    active={connection.id === activeConnection?.id}
                    appearance={workspaceAppearance(appearances, connection.id, showConnectionModal ? { id: connectionForm.id, appearance: connectionAppearanceForm } : undefined)}
                    disabled={running || Boolean(connectionMutation)}
                    useDisabled={focusedConnectionLocked} useDisabledReason="Connection locked. Right-click the focused tab to unlock."
                    globalSelection={connectionScope === 'global'}
                    reorderable={connections.length > 1}
                    onDragStart={(x, y) => startConnectionDrag(connection.id, x, y)}
                    onDragMove={updateConnectionDrag}
                    onDragEnd={finishConnectionDrag}
                    onReorder={(direction) => reorderConnectionWithKeyboard(connection.id, direction)}
                    onActivate={() => activateConnection(connection.id)}
                    onDisconnect={() => disconnectConnection(connection.id)}
                    onEdit={() => editConnection(connection)}
                    onDuplicate={() => duplicateConnection(connection.id)}
                    onCopy={() => copyConnection(connection)}
                    onPaste={pasteConnection}
                    onDelete={() => deleteConnection(connection.id)}
                  />
                </li>
              {/each}
            </ul>
          {/if}
        {:else if sidebar === 'schema'}
          <div class="panel-head">
            <div>
              <h2>Schema</h2>
              <p>{schemaItems.length} objects</p>
            </div>
            <button class="icon-button" aria-label="Refresh schema" title="Refresh schema" on:click={() => refreshSchema()} disabled={schemaLoading}>
              <Icon name="refresh" size={16} />
            </button>
          </div>
          <label class="search-field" aria-label="Search schema">
            <Icon name="search" size={15} />
            <input {...literalTextAttributes} placeholder="Tables, columns, types…" bind:value={schemaSearch} />
            {#if schemaSearch}<button aria-label="Clear schema search" on:click={() => (schemaSearch = '')}><Icon name="close" size={13} /></button>{/if}
          </label>
          {#if schemaLoading}<p class="empty">Loading schema...</p>{/if}
          {#if schemaError}<p class="error-text">{schemaError}</p>{/if}
          {#if !schemaLoading && schemaItems.length > 0 && schemaGroups.length === 0}
            <div class="empty-state compact"><strong>No schema matches</strong><p>Try another table or column name.</p></div>
          {/if}
          {#each schemaGroups as group (group.name)}
            <section class="schema-group">
              <div class="schema-group-title"><span>{group.name}</span><small>{group.items.length}</small></div>
              {#each group.items as item (`${item.schema}.${item.name}`)}
                <details class="schema-item">
                  <summary>
                    <Icon name="chevron" size={14} />
                    <span class="schema-object-name">{item.name}</span>
                    <span class="object-kind">{item.kind}</span>
                  </summary>
                  <div class="schema-detail">
                    {#each item.columns as column (column.name)}
                      <div class="column-row">
                        <span title={column.name}>{column.name}{#if column.primaryKey}<b class="key-tag">PK</b>{/if}{#if column.generated}<b class="generated-tag">GEN</b>{/if}</span>
                        <small title={`${column.typeName}${column.nullable ? '' : ' · not null'}`}>{shortType(column.typeName)}</small>
                      </div>
                    {/each}
                    <div class="row-actions"><button class="ddl-button" on:click={() => browseTable(item)}>Query table</button><button class="ddl-button" on:click={() => openDdl(item)}>Open DDL</button></div>
                  </div>
                </details>
              {/each}
            </section>
          {/each}
        {:else if sidebar === 'history'}
          <div class="panel-head">
            <div>
              <h2>History</h2>
              <p>{historyStats.total} queries · {historyStats.errors} errors</p>
            </div>
            <div class="history-toolbar">
              <button class="history-icon-button" aria-label="History storage" on:click={() => showHistoryStorage()} title="History storage usage"><Icon name="database" size={15} /></button>
              <button class="history-icon-button danger-quiet" aria-label="Purge history" title="Purge all history" on:click={purgeHistory}><Icon name="trash" size={15} /></button>
            </div>
          </div>
          <label class="search-field" aria-label="Search query history">
            <Icon name="search" size={15} />
            <input {...literalTextAttributes} placeholder="Search query history…" bind:value={historySearch} on:input={queueHistorySearch} />
            {#if historySearch}<button aria-label="Clear history search" on:click={() => { historySearch = ''; void refreshHistory(); }}><Icon name="close" size={13} /></button>{/if}
          </label>
          {#if historySaveError}<p class="error-text" role="alert">{historySaveError}</p>{/if}
          {#if history.length === 0}
            <div class="empty-state compact"><strong>No query history</strong><p>Executed queries will appear here.</p></div>
          {/if}
          {#each groupHistory(history) as group (group.key)}
            {#if group.transaction}
              <HistoryTransactionGroup transaction={group.transaction} revision={historyRevision} {connections} connectionId={group.entries[0].connectionId} matches={group.entries} searching={Boolean(historySearch.trim())} onCopy={copyHistorySql} onCopyLog={copyHistoryLog} onCopyTransaction={copyTransactionLog} onOpen={openHistory} onDelete={deleteHistory} />
            {:else}
              <HistoryEntryCard entry={group.entries[0]} {connections} onCopy={copyHistorySql} onCopyLog={copyHistoryLog} onOpen={openHistory} onDelete={deleteHistory} />
            {/if}
          {/each}
          {#if history.length < historyTotal}<button class="history-more" disabled={historyLoading} on:click={() => refreshHistory(true)}>{historyLoading ? 'Loading…' : `Load more (${history.length} of ${historyTotal})`}</button>{/if}
        {:else if sidebar === 'activity'}
          <ActivityPanel {historyRevision} {connectionLabels} onOpen={openHistory} onCopy={copyActivity} />
        {:else if sidebar === 'tabs'}
          <TabsPanel tabs={tabs.map(tab => tab.id === activeTab ? { ...tab, sql, connectionId: tabConnectionId } : tab)} {archivedTabs} {connections} activeId={activeTab} onSelect={switchTab} onReopen={reopenTab} onNew={() => newQuery()} />
        {:else}
          <div class="panel-head"><div><h2>Settings</h2><p>Workspace preferences</p></div></div>
          <AppUpdates state={updateState} automatic={automaticUpdates} blocked={updateBlocked} installError={updateInstallError}
            onCheck={() => void updateChecker.check()} onAutomatic={setAutomaticUpdates} onInstall={() => void installAppUpdate()}
            onReleases={() => void api.openReleases().catch(error => flash(errorMessage(error), true))} />
          <section class="settings-card">
            <div class="settings-card-head"><strong>Diagnostics</strong></div>
            <p>Review a crash report or report a problem. Nothing is sent automatically.</p>
            <button disabled={diagnosticsLoading} on:click={() => void loadDiagnostics()}>{diagnosticsLoading ? 'Preparing report…' : 'Report a problem'}</button>
          </section>
          <section class="settings-card">
            <div class="settings-card-head"><strong>Connection switching</strong></div>
            <Dropdown value={connectionScope} ariaLabel="Switch connections for" options={[{ value: 'tab', label: 'Focused tab' }, { value: 'global', label: 'All unlocked tabs' }]} disabled={running || Boolean(connectionMutation)} onSelect={scope => setConnectionScope(scope as ConnectionScope)} />
            <p>{connectionScope === 'tab' ? 'Each tab keeps its own connection. Selecting a connection changes only the focused tab.' : 'Selecting a connection changes all unlocked tabs. Locked tabs keep their assigned connection.'}</p>
            <p>Right-click a tab to lock or unlock its connection.</p>
          </section>
          <section class="settings-card">
            <div class="settings-card-head"><strong>SQL editor</strong></div>
            <Switch label="Vim mode" checked={vimModeEnabled} onChange={setVimMode} />
            <p>Use Vim keybindings across SQL tabs.</p>
          </section>
          <section class="settings-card">
            <div class="settings-card-head"><strong>Commit mode</strong></div>
            <CommitModeSelect mode={settings.commitMode} disabled={!workspaceLoaded || running || Boolean(connectionMutation) || closingWorkspace} onChange={setCommitMode} />
            <p>Changing mode applies when the next transaction opens.</p>
          </section>
          <section class="settings-card">
            <div class="settings-card-head"><strong>Appearance</strong></div>
            <div class="theme-options" role="group" aria-label="Theme">
              {#each ['tuxedo', 'snow', 'system'] as choice}
                <button aria-pressed={theme === choice} on:click={() => setTheme(choice as ThemePreference)}>{choice === 'tuxedo' ? 'Tuxedo' : choice === 'snow' ? 'Snow' : 'System'}</button>
              {/each}
            </div>
          </section>
          <section class="settings-card">
            <div class="settings-card-head"><strong>Connection pool</strong><span>{settings.poolSize} connections</span></div>
            <label>
              Maximum connections
              <div class="inline-control">
                <input {...literalTextAttributes} type="number" min="1" max="32" bind:value={poolSizeInput} />
                <button disabled={running || Boolean(connectionMutation)} on:click={savePoolSize}>Save</button>
              </div>
            </label>
            <p>Choose between 1 and 32 concurrent PostgreSQL connections.</p>
            <p>{currentPoolStats ? `${currentPoolStats.active} active · ${currentPoolStats.idle} idle` : 'Pool statistics will appear when this connection is in use.'}</p>
          </section>
          <section class="settings-card">
            <div class="settings-card-head"><strong>Supermaven autocomplete</strong><span>{supermavenBusy ? 'starting…' : supermaven.state}</span></div>
            <p>{supermaven.message}</p>
            <p>Enabling sends the active SQL draft (including comments and literals) and reflected table, column and key definitions to Supermaven. Connection credentials, result rows and other tabs are not added. Built-in schema suggestions stay local.</p>
            <div class="row-actions">
              <button disabled={supermavenBusy} on:click={() => setSupermaven(!supermaven.enabled)}>{supermaven.enabled ? 'Disable Supermaven' : 'Enable Supermaven'}</button>
              {#if supermaven.enabled && supermaven.state !== 'ready'}<button disabled={supermavenBusy} on:click={() => setSupermaven(true)}>Retry</button>{/if}
              {#if supermaven.activationUrl}
                <button on:click={() => supermavenAccount('activate')}>Sign in to Supermaven</button>
                <button on:click={() => supermavenAccount('free')}>Use free tier</button>
              {/if}
            </div>
            <p>Tab accepts a suggestion; Escape dismisses it. Option/Alt+\ requests one explicitly. Supermaven currently provides legacy access for existing customers.</p>
          </section>
        {/if}
      </aside>
      <PaneResizer bind:width={sidebarWidth} bounds={sidebarWidthBounds(workspaceWidth)} defaultWidth={DEFAULT_SIDEBAR_WIDTH} label="Resize sidebar" controls="workspace-sidebar" bind:resizing={resizingSidebar} onResizeEnd={persistSidebarWidth} />
    {/if}

    <section class="main-pane" class:resizing={resizingResults} style={`--results-height: ${resultsHeight}px`}>
      <div class="tabbar">
      <nav bind:this={tabBar} class="tabs" class:dragging={Boolean(draggingTab)} aria-label="Query tabs">
        {#each tabs as tab (tab.id)}
          {@const association = tab.id === activeTab ? tabConnectionId : tab.connectionId}
          {@const tabAppearance = workspaceAppearance(appearances, association, showConnectionModal ? { id: connectionForm.id, appearance: connectionAppearanceForm } : undefined)}
          <QueryTab
            id={tab.id} title={tab.title} active={activeTab === tab.id} readonly={tab.kind === 'reference'}
            connectionName={connections.find((connection) => connection.id === association)?.name ?? (association ? 'Unavailable connection' : 'No connection')}
            connectionColor={tabAppearance.color} hasConnection={Boolean(association)} connectionLocked={Boolean(tab.connectionLocked)}
            lockDisabled={running || Boolean(connectionMutation)} onToggleConnectionLock={() => toggleConnectionLock(tab.id)}
            running={operation?.tabId === tab.id}
            failed={tab.kind === 'query' && tab.id !== activeTab && operation?.tabId !== tab.id && Boolean(tab.error)}
            closeDisabled={operation?.tabId === tab.id}
            pending={Boolean((tab.id === activeTab ? pendingChanges : tab.pendingChanges).length)}
            renaming={renamingTab === tab.id} {renameValue}
            dragging={draggingTab === tab.id} dropSide={tabDrop?.id === tab.id ? tabDrop.side : null}
            onSelect={() => switchTab(tab.id)} onRename={() => startRename(tab.id)}
            onDuplicate={() => duplicateTab(tab.id)} onClose={() => closeTab(tab.id)}
            onCopy={() => copyTab(tab.id)} onPaste={pasteTab}
            onRenameValue={(value) => { renameValue = value; }} onRenameKeydown={handleRenameKeydown} onRenameBlur={() => finishRename(false)}
            onDragStart={(x, y) => startTabDrag(tab.id, x, y)} onDragMove={updateTabDrag} onDragEnd={finishTabDrag}
            onReorder={(direction) => reorderTabWithKeyboard(tab.id, direction)}
          />
        {/each}
        <button class="new-tab" aria-label="New query tab" title="New query tab (Cmd/Ctrl+T)" on:click={() => newQuery()}><Icon name="plus" size={16} /></button>
      </nav>
      <div class="editor-actions" aria-label="SQL actions">
        <button aria-label="Find in SQL editor" title="Find in SQL editor (Cmd/Ctrl+F)" disabled={!workspaceLoaded} on:click={() => editorRef?.find()}><Icon name="search" size={16} /></button>
        <button aria-label="Format SQL" title="Format selected SQL, or the entire draft" disabled={!workspaceLoaded || tabKind !== 'query'} on:click={formatSql}><Icon name="format" size={16} /><span class="action-label">Format SQL</span></button>
        <button title="Explain selected SQL or the statement at the cursor" aria-label="Explain query" disabled={!workspaceLoaded || running || Boolean(connectionMutation) || explainOpening || tabKind !== 'query' || !activeConnection} on:click={openExplain}><Icon name="explain" size={16} /><span class="action-label">Explain</span></button>
        <button class="primary run-button" class:cancel-button={running} on:click={runQuery} disabled={!workspaceLoaded || Boolean(connectionMutation) || queryPreparing || operation?.cancelling || (!running && tabKind !== 'query')} title={activeConnection ? 'Run current statement (Cmd/Ctrl+Enter)' : 'Review a saved connection before running'}>
          <Icon name={running ? 'stop' : 'play'} size={14} /><span>{operation?.cancelling ? 'Cancelling…' : running ? 'Cancel' : 'Run'}</span><kbd>⌘↵</kbd>
        </button>
      </div>
      </div>

      <div class="editor-pane">
        {#if txStatus.active}
          <div class="tx-banner">
            <strong>Transaction open</strong>
            <span title={txStatus.startedAt ? `Started ${formatHistoryTime(txStatus.startedAt)}` : undefined}>{transactionDuration}</span>
            {#if txStatus.rowActivity}
              {@const { inserted, updated, deleted, tables } = txStatus.rowActivity}
              <span role="status" title="PostgreSQL row operations, including work rolled back to a savepoint">
                {inserted.toLocaleString()} inserted · {updated.toLocaleString()} updated · {deleted.toLocaleString()} deleted · {tables.toLocaleString()} {tables === 1 ? 'table' : 'tables'}
              </span>
            {:else if txStatus.rowActivity === null}
              <span>Row counts unavailable</span>
            {/if}
            <button disabled={running || Boolean(connectionMutation)} on:click={commitTransaction}>Commit</button>
            <button disabled={running || Boolean(connectionMutation)} on:click={rollbackTransaction}>Rollback</button>
          </div>
        {/if}
        {#if workspaceLoaded}
        {#key activeTab}
          <SqlEditor
            bind:this={editorRef}
            bind:value={sql}
            tabId={activeTab}
            readOnly={tabKind !== 'query'}
            fontSize={editorFontSize}
            {vimModeEnabled}
            {themeKey}
            onPosition={(line, column) => { cursorLine = line; cursorColumn = column; }}
            viewState={editorViewState}
            supermavenEnabled={canRequestSupermaven(supermaven) && tabKind === 'query'}
            {schemaItems}
            supermavenComplete={api.supermavenComplete}
            supermavenCancel={api.supermavenCancel}
          />
        {/key}
        {/if}
      </div>

      <!-- svelte-ignore a11y_no_noninteractive_element_interactions a11y_no_noninteractive_tabindex -->
      <div
        class="results-resizer"
        role="separator"
        aria-label="Resize results panel"
        aria-orientation="horizontal"
        aria-valuemin="180"
        aria-valuemax={Math.max(180, window.innerHeight - 320)}
        aria-valuenow={resultsHeight}
        tabindex="0"
        on:pointerdown={startResultsResize}
        on:keydown={resizeResultsWithKeyboard}
      ><span></span></div>

      <section class="results-pane">
        <div class="results-head">
          <div class="results-title">
            <strong>Results</strong>
            {#if activeOperation}
              <span class="running-state"><span class="operation-dot" aria-hidden="true"></span>{operationText}</span>
            {:else if queryError}
              <span class="failed-state">Failed{historyContext ? ` · ${historyContext.durationMs} ms` : ''}</span>
            {:else if result}
              <span class:failed-state={result.commitFeedback?.outcome === 'interrupted'}>
                {#if !resultCommitLabel || (result.columns.length && result.commitFeedback?.affectedRows === null)}
                  {result.rowCount}{result.hasMore ? '+' : ''} {result.rowCount === 1 ? 'row' : 'rows'} ·
                {/if}
                {#if resultCommitLabel}{resultCommitLabel} · {/if}{result.durationMs} ms
              </span>
            {:else if historyContext}
              <span>No snapshot stored · {historyContext.durationMs} ms</span>
            {:else}
              <span>No query run</span>
            {/if}
          </div>
          <div class="results-actions">
            <ActionMenu label="Export" ariaLabel="Export results" disabled={Boolean(queryError) || !result?.rows.length} items={exportItems} onSelect={handleExport} />
            {#if editMode && !queryError}
              <button disabled={!pendingChanges.length || running || Boolean(connectionMutation)} on:click={previewUpdates}>Review {pendingChanges.length} {pendingChanges.length === 1 ? 'edit' : 'edits'}</button>
              <button disabled={running || Boolean(connectionMutation)} on:click={discardUpdates}>{pendingChanges.length ? 'Discard' : 'Cancel editing'}</button>
            {:else}
              <button aria-disabled={Boolean(editDisabledReason)} title={editDisabledReason || 'Edit cells using the selected primary or unique key'} on:click={() => enableEditMode()}><Icon name="edit" size={14} /> Edit cells</button>
            {/if}
          </div>
        </div>
        {#if resultContext && (!queryError || historyContext)}
          <div class="result-context">
            <span>{resultContext.connectionName}</span>
            <time title={resultContext.executedAt}>{formatHistoryTime(resultContext.executedAt)}</time>
            {#if resultContext.snapshot}<span class="snapshot-badge">History snapshot · read only</span>{/if}
            {#if !resultContext.snapshot && resultContext.connectionId !== tabConnectionId}<span class="context-warning">Different connection · run the query to refresh</span>{/if}
            <details>
              <summary>Executed SQL</summary>
              <pre>{resultContext.execution.sql}</pre>
              {#if resultContext.historyEntryIds?.length}
                <button on:click={() => copyResultLog(resultContext.historyEntryIds!)}><Icon name="copy" size={14} /> Copy execution log</button>
              {/if}
            </details>
          </div>
        {/if}
        {#if editMode && !queryError}
          <div class="edit-bar">
            {pendingChanges.length} pending {pendingChanges.length === 1 ? 'change' : 'changes'}.
            {#if detectEditTarget()}
              Apply will update {detectEditTarget()?.schema}.{detectEditTarget()?.table} using {editEligibility.target?.key.primaryKey ? 'primary' : 'unique'} key ({editEligibility.target?.key.columns.join(', ')}).
            {:else}
              Editing unavailable: {editEligibility.reason}
            {/if}
          </div>
        {/if}
        {#if (queryError || !result?.rows.length) && !(activeOperation?.kind === 'query' && !result)}
          <div class="empty-state results-empty">
            <Cat mood={queryError ? 'error' : 'asleep'} size={52} muted animate={false} />
            {#if queryError}
              <strong>Query failed</strong>
              <div class="query-error" role="alert" data-selectable-text>
                <strong>{queryError.message}</strong>
                {#if queryError.detail}<span>Detail: {queryError.detail}</span>{/if}
                {#if queryError.hint}<span>Hint: {queryError.hint}</span>{/if}
                {#if queryError.code}<span>Code: {queryError.code}</span>{/if}
                {#if queryError.position}<span>Position: {queryError.position}</span>{/if}
              </div>
            {:else}
              <strong>{result ? result.columns.length ? 'No rows returned' : 'Statement completed' : tabKind === 'query' ? 'Run a query to see results' : 'SQL reference'}</strong>
            {/if}
            {#if queryError && result}
              <button on:click={() => patchTab(activeTab, { error: null })}>Show previous results</button>
            {:else if tabKind !== 'query' && (!result || queryError)}
              <button on:click={() => newQuery(sql, tabConnectionId)}>Open in new query tab</button>
            {:else if !result && !queryError && tabKind === 'query'}
              <p>Place the cursor in a statement and press <kbd>⌘↵</kbd>.</p>
            {/if}
          </div>
        {:else if result?.rows.length}
          {#key `${activeTab}:${resultSession?.executedAt}`}
            <ResultsGrid {result} {editableColumns} {editDisabledReason} schemaItems={resultSession?.connectionId === tabConnectionId ? schemaItems : []} {canLoadMore} resultContext={`${activeTab}:${resultSession?.executedAt ?? ''}`} onLoadMore={loadMoreRows} readOnly={!resultUsable} disabled={running || Boolean(connectionMutation)} changedCells={pendingChanges} onEditMany={updateCells} onForeignKeyLookup={lookupForeignKey} onCancelLookup={(token) => { void api.cancelQuery(token).catch(() => {}); }} onMessage={flash} />
          {/key}
          <footer class="results-foot">
            <span>{result.rows.length} loaded {result.hasMore ? '· more available' : ''}</span>
            {#if activeOperation?.kind !== 'load' && result.autoLimited && result.hasMore && resultUsable}
              <span>{txStatus.active ? 'Commit or roll back to load more' : pendingChanges.length ? 'Apply or discard edits to load more' : 'Scroll down to load more'}</span>
            {/if}
          </footer>
        {/if}
      </section>
    </section>
  </section>

  <footer class="status-bar">
    <ConnectionPill connection={statusConnection} online={Boolean(activeConnection)} connectionLocked={focusedConnectionLocked} {lockedConnectionName} state={tabConnectionTest} activity={operation?.connectionId === tabConnectionId ? operation.cancelling ? 'cancelling' : 'running' : null} now={nowMs} compact disabled={running || Boolean(connectionMutation)} onTest={testCurrentConnection} onChoose={() => { sidebar = 'connections'; sidebarOpen = true; }} />
    <CommitModeSelect compact mode={settings.commitMode} disabled={!workspaceLoaded || running || Boolean(connectionMutation) || closingWorkspace} onChange={setCommitMode} />
    {#if activeConnection}<button class="status-link" tabindex="-1" title={`Open connection pool settings for ${activeConnection.name}`} on:click={() => { sidebar = 'settings'; sidebarOpen = true; }}>pool {currentPoolStats ? `${currentPoolStats.active} active · ${currentPoolStats.idle} idle · ${currentPoolStats.poolSize} max` : '—'}</button>{/if}
    <button class="status-link" tabindex="-1" title="Open schema browser" on:click={() => { sidebar = 'schema'; sidebarOpen = true; }}>{schemaItems.length ? `${schemaItems.length} schema objects` : schemaLoading ? 'Loading schema…' : 'Schema'}</button>
    <span class="draft-status">{!workspaceLoaded ? 'restoring workspace…' : closingWorkspace || draftStatus === 'saving' ? 'saving drafts…' : draftStatus === 'pending' ? 'saving drafts…' : draftStatus === 'error' ? 'draft save failed' : 'drafts saved'}</span>
    {#if operation}<span class="footer-operation">{operationText}</span>{/if}
    <span class="cursor-position">Ln {cursorLine}, Col {cursorColumn}</span>
    {#if statusTiming}
      <button class="status-flash status-timing" tabindex="-1" title="Show query timing breakdown" aria-haspopup="dialog" on:click={() => (timingDetails = statusTiming)}>{statusFlash}</button>
    {:else}
      <span class="status-flash">{statusFlash}</span>
    {/if}
  </footer>
</main>

{#if updateInstalling}
  <Modal label="Updating Catabase" locked onClose={() => {}}>
    <div class="modal">
      <h2>Updating Catabase</h2>
      <p role="status">{updateProgress?.phase === 'restarting' ? 'Restarting…' : updateProgress?.phase === 'installing' ? 'Installing verified update…' : updateProgress ? 'Downloading update…' : 'Saving SQL drafts…'}</p>
      {#if updateProgress?.phase === 'downloading' && updateProgress.totalBytes}
        <progress aria-label="Update download" value={updateProgress.downloadedBytes} max={updateProgress.totalBytes}></progress>
        <p>{Math.min(100, Math.floor(updateProgress.downloadedBytes / updateProgress.totalBytes * 100))}% downloaded</p>
      {/if}
      <p>Catabase will reopen when the update is installed.</p>
    </div>
  </Modal>
{/if}

{#if historyStorage}
  <HistoryStorageDialog entry={historyStorage.entry} stats={historyStorage.stats} error={historyStorage.error} onClose={() => historyStorage = null} />
{/if}
{#if diagnosticsOpen && diagnosticReport}
  <DiagnosticDialog report={diagnosticReport} onClose={closeDiagnostics} />
{/if}
{#if reconnectReview}
  <ResumeConnectionDialog {connections} initialConnectionId={reconnectReview.connectionId} {appearances} query={reconnectReview.target.sql} locked={focusedConnectionLocked} globalSelection={connectionScope === 'global'} onConfirm={resolveReconnectReview} onClose={() => resolveReconnectReview(null)} />
{/if}
{#if picker}
  {#key picker}
    <WorkspacePicker label={picker === 'connections' ? 'Switch connection' : 'Search workspace'} placeholder={picker === 'connections' ? 'Find a connection…' : 'Search tables, queries, connections, history, commands…'} items={pickerItems} onSearch={searchPickerHistory} searchStatus={picker === 'commands' ? pickerHistory.loading ? 'Searching history…' : pickerHistory.error ? `History unavailable: ${pickerHistory.error}` : pickerHistory.total > 20 ? `Showing the 20 newest of ${pickerHistory.total} history matches. Refine your search for older entries.` : '' : ''} onSelect={selectPickerItem} onClose={() => picker = null} />
  {/key}
{/if}

{#if explainSession}
  <ExplainDialog query={explainSession.query} connectionName={explainSession.connectionName} transactionActive={txStatus.active}
    state={explainState} now={nowMs} onStart={runExplain} onShare={shareExplain} onCancel={() => void explainWorkflow.cancel()}
    onClose={closeExplain} onOpenLink={api.openExplainLink} />
{/if}

{#if timingDetails}
  <QueryTimingDialog details={timingDetails} onClose={() => (timingDetails = null)} />
{/if}

{#if toastMessage}
  <div class="toast" class:error-toast={toastError} role="status">
    <span class="toast-icon"><Icon name={toastError ? 'close' : 'check'} size={15} /></span>
    <span>{toastMessage}</span>
    <button aria-label="Dismiss message" on:click={() => (toastMessage = '')}><Icon name="close" size={14} /></button>
  </div>
{/if}

{#if showUpdatePreview}
  <Modal label="Review cell edits" locked={running} onClose={() => (showUpdatePreview = false)}>
    <div class="modal update-review">
      <div class="panel-head"><h2>Review cell edits</h2><button disabled={running} on:click={() => (showUpdatePreview = false)}>Close</button></div>
      <p>{pendingChanges.length} staged {pendingChanges.length === 1 ? 'change' : 'changes'} on {resultSession?.connectionName}. {#if editEligibility.target}Rows are identified by {editEligibility.target.key.primaryKey ? 'primary' : 'unique'} key ({editEligibility.target.key.columns.join(', ')}).{/if}</p>
      {#if txStatus.active || settings.commitMode !== 'auto'}
        <p class="context-warning">{txStatus.active ? 'Changes will use the open transaction.' : 'Applying changes will open a transaction.'} Commit to save them, or Rollback to undo them.</p>
      {:else}
        <p class="context-warning">Changes will commit when applied.</p>
      {/if}
      <div>
        <Switch label="Only if unchanged" bind:checked={checkOriginalValues} disabled={running} />
        <p>{checkOriginalValues ? 'Apply only if the edited cells still match their original values.' : 'Match by row key and overwrite the edited cells even if they have changed.'}</p>
      </div>
      {#if editEligibility.target && reviewedUpdates.length}
        <SqlPreview sql={generateUpdateSql(editEligibility.target, reviewedUpdates)} {themeKey} />
      {:else}
        <p class="context-warning">Apply is unavailable without an unambiguous source table and a complete primary or unique key.</p>
      {/if}
      {#if updateReviewError}<p class="error-text" role="alert">{updateReviewError}</p>{/if}
      <div class="row-actions"><button disabled={running} on:click={() => (showUpdatePreview = false)}>Back to results</button><button class="primary" disabled={running || Boolean(connectionMutation) || !reviewedUpdates.length} on:click={applyUpdates}>{running ? 'Applying…' : 'Apply changes'}</button></div>
    </div>
  </Modal>
{/if}

{#if discardRunConfirmation}
  <Modal label="Discard staged edits?" onClose={() => resolveDiscardForRun(false)}>
    <div class="modal">
      <h2>Discard staged edits?</h2>
      <p>You have {discardRunConfirmation.changeCount} staged {discardRunConfirmation.changeCount === 1 ? 'cell edit' : 'cell edits'}{discardRunConfirmation.globalSelection ? ' across the affected tabs' : ''}. {#if discardRunConfirmation.connectionName}Switching to {discardRunConfirmation.connectionName} will discard these changes.{:else}Running a new query will replace this result set and discard these changes.{/if} They have not been saved to the database.</p>
      <div class="row-actions">
        <button on:click={() => resolveDiscardForRun(false)}>Keep editing</button>
        <button class="danger-quiet" on:click={() => resolveDiscardForRun(true)}>Discard edits and {discardRunConfirmation.connectionName ? 'switch' : 'run'}</button>
      </div>
    </div>
  </Modal>
{/if}

{#if showConnectionModal}
  <Modal label={connectionForm.id ? 'Edit connection' : 'New connection'} locked={savingConnection} onClose={() => (showConnectionModal = false)}>
    <ConnectionEditor bind:connection={connectionForm} bind:appearance={connectionAppearanceForm}
      feedback={modalFeedback} saving={savingConnection} testing={testingConnection}
      onClose={() => showConnectionModal = false} onSave={saveConnection} onTest={testConnection} />
  </Modal>
{/if}
