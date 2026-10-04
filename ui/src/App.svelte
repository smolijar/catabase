<script lang="ts">
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';
  import { save } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import Dropdown from './components/Dropdown.svelte';
  import ActionMenu from './components/ActionMenu.svelte';
  import ResultsGrid from './components/ResultsGrid.svelte';
  import Modal from './components/Modal.svelte';
  import Icon from './components/Icon.svelte';
  import SqlEditor from './components/SqlEditor.svelte';
  import { serializeRows } from './lib/export';
  import { api, errorMessage } from './lib/ipc';
  import { canUseResult, createQueryTab, restoreWorkspace, serializeWorkspace } from './lib/workspace';
  import type { PendingChange, ResultSession, WorkspaceTab } from './lib/workspace';
  import type {
    AiHealth,
    AppError,
    ColumnMeta,
    ConnectionInput,
    ConnectionRecord,
    ConnectionStats,
    ExecutionTarget,
    HistoryEntry,
    HistoryStats,
    QueryResult,
    SchemaItem,
    Settings,
    TxStatus
  } from './lib/types';

  const DEFAULT_SQL = `-- Welcome to Catabase\n-- Add a PostgreSQL connection, then run SQL with Cmd/Ctrl+Enter.\nselect now() as connected_at;`;
  const PAGE_SIZE = 200;

  type SidebarTab = 'connections' | 'schema' | 'history' | 'settings';
  type EditorHandle = {
    getExecutionTarget: (splitSql: typeof api.splitSql) => Promise<ExecutionTarget>;
    clearHighlights: () => void;
    markError: (offset: number, message: string) => void;
    getViewState: () => WorkspaceTab['viewState'];
  };
  type EditTarget = {
    schema: string;
    table: string;
    whereColumns: ColumnMeta[];
    warning?: string;
  };
  type SchemaGroup = { name: string; items: SchemaItem[] };
  type Operation = { token: string; tabId: string; connectionId: string; kind: 'query' | 'load' | 'export' | 'apply'; startedAt: number; cancelling: boolean };

  let sidebar: SidebarTab = 'connections';
  let sidebarOpen = true;
  let sql = DEFAULT_SQL;
  let activeTab = 'playground';
  let tabs: WorkspaceTab[] = [createQueryTab('playground', 'Query 1', DEFAULT_SQL)];
  let tabKind: WorkspaceTab['kind'] = 'query';
  let tabConnectionId: string | null = null;
  let editorViewState: WorkspaceTab['viewState'] = null;
  let renamingTab = '';
  let renameValue = '';
  let workspaceLoaded = false;
  let editorRef: EditorHandle | undefined;
  let running = false;
  let nowMs = Date.now();
  let operation: Operation | null = null;
  let resultSession: ResultSession | null = null;
  let result: QueryResult | null = null;
  let originalRows: Record<string, unknown>[] = [];
  let queryError: AppError | null = null;
  let statusFlash = 'Ready';
  let toastVisible = false;
  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  let toastError = false;
  let mounted = false;
  let txStatus: TxStatus = { active: false, startedAt: null };
  let editMode = false;
  let pendingChanges: PendingChange[] = [];

  let connections: ConnectionRecord[] = [];
  let settings: Settings = { poolSize: 5, aiModel: null };
  let poolStats: ConnectionStats | null = null;
  let poolSizeInput = 5;
  let aiModels: string[] = [];
  let aiHealth: AiHealth = { state: 'off', message: null };
  let schemaItems: SchemaItem[] = [];
  let schemaContext = '';
  let schemaSearch = '';
  let schemaGroups: SchemaGroup[] = [];
  let schemaLoading = false;
  let schemaError = '';
  let history: HistoryEntry[] = [];
  let historyStats: HistoryStats = { total: 0, errors: 0, lastExecutedAt: null };
  let historySearch = '';
  let historyTotal = 0;
  let historyLoading = false;
  let historyTimer: ReturnType<typeof setTimeout> | null = null;
  let resultsHeight = 360;
  let resizingResults = false;
  let showUpdatePreview = false;
  let schemaRequest = 0;

  let showConnectionModal = false;
  let connectionForm: ConnectionInput = emptyConnection();
  let connectionUrl = '';
  let modalFeedback = '';
  let testingConnection = false;
  let savingConnection = false;
  let draftTimer: ReturnType<typeof setTimeout> | null = null;
  let urlTimer: ReturnType<typeof setTimeout> | null = null;

  $: activeConnection = connections.find((connection) => connection.id === tabConnectionId) ?? null;
  $: running = operation !== null;
  $: resultUsable = canUseResult(resultSession, tabConnectionId) && tabKind === 'query';
  $: schemaContext = schemaItems
    .map((item) => `${item.schema}.${item.name}(${item.columns.map((column) => column.name).join(', ')})`)
    .join('\n');
  $: schemaGroups = groupSchemaItems(schemaItems, schemaSearch);
  $: aiModelOptions = [
    { value: '', label: 'Default' },
    ...Array.from(new Set([...(settings.aiModel ? [settings.aiModel] : []), ...aiModels])).map((model) => ({ value: model, label: aiModels.includes(model) ? model : `${model} (unavailable)` }))
  ];
  $: if (workspaceLoaded) scheduleDraftSave(sql, activeTab, tabConnectionId, tabs);
  $: if (workspaceLoaded) void refreshSchemaForConnection(tabConnectionId);
  $: exportItems = [
    { id: 'copy-csv', label: 'Copy as CSV' },
    { id: 'copy-tsv', label: 'Copy as TSV' },
    { id: 'copy-json', label: 'Copy as JSON' },
    { id: 'save-csv', label: 'Save CSV…' },
    { id: 'save-tsv', label: 'Save TSV…' },
    { id: 'save-json', label: 'Save JSON…' },
    { id: 'more-csv', label: 'Re-run & save up to 10k (CSV)…', disabled: !resultUsable || !result?.autoLimited || running },
    { id: 'more-json', label: 'Re-run & save up to 10k (JSON)…', disabled: !resultUsable || !result?.autoLimited || running }
  ];

  onMount(() => {
    mounted = true;
    const savedResultsHeight = Number(localStorage.getItem('catabase.resultsHeight'));
    if (Number.isFinite(savedResultsHeight) && savedResultsHeight >= 180) resultsHeight = savedResultsHeight;
    void bootstrap();
    window.addEventListener('keydown', handleKeydown);
    window.addEventListener('beforeunload', persistDraftNow);
    const statsInterval = window.setInterval(() => {
      if (activeConnection) void loadConnectionStats();
    }, 5000);
    const runningInterval = window.setInterval(() => {
      nowMs = Date.now();
      if (running && activeConnection) void loadConnectionStats();
    }, 1000);
    const aiInterval = window.setInterval(checkAiHealth, 30000);

    return () => {
      window.removeEventListener('keydown', handleKeydown);
      window.removeEventListener('beforeunload', persistDraftNow);
      window.clearInterval(statsInterval);
      window.clearInterval(runningInterval);
      window.clearInterval(aiInterval);
      if (toastTimer) window.clearTimeout(toastTimer);
      if (draftTimer) window.clearTimeout(draftTimer);
      if (historyTimer) window.clearTimeout(historyTimer);
      stopResultsResize();
    };
  });

  async function bootstrap() {
    await Promise.allSettled([
      loadSettings(),
      loadConnections(),
      refreshHistory(),
      checkAiHealth(),
      refreshTxStatus()
    ]);
    await loadDraft();
    workspaceLoaded = true;
  }

  function snapshotActive(): WorkspaceTab {
    const tab = tabs.find((candidate) => candidate.id === activeTab)!;
    return { ...tab, sql, connectionId: tabConnectionId, viewState: editorRef?.getViewState() ?? editorViewState,
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

  function newQuery(content = '', connectionId = tabConnectionId, title?: string) {
    captureActive();
    const tab = createQueryTab(crypto.randomUUID(), title ?? `Query ${tabs.filter((tab) => tab.kind === 'query').length + 1}`, content, connectionId);
    tabs = [...tabs, tab];
    displayTab(tab);
  }

  function closeTab(id: string) {
    if (operation?.tabId === id) { flash('Cancel or wait for this query before closing its tab.'); return; }
    captureActive();
    const tab = tabs.find((item) => item.id === id);
    if (tab?.pendingChanges.length && !confirm('Discard the staged cell edits and close this tab?')) return;
    if (tab?.kind === 'query' && tab.sql.trim() && !confirm(`Close “${tab.title}”? Its draft will be removed from the workspace.`)) return;
    const index = tabs.findIndex((item) => item.id === id);
    tabs = tabs.filter((item) => item.id !== id);
    if (!tabs.length) tabs = [createQueryTab(crypto.randomUUID(), 'Query 1', '', tabConnectionId)];
    if (activeTab === id) displayTab(tabs[Math.min(index, tabs.length - 1)]);
  }

  function startRename(id = activeTab) {
    renamingTab = id;
    renameValue = tabs.find((tab) => tab.id === id)?.title ?? '';
  }

  function finishRename() {
    if (renameValue.trim()) tabs = tabs.map((tab) => tab.id === renamingTab ? { ...tab, title: renameValue.trim() } : tab);
    renamingTab = '';
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
      ssl: false
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

  function handleKeydown(event: KeyboardEvent) {
    if (event.defaultPrevented || showConnectionModal || showUpdatePreview) return;
    const command = event.metaKey || event.ctrlKey;
    if (!command) return;
    if (event.key === 'Enter') {
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
      connections = await withTimeout(api.listConnections(), 5000, 'Loading connections timed out.');
    } catch (error) {
      flash(`Connections unavailable: ${errorMessage(error)}`);
    }
  }

  async function loadDraft() {
    try {
      const restored = restoreWorkspace(await api.loadDraft('workspace.v1') ?? '');
      if (restored) {
        tabs = restored.tabs;
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

  function scheduleDraftSave(_sql: string, _activeId: string, _connectionId: string | null, _tabs: WorkspaceTab[]) {
    if (draftTimer) clearTimeout(draftTimer);
    draftTimer = setTimeout(() => void persistDraftNow(), 1200);
  }

  function persistDraftNow() {
    if (!mounted || !workspaceLoaded) return;
    const captured = snapshotActive();
    const workspace = tabs.map((tab) => tab.id === activeTab ? captured : tab);
    void api.saveDraft('workspace.v1', serializeWorkspace(workspace, activeTab)).catch((error) => flash(`Draft save failed: ${errorMessage(error)}`, true));
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
    return { data, execution, connectionId, connectionName: connections.find((connection) => connection.id === connectionId)?.name ?? 'Unavailable connection', executedAt: new Date().toISOString(), snapshot };
  }

  function resultPatch(session: ResultSession): Partial<WorkspaceTab> {
    return { session, originalRows: cloneRows(session.data.rows), pendingChanges: [], editMode: false, error: null };
  }

  async function runQuery() {
    if (operation) {
      await cancelQuery();
      return;
    }
    if (tabKind !== 'query') {
      flash('Open this SQL in a query tab to run it.');
      return;
    }
    if (!activeConnection) { sidebar = 'connections'; sidebarOpen = true; flash('Choose a connection for this query.'); return; }
    if (pendingChanges.length && !confirm('Discard staged cell edits and run this query?')) return;
    const op = beginOperation('query', activeConnection.id);
    const editor = editorRef;
    let target: ExecutionTarget = { sql: sql.trim(), start: 0, end: sql.length };
    statusFlash = 'Running query...';
    try {
      target = editor ? await editor.getExecutionTarget(api.splitSql) : target;
      if (!target.sql.trim()) throw new Error('No query to execute.');
      if (op.cancelling) { flash('Query cancelled before execution.'); return; }
      patchTab(op.tabId, { error: null });
      void loadConnectionStats();
      const next = await api.executeQuery(target.sql, op.token, 0, PAGE_SIZE, op.connectionId);
      if (!isCurrentOperation(op)) return;
      patchTab(op.tabId, resultPatch(makeSession(next, target, op.connectionId)));
      flash(`${op.cancelling ? 'Query completed before cancellation. ' : ''}Loaded ${next.rowCount}${next.hasMore ? '+' : ''} ${next.rowCount === 1 ? 'row' : 'rows'} in ${next.durationMs} ms.`);
    } catch (error) {
      if (!isCurrentOperation(op)) return;
      const normalized = normalizeAppError(error);
      if (normalized.code === '57014') {
        flash('Query cancelled.');
      } else {
        patchTab(op.tabId, { error: normalized });
        if (normalized.position && activeTab === op.tabId) editorRef?.markError(target.start + normalized.position - 1, normalized.message);
        flash(normalized.message, true);
      }
    } finally {
      finishOperation(op);
    }
  }

  async function cancelQuery() {
    const op = operation;
    if (!op || op.cancelling) return;
    op.cancelling = true;
    operation = op;
    statusFlash = 'Cancelling…';
    try {
      await api.cancelQuery(op.token);
    } catch (error) {
      if (isCurrentOperation(op)) { op.cancelling = false; operation = op; flash(`Cancellation failed: ${errorMessage(error)}`, true); }
    }
  }

  async function loadMoreRows() {
    if (!result || !resultSession || !resultUsable || operation || !tabConnectionId) return;
    if (pendingChanges.length) { flash('Apply or discard staged edits before loading more rows.'); return; }
    const session = { ...resultSession, data: result };
    const op = beginOperation('load', tabConnectionId);
    try {
      const next = await api.executeQuery(session.execution.sql, op.token, session.data.rows.length, PAGE_SIZE, session.connectionId!, true);
      if (!isCurrentOperation(op)) return;
      const merged = {
        ...next,
        rows: [...session.data.rows, ...next.rows],
        rowCount: session.data.rows.length + next.rows.length
      };
      patchTab(op.tabId, resultPatch({ ...session, data: merged }));
      flash(`Loaded ${merged.rowCount}${next.hasMore ? '+' : ''} rows.`);
    } catch (error) {
      if (isCurrentOperation(op)) flash(normalizeAppError(error).code === '57014' ? 'Loading cancelled.' : `Load more failed: ${errorMessage(error)}`, normalizeAppError(error).code !== '57014');
    } finally {
      finishOperation(op);
    }
  }

  function clearEditor() {
    if (tabKind !== 'query' || operation?.tabId === activeTab) {
      flash('Open an idle query tab to clear SQL.');
      return;
    }
    if (sql.trim() && !confirm('Clear the SQL editor? This cannot be undone.')) return;
    sql = '';
    flash('Editor cleared.');
  }

  function clearResults() {
    if (operation?.tabId === activeTab) return;
    if (pendingChanges.length && !confirm('Discard staged cell edits and clear results?')) return;
    setResult(null);
    queryError = null;
    editorRef?.clearHighlights();
    flash('Results cleared.');
  }

  function openNewConnection() {
    connectionForm = emptyConnection();
    connectionUrl = '';
    modalFeedback = '';
    showConnectionModal = true;
  }

  function editConnection(connection: ConnectionRecord) {
    connectionForm = { ...connection };
    modalFeedback = '';
    showConnectionModal = true;
    void syncConnectionUrl();
  }

  function queueConnectionUrlSync() {
    if (urlTimer) clearTimeout(urlTimer);
    urlTimer = setTimeout(() => void syncConnectionUrl(), 250);
  }

  async function syncConnectionUrl() {
    if (!connectionForm.host || !connectionForm.database || !connectionForm.username) return;
    try {
      connectionUrl = await api.buildConnectionUrl(connectionForm);
    } catch {
      // URL field remains manually editable while form is incomplete.
    }
  }

  async function importConnectionUrl() {
    try {
      const parsed = await api.parseConnectionUrl(connectionUrl);
      connectionForm = { ...connectionForm, ...parsed };
      modalFeedback = 'URL imported.';
    } catch (error) {
      modalFeedback = errorMessage(error);
    }
  }

  async function testConnection() {
    testingConnection = true;
    modalFeedback = 'Testing connection...';
    try {
      await withTimeout(api.testConnection(connectionForm), 12000, 'Connection test timed out.');
      modalFeedback = 'Connection test succeeded.';
    } catch (error) {
      modalFeedback = errorMessage(error);
    } finally {
      testingConnection = false;
    }
  }

  async function saveConnection() {
    if (operation) return;
    const initiatingTab = activeTab;
    const previousId = connectionForm.id;
    savingConnection = true;
    try {
      const saved = await withTimeout(api.saveConnection(connectionForm), 12000, 'Saving connection timed out.');
      showConnectionModal = false;
      await loadConnections();
      // A reused ID may now point at a different database. Never apply or replay
      // results obtained using its previous configuration.
      if (previousId) {
        captureActive();
        for (const tab of tabs.filter((item) => item.connectionId === previousId)) {
          patchTab(tab.id, { session: null, originalRows: [], pendingChanges: [], editMode: false, error: null });
        }
      }
      patchTab(initiatingTab, { connectionId: saved.id });
      flash(`${saved.name} saved and ready.`);
      void refreshTxStatus();
      void refreshSchema();
    } catch (error) {
      modalFeedback = errorMessage(error);
    } finally {
      savingConnection = false;
    }
  }

  async function activateConnection(id: string) {
    if (operation) { flash('Finish or cancel the running operation before switching connections.'); return; }
    if (pendingChanges.length && !confirm('Discard staged edits before changing this tab’s connection?')) return;
    const initiatingTab = activeTab;
    const current = snapshotActive();
    captureActive();
    try {
      const active = await api.activateConnection(id);
      await loadConnections();
      patchTab(initiatingTab, {
        connectionId: id,
        pendingChanges: [],
        editMode: false,
        session: current.session ? { ...current.session, data: { ...current.session.data, rows: cloneRows(current.originalRows) } } : null
      });
      poolStats = null;
      flash(`Using ${active.name}.`);
      void refreshTxStatus();
      void loadConnectionStats();
    } catch (error) {
      flash(errorMessage(error), true);
    }
  }

  async function deleteConnection(id: string) {
    if (operation) return;
    if (!confirm('Delete this connection?')) return;
    try {
      await api.deleteConnection(id);
      await loadConnections();
      if (tabConnectionId === id) { tabConnectionId = null; poolStats = null; }
      void refreshTxStatus();
      if (connections.some((connection) => connection.isActive)) {
        void refreshSchema();
      } else {
        schemaItems = [];
      }
      flash('Connection deleted.');
    } catch (error) {
      flash(errorMessage(error));
    }
  }

  async function savePoolSize() {
    try {
      const normalized = await api.setPoolSize(poolSizeInput);
      settings = { ...settings, poolSize: normalized };
      poolSizeInput = normalized;
      flash(`Pool size saved: ${normalized}.`);
    } catch (error) {
      flash(errorMessage(error));
    }
  }

  async function saveAiModel(model: string) {
    try {
      settings = { ...settings, aiModel: await api.setAiModel(model || null) };
      flash(settings.aiModel ? `AI model: ${settings.aiModel}.` : 'AI model reset.');
    } catch (error) {
      flash(errorMessage(error));
    }
  }

  async function checkAiHealth() {
    try {
      aiHealth = await api.aiHealth();
      if (aiHealth.state === 'on') aiModels = await api.aiModels();
    } catch {
      aiHealth = { state: 'off', message: 'OpenCode unavailable' };
    }
  }

  async function loadConnectionStats() {
    if (!connections.some((connection) => connection.id === tabConnectionId && connection.isActive)) { poolStats = null; return; }
    poolStats = await api.connectionStats().catch(() => poolStats);
  }

  async function refreshTxStatus() {
    txStatus = await api.txStatus().catch(() => ({ active: false, startedAt: null }));
  }

  async function commitTransaction() {
    try {
      txStatus = await api.txCommit();
      flash('Transaction committed.');
    } catch (error) {
      flash(errorMessage(error));
    }
  }

  async function rollbackTransaction() {
    try {
      txStatus = await api.txRollback();
      flash('Transaction rolled back.');
    } catch (error) {
      flash(errorMessage(error));
    }
  }

  async function refreshSchemaForConnection(connectionId: string | null) {
    schemaItems = [];
    schemaError = '';
    if (!connectionId) { schemaRequest++; schemaLoading = false; return; }
    await refreshSchema();
  }

  async function refreshSchema() {
    const connectionId = tabConnectionId;
    if (!connectionId) return;
    const request = ++schemaRequest;
    schemaLoading = true;
    schemaError = '';
    try {
      const tree = await api.introspectSchema(connectionId);
      if (request === schemaRequest && connectionId === tabConnectionId) schemaItems = tree.schemas;
    } catch (error) {
      if (request === schemaRequest) schemaError = errorMessage(error);
    } finally {
      if (request === schemaRequest) schemaLoading = false;
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

  function openReadonlyTab(id: string, title: string, content: string, connectionId = tabConnectionId, session: ResultSession | null = null) {
    captureActive();
    const tab = { ...createQueryTab(id, title, content, connectionId), kind: 'reference' as const, session };
    tabs = tabs.some((item) => item.id === id) ? tabs.map((item) => item.id === id ? tab : item) : [...tabs, tab];
    displayTab(tab);
  }

  function browseTable(item: SchemaItem) {
    newQuery(`select *\nfrom ${quoteIdent(item.schema)}.${quoteIdent(item.name)};`, tabConnectionId, item.name);
  }

  function restoreHistory(entry: HistoryEntry) {
    newQuery(entry.query, entry.connectionId ?? null, `History #${entry.id}`);
  }

  async function openHistory(entry: HistoryEntry) {
    try {
      const payload = await api.historyGetResult(entry.id);
      if (!payload.result) {
        flash('No stored result for this entry.');
        return;
      }
      const session: ResultSession = { data: payload.result, execution: { sql: entry.query, start: 0, end: entry.query.length }, connectionId: entry.connectionId ?? null,
        connectionName: connections.find((connection) => connection.id === entry.connectionId)?.name ?? 'Unavailable connection', executedAt: entry.executedAt, snapshot: true };
      openReadonlyTab(`history:${entry.id}`, `History #${entry.id}`, entry.query, entry.connectionId ?? null, session);
    } catch (error) {
      flash(errorMessage(error));
    }
  }

  function queueHistorySearch() {
    if (historyTimer) clearTimeout(historyTimer);
    historyTimer = setTimeout(() => void refreshHistory(), 300);
  }

  async function refreshHistory(append = false) {
    const search = historySearch;
    historyLoading = true;
    try {
      const page = await api.historyPage(search || null, append ? history.length : 0, 50);
      if (search !== historySearch) return;
      history = append ? [...history, ...page.entries] : page.entries;
      historyTotal = page.total;
      historyStats = await api.historyStats();
    } catch {
      if (!append) history = [];
    } finally {
      historyLoading = false;
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
    const text = serializeRows(result, format);
    try {
      await writeText(text).catch(() => navigator.clipboard.writeText(text));
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
    if (full && (!resultUsable || !resultSession || operation || !result.autoLimited)) {
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
        if (operation) { flash('Wait for the running operation before exporting.'); return; }
        op = beginOperation('export', session.connectionId);
        op.tabId = sourceTabId;
        flash('Preparing export…');
        exportResult = await api.executeQuery(session.execution.sql, op.token, 0, 10_000, session.connectionId, true);
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

  function handleWorkspaceAction(action: string) {
    if (action === 'clear-sql') clearEditor();
    else if (action === 'clear-results') clearResults();
    else if (action === 'rename') startRename();
    else if (action === 'duplicate') newQuery(sql, tabConnectionId, `${tabs.find((tab) => tab.id === activeTab)?.title ?? 'Query'} copy`);
    else if (action === 'settings') { sidebar = 'settings'; sidebarOpen = true; }
  }

  function enableEditMode() {
    if (!result || !result.rows.length || !resultUsable || operation) {
      flash('No result set to edit. Run a query first.');
      return;
    }
    editMode = true;
    originalRows = cloneRows(result.rows);
    pendingChanges = [];
    const target = detectEditTarget();
    flash(
      target
        ? `Edit mode enabled for ${target.schema}.${target.table}.${target.warning ? ` ${target.warning}` : ''}`
        : 'Edit mode enabled. Preview only: source table could not be detected.'
    );
  }

  function disableEditMode() {
    if (pendingChanges.length && !confirm('Discard pending changes?')) return;
    discardUpdates();
    editMode = false;
  }

  function updateCell(rowIndex: number, column: string, newValue: unknown) {
    if (!result || !resultUsable || operation) return;
    const oldValue = originalRows[rowIndex]?.[column];
    result = {
      ...result,
      rows: result.rows.map((row, index) => (index === rowIndex ? { ...row, [column]: newValue } : row))
    };
    pendingChanges = pendingChanges.filter((change) => !(change.rowIndex === rowIndex && change.column === column));
    if (JSON.stringify(oldValue) !== JSON.stringify(newValue)) {
      pendingChanges = [...pendingChanges, { rowIndex, column, oldValue, newValue }];
    }
  }

  function previewUpdates() {
    if (!pendingChanges.length) {
      flash('No pending changes.');
      return;
    }
    showUpdatePreview = true;
  }

  async function applyUpdates() {
    if (!pendingChanges.length || operation || !resultUsable || !tabConnectionId) {
      flash('No pending changes.');
      return;
    }
    const target = detectEditTarget();
    if (!target) {
      flash('Apply disabled: source table could not be detected. Use Preview SQL.');
      return;
    }

    const op = beginOperation('apply', tabConnectionId);
    const editedRows = cloneRows(result!.rows);
    const updates = buildRowUpdates(target);
    try {
      const applied = await api.applyRowUpdates(op.connectionId, op.token, updates);
      if (!isCurrentOperation(op)) return;
      patchTab(op.tabId, { originalRows: editedRows, pendingChanges: [], editMode: false });
      showUpdatePreview = false;
      flash(`${applied.updatedRows} ${applied.updatedRows === 1 ? 'row' : 'rows'} updated${applied.inTransaction ? ' in the open transaction. Commit to save.' : '.'}`);
    } catch (error) {
      flash(normalizeAppError(error).code === '57014' ? 'Update cancelled; staged edits retained.' : `Apply failed: ${errorMessage(error)}`, normalizeAppError(error).code !== '57014');
    } finally {
      finishOperation(op);
    }
  }

  function buildRowUpdates(target: EditTarget) {
    const indices = [...new Set(pendingChanges.map((change) => change.rowIndex))];
    const sourceColumns = result!.columns.filter((column) => column.tableSchema === target.schema && column.tableName === target.table && column.sourceName);
    return indices.map((rowIndex) => ({ schema: target.schema, table: target.table,
      values: Object.fromEntries(pendingChanges.filter((change) => change.rowIndex === rowIndex).map((change) => [sourceColumnName(columnByName(change.column)), change.newValue])),
      original: Object.fromEntries(sourceColumns.map((column) => [sourceColumnName(column), originalRows[rowIndex]?.[column.name]])) }));
  }

  function discardUpdates() {
    if (result) result = { ...result, rows: cloneRows(originalRows) };
    pendingChanges = [];
    flash('Pending edits discarded.');
  }

  function generateUpdateSql(): string {
    const target = detectEditTarget();
    if (!target) {
      return [
        '-- Source table detection unavailable. Replace table_name and WHERE clause before running.',
        ...pendingChanges.map(
          (change) =>
            `UPDATE table_name SET ${quoteIdent(change.column)} = ${sqlLiteral(change.newValue)} WHERE /* row ${change.rowIndex + 1} key */;`
        )
      ].join('\n');
    }

    const changesByRow = new Map<number, PendingChange[]>();
    for (const change of pendingChanges) {
      const column = columnByName(change.column);
      if (!column || column.generated) continue;
      const changes = changesByRow.get(change.rowIndex) ?? [];
      changes.push(change);
      changesByRow.set(change.rowIndex, changes);
    }

    return [
      target.warning ? `-- ${target.warning}` : '',
      ...Array.from(changesByRow.entries()).map(([rowIndex, changes]) => {
        const setSql = changes
          .map((change) => `${quoteIdent(sourceColumnName(columnByName(change.column)))} = ${sqlLiteral(change.newValue)}`)
          .join(', ');
        const whereSql = target.whereColumns
          .map(
            (column) =>
              `${quoteIdent(sourceColumnName(column))} IS NOT DISTINCT FROM ${sqlLiteral(originalRows[rowIndex]?.[column.name])}`
          )
          .join(' AND ');
        return `UPDATE ${quoteIdent(target.schema)}.${quoteIdent(target.table)} SET ${setSql} WHERE ${whereSql};`;
      })
    ]
      .filter(Boolean)
      .join('\n');
  }

  function detectEditTarget(): EditTarget | null {
    if (!result || !result.columns.length || !resultUsable) return null;
    const changedColumns = pendingChanges.map((change) => columnByName(change.column)).filter(Boolean) as ColumnMeta[];
    const relevantColumns = changedColumns.length ? changedColumns : result.columns;
    const identities = new Set(
      relevantColumns
        .filter((column) => column.tableSchema && column.tableName && sourceColumnName(column))
        .map((column) => `${column.tableSchema}\u001f${column.tableName}`)
    );
    if (identities.size !== 1) return null;

    const [identity] = Array.from(identities);
    const [schema, table] = identity.split('\u001f');
    const tableColumns = result.columns.filter(
      (column) => column.tableSchema === schema && column.tableName === table && sourceColumnName(column)
    );
    if (!tableColumns.length) return null;

    const primaryKeyColumns = tableColumns.filter((column) => column.primaryKey);
    if (primaryKeyColumns.length) return { schema, table, whereColumns: tableColumns.filter((column) => !column.generated) };
    return null;
  }

  function setResult(next: QueryResult | null) {
    result = next;
    if (!next) resultSession = null;
    originalRows = next ? cloneRows(next.rows) : [];
    pendingChanges = [];
    editMode = false;
  }

  function canApplyUpdates(): boolean {
    return !operation && resultUsable && pendingChanges.length > 0 && detectEditTarget() !== null;
  }

  function normalizeAppError(error: unknown): AppError {
    const appError = error as AppError;
    return { ...appError, message: appError?.message ?? errorMessage(error) };
  }

  function flash(message: string, isError = false) {
    statusFlash = message;
    toastError = isError;
    toastVisible = true;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toastVisible = false;
      toastTimer = null;
    }, 3600);
  }

  function cloneRows(rows: Record<string, unknown>[]): Record<string, unknown>[] {
    return rows.map((row) => ({ ...row }));
  }

  function columnByName(name: string): ColumnMeta | undefined {
    return result?.columns.find((column) => column.name === name);
  }

  function sourceColumnName(column: ColumnMeta | undefined): string {
    return column?.sourceName || column?.name || '';
  }

  function sqlLiteral(value: unknown): string {
    if (value === null || value === undefined) return 'NULL';
    if (typeof value === 'number' || typeof value === 'boolean') return String(value);
    if (typeof value === 'object') value = JSON.stringify(value);
    return `E'${String(value).replaceAll('\\', '\\\\').replaceAll("'", "\\'")}'`;
  }

  function quoteIdent(value: string): string {
    return `"${value.replaceAll('"', '""')}"`;
  }

  function timestamp(): string {
    return new Date().toISOString().replaceAll(':', '-').replaceAll('.', '-');
  }

  function runningDuration(): string {
    if (!operation) return '0.0s';
    return `${((nowMs - operation.startedAt) / 1000).toFixed(1)}s`;
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

<main class="app-shell">
  <header class="toolbar">
    <div class="brand">
      <span class="brand-mark">C</span>
      <div>
        <strong>Catabase</strong>
      </div>
    </div>
    <button
      class="primary run-button"
      class:cancel-button={running}
      on:click={runQuery}
      disabled={operation?.cancelling || (!running && (!activeConnection || tabKind !== 'query'))}
      title={activeConnection ? 'Run current statement (Cmd/Ctrl+Enter)' : 'Add or activate a connection first'}
    >
      <Icon name={running ? 'stop' : 'play'} size={16} />
      <span>{operation?.cancelling ? 'Cancelling…' : running ? 'Cancel' : 'Run'}</span>
      <kbd>⌘↵</kbd>
    </button>
    <button
      class="chip connection-chip"
      class:connected={Boolean(activeConnection)}
      on:click={() => { sidebar = 'connections'; sidebarOpen = true; }}
      title="Choose this tab’s database connection"
    >
      <span class="status-dot"></span>
      {activeConnection ? activeConnection.name : 'Choose connection'}
    </button>
    {#if operation}<button class="operation-status" on:click={() => switchTab(operation!.tabId)}>{operation.cancelling ? 'Cancelling' : operation.kind === 'query' ? 'Running' : operation.kind === 'load' ? 'Loading rows' : operation.kind === 'apply' ? 'Applying edits' : 'Exporting'} · {runningDuration()} · {tabs.find((tab) => tab.id === operation?.tabId)?.title}</button>{/if}
    <div class="toolbar-spacer"></div>
    <ActionMenu label="More" ariaLabel="Workspace actions" items={[
      { id: 'duplicate', label: 'Duplicate in new query tab' },
      { id: 'rename', label: 'Rename tab' },
      { id: 'clear-sql', label: 'Clear SQL', disabled: tabKind !== 'query' || operation?.tabId === activeTab },
      { id: 'clear-results', label: 'Clear results', disabled: !result || operation?.tabId === activeTab },
      { id: 'settings', label: 'Settings' }
    ]} onSelect={handleWorkspaceAction} />
  </header>

  <section class="workspace">
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
        class:active={sidebar === 'settings' && sidebarOpen}
        aria-label="Settings"
        aria-pressed={sidebar === 'settings' && sidebarOpen}
        data-tooltip="Settings"
        on:click={() => selectSidebar('settings')}
      ><Icon name="settings" /></button>
    </aside>

    {#if sidebarOpen}
      <aside class="sidebar">
        {#if sidebar === 'connections'}
          <div class="panel-head">
            <div>
              <h2>Connections</h2>
              <p>{connections.length} saved</p>
            </div>
            <button class="icon-label-button" on:click={openNewConnection}><Icon name="plus" size={15} /> Add</button>
          </div>
          {#if connections.length === 0}
            <div class="empty-state">
              <span class="empty-state-icon"><Icon name="database" size={24} /></span>
              <strong>No connections yet</strong>
              <p>Add a PostgreSQL connection to start querying.</p>
              <button class="primary" on:click={openNewConnection}><Icon name="plus" size={15} /> Add connection</button>
            </div>
          {/if}
          {#each connections as connection (connection.id)}
            <article class:active-card={connection.id === tabConnectionId} class="connection-card">
              <div class="connection-card-head">
                <strong>{connection.name}</strong>
                {#if connection.id === tabConnectionId}<span class="active-badge"><span class="status-dot"></span> This tab</span>{/if}
              </div>
              <span class="connection-address">{connection.username}@{connection.host}:{connection.port}/{connection.database}</span>
              <div class="row-actions">
                <button disabled={connection.id === tabConnectionId || running} on:click={() => activateConnection(connection.id)}>
                  <Icon name="check" size={14} /> {connection.id === tabConnectionId ? 'In use' : 'Use in tab'}
                </button>
                <button disabled={running} on:click={() => editConnection(connection)}><Icon name="edit" size={14} /> Edit</button>
                <button disabled={running} class="danger-quiet" aria-label={`Delete ${connection.name}`} title={`Delete ${connection.name}`} on:click={() => deleteConnection(connection.id)}>
                  <Icon name="trash" size={14} />
                </button>
              </div>
            </article>
          {/each}
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
            <input placeholder="Tables, columns, types…" bind:value={schemaSearch} />
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
                        <span>{column.name}{column.primaryKey ? ' PK' : ''}{column.generated ? ' GEN' : ''}</span>
                        <small>{column.typeName}{column.nullable ? '' : ' not null'}</small>
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
            <button class="danger-quiet icon-label-button" on:click={purgeHistory}><Icon name="trash" size={14} /> Purge</button>
          </div>
          <label class="search-field" aria-label="Search query history">
            <Icon name="search" size={15} />
            <input placeholder="Search query history…" bind:value={historySearch} on:input={queueHistorySearch} />
            {#if historySearch}<button aria-label="Clear history search" on:click={() => { historySearch = ''; void refreshHistory(); }}><Icon name="close" size={13} /></button>{/if}
          </label>
          {#if history.length === 0}
            <div class="empty-state compact"><strong>No query history</strong><p>Executed queries will appear here.</p></div>
          {/if}
          {#each history as entry (entry.id)}
            <article class:error-card={entry.isError} class="history-card">
              <div class="history-meta">
                <span class:error-status={entry.isError} class="history-status">{entry.isError ? 'Error' : 'Success'}</span>
                <time title={entry.executedAt}>{formatHistoryTime(entry.executedAt)}</time>
              </div>
              <code>{entry.query}</code>
              <small>{entry.durationMs} ms{entry.rowCount != null ? ` · ${entry.rowCount} rows` : ''}</small>
              <small class="history-connection">{connections.find((connection) => connection.id === entry.connectionId)?.name ?? 'Unavailable connection'}</small>
              {#if entry.errorMessage}<small class="history-error">{entry.errorMessage}</small>{/if}
              <div class="row-actions">
                <button on:click={() => restoreHistory(entry)}><Icon name="plus" size={14} /> New query</button>
                <button disabled={!entry.hasResult} on:click={() => openHistory(entry)}>Open result</button>
                <button class="danger-quiet" aria-label="Delete history entry" title="Delete history entry" on:click={() => deleteHistory(entry.id)}><Icon name="trash" size={14} /></button>
              </div>
            </article>
          {/each}
          {#if history.length < historyTotal}<button class="history-more" disabled={historyLoading} on:click={() => refreshHistory(true)}>{historyLoading ? 'Loading…' : `Load more (${history.length} of ${historyTotal})`}</button>{/if}
        {:else}
          <div class="panel-head"><div><h2>Settings</h2><p>Workspace preferences</p></div></div>
          <section class="settings-card">
            <div class="settings-card-head"><strong>Connection pool</strong><span>{settings.poolSize} connections</span></div>
            <label>
              Maximum connections
              <div class="inline-control">
                <input type="number" min="1" max="32" bind:value={poolSizeInput} />
                <button on:click={savePoolSize}>Save</button>
              </div>
            </label>
            <p>Choose between 1 and 32 concurrent PostgreSQL connections.</p>
            <p>{poolStats ? `${poolStats.active} active · ${poolStats.idle} idle` : 'Pool statistics are available for the active database.'}</p>
          </section>
          <section class="settings-card">
            <div class="settings-card-head"><strong>AI completion</strong><span class="ai-{aiHealth.state}">{aiHealth.state}</span></div>
            <label>
              OpenCode model
              <Dropdown
                value={settings.aiModel ?? ''}
                options={aiModelOptions}
                ariaLabel="OpenCode model"
                onSelect={(model) => void saveAiModel(model)}
              />
            </label>
            <p>{aiHealth.state === 'on' ? 'Inline SQL suggestions are available.' : 'OpenCode is unavailable. Start it to enable AI suggestions.'}</p>
            {#if aiHealth.message}<details class="technical-detail"><summary>Connection details</summary><p>{aiHealth.message}</p></details>{/if}
          </section>
        {/if}
      </aside>
    {/if}

    <section class="main-pane" class:resizing={resizingResults} style={`--results-height: ${resultsHeight}px`}>
      <nav class="tabs" aria-label="Query tabs">
        {#each tabs as tab (tab.id)}
          <div class:active={activeTab === tab.id} class="tab-chip">
            {#if renamingTab === tab.id}
              <form class="tab-rename" on:submit|preventDefault={finishRename}><input aria-label="Tab name" bind:value={renameValue} on:keydown={(event) => { if (event.key === 'Escape') renamingTab = ''; }} /><button type="submit" aria-label="Save tab name"><Icon name="check" size={13} /></button></form>
            {:else}
              <button class="tab-label" title={`${tab.title} · ${connections.find((connection) => connection.id === (tab.id === activeTab ? tabConnectionId : tab.connectionId))?.name ?? 'No connection'}`} on:click={() => switchTab(tab.id)} on:dblclick={() => startRename(tab.id)}>
                {#if operation?.tabId === tab.id}<span class="spinner"></span>{/if}
                {tab.title}{#if (tab.id === activeTab ? pendingChanges : tab.pendingChanges).length}<span class="pending-dot" title="Staged edits">•</span>{/if}
                {#if tab.kind === 'reference'}<small>read only</small>{/if}
              </button>
              <button class="tab-close" aria-label={`Close ${tab.title}`} on:click={() => closeTab(tab.id)}><Icon name="close" size={13} /></button>
            {/if}
          </div>
        {/each}
        <button class="new-tab" aria-label="New query tab" title="New query tab (Cmd/Ctrl+T)" on:click={() => newQuery()}><Icon name="plus" size={16} /></button>
      </nav>

      <div class="editor-pane">
        {#if txStatus.active}
          <div class="tx-banner">
            <strong>Transaction active</strong>
            <span>{txStatus.startedAt}</span>
            <button on:click={commitTransaction}>Commit</button>
            <button on:click={rollbackTransaction}>Rollback</button>
          </div>
        {/if}
        {#if workspaceLoaded}
        {#key activeTab}
          <SqlEditor
            bind:this={editorRef}
            bind:value={sql}
            readOnly={tabKind !== 'query'}
            viewState={editorViewState}
            aiEnabled={aiHealth.state === 'on' && tabKind === 'query'}
            aiModel={settings.aiModel}
            {schemaContext}
            {schemaItems}
            aiComplete={api.aiComplete}
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
            {#if operation?.tabId === activeTab}
              <span class="running-state"><span class="spinner"></span> {operation.cancelling ? 'Cancelling…' : 'Running'} {runningDuration()}</span>
            {:else if result}
              <span>{result.rowCount}{result.hasMore ? '+' : ''} {result.rowCount === 1 ? 'row' : 'rows'} · {result.durationMs} ms</span>
            {:else}
              <span>No query run</span>
            {/if}
          </div>
          <div class="results-actions">
            <ActionMenu label="Export" ariaLabel="Export results" disabled={!result?.rows.length} items={exportItems} onSelect={handleExport} />
            {#if editMode}
              <button disabled={!pendingChanges.length || running} on:click={previewUpdates}>Review {pendingChanges.length} edits</button>
              <button disabled={running} on:click={discardUpdates}>Discard</button>
              <button disabled={running} on:click={disableEditMode}>Done</button>
            {:else}
              <button disabled={!result?.rows.length || !resultUsable || running} on:click={enableEditMode}><Icon name="edit" size={14} /> Edit cells</button>
            {/if}
          </div>
        </div>
        {#if resultSession}
          <div class="result-context">
            <span>{resultSession.connectionName}</span>
            <time title={resultSession.executedAt}>{formatHistoryTime(resultSession.executedAt)}</time>
            {#if resultSession.snapshot}<span class="snapshot-badge">History snapshot · read only</span>{/if}
            {#if !resultSession.snapshot && resultSession.connectionId !== tabConnectionId}<span class="context-warning">Different connection · run the query to refresh</span>{/if}
            <details><summary>Executed SQL</summary><pre>{resultSession.execution.sql}</pre></details>
          </div>
        {/if}
        {#if editMode}
          <div class="edit-bar">
            {pendingChanges.length} pending changes.
            {#if detectEditTarget()}
              Apply will update {detectEditTarget()?.schema}.{detectEditTarget()?.table}.
            {:else}
              Preview only: a single source table and primary key must be present to apply edits.
            {/if}
          </div>
        {/if}
        {#if queryError}
          <div class="query-error">
            <strong>{queryError.message}</strong>
            {#if queryError.detail}<span>Detail: {queryError.detail}</span>{/if}
            {#if queryError.hint}<span>Hint: {queryError.hint}</span>{/if}
            {#if queryError.code}<span>Code: {queryError.code}</span>{/if}
            {#if queryError.position}<span>Position: {queryError.position}</span>{/if}
          </div>
        {/if}
        {#if result && result.rows.length}
          {#key `${activeTab}:${resultSession?.executedAt}`}
            <ResultsGrid {result} {editMode} readOnly={!resultUsable} disabled={running} changedCells={pendingChanges} onEdit={updateCell} onMessage={flash} />
          {/key}
          <footer class="results-foot">
            <span>{result.rows.length} loaded {result.hasMore ? '· more available' : ''}</span>
            {#if result.hasMore && !resultSession?.snapshot}<button disabled={!resultUsable || running || pendingChanges.length > 0} on:click={loadMoreRows}>Load next {PAGE_SIZE} rows</button>{/if}
          </footer>
        {:else if result}
          <p class="empty">Query completed in {result.durationMs} ms with no rows.</p>
        {:else}
          <div class="empty-state results-empty">
            <span class="empty-state-icon"><Icon name="play" size={22} /></span>
            <strong>{tabKind === 'query' ? 'Run a query to see results' : 'SQL reference'}</strong>
            {#if tabKind === 'query'}<p>Place the cursor in a statement and press <kbd>⌘↵</kbd>.</p>{:else}<button on:click={() => newQuery(sql, tabConnectionId)}>Open in new query tab</button>{/if}
          </div>
        {/if}
      </section>
    </section>
  </section>

  <footer class="status-bar">
    <span class="status-connection"><span class:connected={Boolean(activeConnection)} class="status-dot"></span>{activeConnection ? `${activeConnection.host}/${activeConnection.database}` : 'No connection'}</span>
    <span>{schemaItems.length ? `${schemaItems.length} schema objects` : schemaLoading ? 'Loading schema…' : ''}</span>
    <span>{workspaceLoaded ? 'Drafts saved automatically' : 'Restoring workspace…'}</span>
    {#if running}<span>Running {runningDuration()}</span>{/if}
    <span class="status-flash">{statusFlash}</span>
  </footer>
</main>

{#if toastVisible}
  <div class="toast" class:error-toast={toastError} role="status">
    <span class="toast-icon"><Icon name={toastError ? 'close' : 'check'} size={15} /></span>
    <span>{statusFlash}</span>
    <button aria-label="Dismiss message" on:click={() => (toastVisible = false)}><Icon name="close" size={14} /></button>
  </div>
{/if}

{#if showUpdatePreview}
  <Modal label="Review cell edits" locked={running} onClose={() => (showUpdatePreview = false)}>
    <div class="modal update-review">
      <div class="panel-head"><h2>Review cell edits</h2><button disabled={running} on:click={() => (showUpdatePreview = false)}>Close</button></div>
      <p>{pendingChanges.length} staged changes on {resultSession?.connectionName}. Each updated row must match its original values.</p>
      <pre>{generateUpdateSql()}</pre>
      {#if !detectEditTarget()}<p class="context-warning">Apply is unavailable without an unambiguous source table and primary key.</p>{/if}
      <div class="row-actions"><button disabled={running} on:click={() => (showUpdatePreview = false)}>Back to results</button><button class="primary" disabled={!canApplyUpdates()} on:click={applyUpdates}>{running ? 'Applying…' : 'Apply changes'}</button></div>
    </div>
  </Modal>
{/if}

{#if showConnectionModal}
  <Modal label={connectionForm.id ? 'Edit connection' : 'New connection'} locked={savingConnection} onClose={() => (showConnectionModal = false)}>
    <form class="modal" on:submit|preventDefault={saveConnection}>
      <div class="panel-head">
        <h2>{connectionForm.id ? 'Edit connection' : 'New connection'}</h2>
        <button type="button" on:click={() => (showConnectionModal = false)}>Close</button>
      </div>
      <label>Name <input bind:value={connectionForm.name} on:input={queueConnectionUrlSync} required /></label>
      <label>Host <input bind:value={connectionForm.host} on:input={queueConnectionUrlSync} required /></label>
      <div class="form-grid">
        <label>Port <input type="number" bind:value={connectionForm.port} on:input={queueConnectionUrlSync} required /></label>
        <label>Database <input bind:value={connectionForm.database} on:input={queueConnectionUrlSync} required /></label>
      </div>
      <label>Username <input bind:value={connectionForm.username} on:input={queueConnectionUrlSync} required /></label>
      <label>Password <input type="password" bind:value={connectionForm.password} on:input={queueConnectionUrlSync} /></label>
      <label class="checkbox"><input type="checkbox" bind:checked={connectionForm.ssl} on:change={queueConnectionUrlSync} /> Use SSL</label>
      <label>Connection URL <input bind:value={connectionUrl} /></label>
      <div class="row-actions">
        <button type="button" on:click={importConnectionUrl}>Import URL</button>
        <button type="button" on:click={testConnection} disabled={testingConnection}>{testingConnection ? 'Testing...' : 'Test'}</button>
        <button class="primary" disabled={savingConnection}>{savingConnection ? 'Saving...' : 'Save'}</button>
      </div>
      {#if modalFeedback}<p class="muted">{modalFeedback}</p>{/if}
    </form>
  </Modal>
{/if}
