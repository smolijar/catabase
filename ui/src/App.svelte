<script lang="ts">
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';
  import { save } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import Dropdown from './components/Dropdown.svelte';
  import Icon from './components/Icon.svelte';
  import SqlEditor from './components/SqlEditor.svelte';
  import { serializeRows } from './lib/export';
  import { api, errorMessage } from './lib/ipc';
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
  const EXPORT_OPTIONS = [
    { value: 'csv', label: 'CSV' },
    { value: 'tsv', label: 'TSV' },
    { value: 'json', label: 'JSON' }
  ];

  type SidebarTab = 'connections' | 'schema' | 'history' | 'settings';
  type ReadonlyTab = { id: string; title: string; content: string };
  type EditorHandle = {
    getExecutionTarget: (splitSql: typeof api.splitSql) => Promise<ExecutionTarget>;
    clearHighlights: () => void;
    markError: (offset: number, message: string) => void;
  };
  type PendingChange = {
    rowIndex: number;
    column: string;
    oldValue: unknown;
    newValue: unknown;
  };
  type EditTarget = {
    schema: string;
    table: string;
    whereColumns: ColumnMeta[];
    warning?: string;
  };
  type SchemaGroup = { name: string; items: SchemaItem[] };

  let sidebar: SidebarTab = 'connections';
  let sidebarOpen = true;
  let sql = DEFAULT_SQL;
  let activeTab = 'playground';
  let readonlyTabs: ReadonlyTab[] = [];
  let readonlyContent = '';
  let editorRef: EditorHandle | undefined;
  let running = false;
  let runningStartedAt = 0;
  let nowMs = Date.now();
  let queryToken: string | null = null;
  let result: QueryResult | null = null;
  let originalRows: Record<string, unknown>[] = [];
  let queryError: AppError | null = null;
  let lastExecution: ExecutionTarget | null = null;
  let statusFlash = 'Ready';
  let toastVisible = false;
  let toastTimer: ReturnType<typeof setTimeout> | null = null;
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
  let historyTimer: ReturnType<typeof setTimeout> | null = null;
  let exportFormat: 'csv' | 'tsv' | 'json' = 'csv';
  let resultsHeight = 292;
  let resizingResults = false;

  let showConnectionModal = false;
  let connectionForm: ConnectionInput = emptyConnection();
  let connectionUrl = '';
  let modalFeedback = '';
  let testingConnection = false;
  let savingConnection = false;
  let draftTimer: ReturnType<typeof setTimeout> | null = null;
  let urlTimer: ReturnType<typeof setTimeout> | null = null;

  $: activeConnection = connections.find((connection) => connection.isActive) ?? null;
  $: readonlyContent = readonlyTabs.find((tab) => tab.id === activeTab)?.content ?? '';
  $: schemaContext = schemaItems
    .map((item) => `${item.schema}.${item.name}(${item.columns.map((column) => column.name).join(', ')})`)
    .join('\n');
  $: schemaGroups = groupSchemaItems(schemaItems, schemaSearch);
  $: aiModelOptions = [
    { value: '', label: 'Default' },
    ...aiModels.map((model) => ({ value: model, label: model }))
  ];
  $: if (mounted && activeTab === 'playground') scheduleDraftSave();

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
      stopResultsResize();
    };
  });

  async function bootstrap() {
    await Promise.allSettled([
      loadSettings(),
      loadConnections(),
      loadDraft(),
      refreshHistory(),
      checkAiHealth(),
      refreshTxStatus()
    ]);
    if (connections.some((connection) => connection.isActive)) void refreshSchema();
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
      const draft = await api.loadDraft('playground');
      if (draft && (!sql.trim() || sql === DEFAULT_SQL)) sql = draft;
    } catch {
      // Browser-only dev mode has no Tauri IPC; keep seeded SQL.
    }
  }

  function scheduleDraftSave() {
    if (draftTimer) clearTimeout(draftTimer);
    draftTimer = setTimeout(() => void persistDraftNow(), 1200);
  }

  function persistDraftNow() {
    if (!mounted) return;
    void api.saveDraft('playground', sql).catch(() => undefined);
  }

  async function runQuery() {
    if (running) {
      await cancelQuery();
      return;
    }
    if (activeTab !== 'playground') {
      flash('Switch to Playground to run SQL.');
      return;
    }
    const target = editorRef ? await editorRef.getExecutionTarget(api.splitSql) : { sql: sql.trim(), start: 0, end: sql.length };
    if (!target.sql.trim()) {
      queryError = { message: 'No query to execute.' };
      result = null;
      return;
    }

    running = true;
    runningStartedAt = Date.now();
    nowMs = runningStartedAt;
    queryError = null;
    queryToken = crypto.randomUUID();
    statusFlash = 'Running query...';
    lastExecution = target;

    try {
      void loadConnectionStats();
      const next = await api.executeQuery(target.sql, queryToken, 0, PAGE_SIZE);
      setResult(next);
      flash(`Loaded ${next.rowCount}${next.hasMore ? '+' : ''} rows in ${next.durationMs} ms.`);
      void refreshHistory();
    } catch (error) {
      queryError = normalizeAppError(error);
      setResult(null);
      if (queryError.position && editorRef) {
        editorRef.markError(target.start + queryError.position - 1, queryError.message);
      }
      flash(queryError.message);
      void refreshHistory();
    } finally {
      running = false;
      queryToken = null;
      void loadConnectionStats();
      void refreshTxStatus();
    }
  }

  async function cancelQuery() {
    const token = queryToken;
    running = false;
    queryToken = null;
    if (token) await api.cancelQuery(token).catch(() => undefined);
    void loadConnectionStats();
    editorRef?.clearHighlights();
    flash('Cancelled locally. Server query may still finish in background.');
  }

  async function loadMoreRows() {
    if (!result || !lastExecution || running) return;
    running = true;
    runningStartedAt = Date.now();
    nowMs = runningStartedAt;
    const token = crypto.randomUUID();
    try {
      const next = await api.executeQuery(lastExecution.sql, token, result.rows.length, PAGE_SIZE);
      result = {
        ...next,
        rows: [...result.rows, ...next.rows],
        rowCount: result.rows.length + next.rows.length
      };
      flash(`Loaded ${result.rowCount}${next.hasMore ? '+' : ''} rows.`);
    } catch (error) {
      flash(`Load more failed: ${errorMessage(error)}`);
    } finally {
      running = false;
      void loadConnectionStats();
    }
  }

  function clearEditor() {
    if (activeTab !== 'playground') {
      flash('Switch to Playground to clear SQL.');
      return;
    }
    if (sql.trim() && !confirm('Clear the SQL editor? This cannot be undone.')) return;
    sql = '';
    flash('Editor cleared.');
  }

  function clearResults() {
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
    savingConnection = true;
    try {
      const saved = await withTimeout(api.saveConnection(connectionForm), 12000, 'Saving connection timed out.');
      showConnectionModal = false;
      await loadConnections();
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
    try {
      const active = await api.activateConnection(id);
      await loadConnections();
      flash(`Using ${active.name}.`);
      void refreshTxStatus();
      void refreshSchema();
    } catch (error) {
      flash(errorMessage(error));
    }
  }

  async function deleteConnection(id: string) {
    if (!confirm('Delete this connection?')) return;
    try {
      await api.deleteConnection(id);
      await loadConnections();
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

  async function refreshSchema(retries = 0) {
    schemaLoading = true;
    schemaError = '';
    try {
      const tree = await api.introspectSchema();
      schemaItems = tree.schemas;
      if (!schemaItems.length && retries < 3) {
        setTimeout(() => void refreshSchema(retries + 1), 3000);
      }
    } catch (error) {
      schemaError = errorMessage(error);
      if (retries < 3) setTimeout(() => void refreshSchema(retries + 1), 3000);
    } finally {
      schemaLoading = false;
    }
  }

  async function openDdl(item: SchemaItem) {
    try {
      const ddl = await api.getDdl(item.schema, item.name);
      openReadonlyTab(`ddl:${item.schema}.${item.name}`, `${item.schema}.${item.name}`, ddl);
    } catch (error) {
      flash(errorMessage(error));
    }
  }

  function openReadonlyTab(id: string, title: string, content: string) {
    const existing = readonlyTabs.find((tab) => tab.id === id);
    if (existing) existing.content = content;
    else readonlyTabs = [...readonlyTabs, { id, title, content }];
    activeTab = id;
  }

  function closeReadonlyTab(id: string) {
    readonlyTabs = readonlyTabs.filter((tab) => tab.id !== id);
    if (activeTab === id) activeTab = 'playground';
  }

  function restoreHistory(entry: HistoryEntry) {
    sql = entry.query;
    activeTab = 'playground';
    flash('History query restored.');
  }

  async function openHistory(entry: HistoryEntry) {
    try {
      const payload = await api.historyGetResult(entry.id);
      if (!payload.result) {
        flash('No stored result for this entry.');
        return;
      }
      result = payload.result;
      originalRows = cloneRows(result.rows);
      pendingChanges = [];
      editMode = false;
      openReadonlyTab(`history:${entry.id}`, `History #${entry.id}`, entry.query);
    } catch (error) {
      flash(errorMessage(error));
    }
  }

  function queueHistorySearch() {
    if (historyTimer) clearTimeout(historyTimer);
    historyTimer = setTimeout(() => void refreshHistory(), 300);
  }

  async function refreshHistory() {
    try {
      const page = await api.historyPage(historySearch || null, 0, 50);
      history = page.entries;
      historyStats = await api.historyStats();
    } catch {
      history = [];
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
    if (full && !lastExecution) {
      flash('No query context for full export.');
      return;
    }

    try {
      let exportResult = result;
      if (full && lastExecution) {
        flash('Preparing full export…');
        const token = crypto.randomUUID();
        exportResult = await api.executeQuery(lastExecution.sql, token, 0, 100_000);
      }

      const path = await save({ defaultPath: `catabase-${timestamp()}.${format}` });
      if (!path) return;
      await api.exportWriteFile(path, serializeRows(exportResult, format));
      flash(`${format.toUpperCase()} saved.`);
    } catch (error) {
      flash(`Export failed: ${errorMessage(error)}`);
    }
  }

  function enableEditMode() {
    if (!result || !result.rows.length) {
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

  function editCell(rowIndex: number, columnName: string) {
    if (!editMode || !result) return;
    const column = result.columns.find((candidate) => candidate.name === columnName);
    if (!column) return;
    if (column.generated) {
      flash('Generated columns cannot be edited.');
      return;
    }

    const current = result.rows[rowIndex][columnName];
    if (column.typeName === 'bool') {
      const next = current === true ? false : current === false && column.nullable ? null : true;
      updateCell(rowIndex, columnName, next);
      return;
    }

    const next = prompt(`New value for ${columnName}`, current == null ? '' : String(current));
    if (next === null) return;
    if (isNumericColumn(column.typeName) && next !== '' && Number.isNaN(Number(next))) {
      flash('Invalid numeric value.');
      return;
    }
    updateCell(rowIndex, columnName, next === '' ? null : coerceValue(next, column.typeName));
  }

  function updateCell(rowIndex: number, column: string, newValue: unknown) {
    if (!result) return;
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
    openReadonlyTab('update-preview', 'Update preview', generateUpdateSql());
  }

  async function applyUpdates() {
    if (!pendingChanges.length) {
      flash('No pending changes.');
      return;
    }
    const target = detectEditTarget();
    if (!target) {
      flash('Apply disabled: source table could not be detected. Use Preview SQL.');
      return;
    }

    const token = crypto.randomUUID();
    try {
      running = true;
      await api.executeQuery(generateUpdateSql(), token, 0, 1);
      originalRows = result ? cloneRows(result.rows) : [];
      pendingChanges = [];
      editMode = false;
      flash(`Applied updates to ${target.schema}.${target.table}.`);
      void refreshHistory();
    } catch (error) {
      flash(`Apply failed: ${errorMessage(error)}`);
    } finally {
      running = false;
    }
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
    if (!result || !result.columns.length) return null;
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
    if (primaryKeyColumns.length) return { schema, table, whereColumns: primaryKeyColumns };

    return {
      schema,
      table,
      whereColumns: tableColumns.filter((column) => !column.generated),
      warning: 'No primary key returned; WHERE uses all original result columns.'
    };
  }

  function setResult(next: QueryResult | null) {
    result = next;
    originalRows = next ? cloneRows(next.rows) : [];
    pendingChanges = [];
    editMode = false;
  }

  function isChanged(rowIndex: number, column: string): boolean {
    return pendingChanges.some((change) => change.rowIndex === rowIndex && change.column === column);
  }

  function canApplyUpdates(): boolean {
    return pendingChanges.length > 0 && detectEditTarget() !== null;
  }

  function normalizeAppError(error: unknown): AppError {
    const appError = error as AppError;
    return { ...appError, message: appError?.message ?? errorMessage(error) };
  }

  function flash(message: string) {
    statusFlash = message;
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

  function isNumericColumn(typeName: string): boolean {
    return ['int2', 'int4', 'int8', 'float4', 'float8', 'numeric'].includes(typeName);
  }

  function columnByName(name: string): ColumnMeta | undefined {
    return result?.columns.find((column) => column.name === name);
  }

  function sourceColumnName(column: ColumnMeta | undefined): string {
    return column?.sourceName || column?.name || '';
  }

  function coerceValue(value: string, typeName: string): unknown {
    if (isNumericColumn(typeName)) return Number(value);
    return value;
  }

  function sqlLiteral(value: unknown): string {
    if (value === null || value === undefined) return 'NULL';
    if (typeof value === 'number' || typeof value === 'boolean') return String(value);
    return `E'${String(value).replaceAll('\\', '\\\\').replaceAll("'", "\\'")}'`;
  }

  function quoteIdent(value: string): string {
    return `"${value.replaceAll('"', '""')}"`;
  }

  function timestamp(): string {
    return new Date().toISOString().replaceAll(':', '-').replaceAll('.', '-');
  }

  function runningDuration(): string {
    if (!runningStartedAt) return '0.0s';
    return `${((nowMs - runningStartedAt) / 1000).toFixed(1)}s`;
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

  function formatCell(value: unknown): string {
    if (value === null || value === undefined) return 'NULL';
    if (typeof value === 'object') return JSON.stringify(value);
    return String(value);
  }
</script>

<main class="app-shell">
  <header class="toolbar">
    <div class="brand">
      <span class="brand-mark">C</span>
      <div>
        <strong>Catabase</strong>
        <small>PostgreSQL query workspace</small>
      </div>
    </div>
    <button
      class="primary run-button"
      class:cancel-button={running}
      on:click={runQuery}
      disabled={!activeConnection && !running}
      title={activeConnection ? 'Run current statement (Cmd/Ctrl+Enter)' : 'Add or activate a connection first'}
    >
      <Icon name={running ? 'stop' : 'play'} size={16} />
      <span>{running ? 'Cancel' : 'Run'}</span>
      <kbd>⌘↵</kbd>
    </button>
    <button class="toolbar-action" on:click={clearEditor} title="Clear SQL editor">
      <Icon name="clear" size={16} /><span>Clear SQL</span>
    </button>
    <button class="toolbar-action" on:click={clearResults} title="Clear query results">
      <Icon name="close" size={16} /><span>Clear results</span>
    </button>
    <div class="toolbar-spacer"></div>
    <button
      class="chip connection-chip"
      class:connected={Boolean(activeConnection)}
      on:click={() => selectSidebar('connections')}
      title="Show connections"
    >
      <span class="status-dot"></span>
      {activeConnection ? activeConnection.name : 'No connection'}
    </button>
    <span class="chip">{poolStats ? `${poolStats.active} active / ${poolStats.idle} idle` : `Pool ${settings.poolSize}`}</span>
    {#if running}<span class="chip running-chip">Running {runningDuration()}</span>{/if}
    <button class="chip ai-{aiHealth.state}" on:click={() => selectSidebar('settings')} title={aiHealth.message ?? 'AI status'}>
      <Icon name="sparkles" size={14} /> AI {aiHealth.state}
    </button>
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
            <article class:active-card={connection.isActive} class="connection-card">
              <div class="connection-card-head">
                <strong>{connection.name}</strong>
                {#if connection.isActive}<span class="active-badge"><span class="status-dot"></span> Active</span>{/if}
              </div>
              <span class="connection-address">{connection.username}@{connection.host}:{connection.port}/{connection.database}</span>
              <div class="row-actions">
                <button disabled={connection.isActive} on:click={() => activateConnection(connection.id)}>
                  <Icon name="check" size={14} /> {connection.isActive ? 'In use' : 'Use'}
                </button>
                <button on:click={() => editConnection(connection)}><Icon name="edit" size={14} /> Edit</button>
                <button class="danger-quiet" aria-label={`Delete ${connection.name}`} title={`Delete ${connection.name}`} on:click={() => deleteConnection(connection.id)}>
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
                    <button class="ddl-button" on:click={() => openDdl(item)}>Open DDL</button>
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
              {#if entry.errorMessage}<small class="history-error">{entry.errorMessage}</small>{/if}
              <div class="row-actions">
                <button on:click={() => restoreHistory(entry)}><Icon name="refresh" size={14} /> Restore</button>
                <button disabled={!entry.hasResult} on:click={() => openHistory(entry)}>Open result</button>
                <button class="danger-quiet" aria-label="Delete history entry" title="Delete history entry" on:click={() => deleteHistory(entry.id)}><Icon name="trash" size={14} /></button>
              </div>
            </article>
          {/each}
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
            <p>{aiHealth.message ?? 'Connect OpenCode to enable inline SQL suggestions.'}</p>
          </section>
        {/if}
      </aside>
    {/if}

    <section class="main-pane" class:resizing={resizingResults} style={`--results-height: ${resultsHeight}px`}>
      <nav class="tabs">
        <button class:active={activeTab === 'playground'} on:click={() => (activeTab = 'playground')}>Playground</button>
        {#each readonlyTabs as tab (tab.id)}
          <div class:active={activeTab === tab.id} class="tab-chip">
            <button class="tab-label" on:click={() => (activeTab = tab.id)}>{tab.title}</button>
            <button class="tab-close" aria-label={`Close ${tab.title}`} on:click={() => closeReadonlyTab(tab.id)}>x</button>
          </div>
        {/each}
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
        {#if activeTab === 'playground'}
          <SqlEditor
            bind:this={editorRef}
            bind:value={sql}
            readOnly={false}
            aiEnabled={aiHealth.state === 'on'}
            aiModel={settings.aiModel}
            {schemaContext}
            {schemaItems}
            aiComplete={api.aiComplete}
          />
        {:else}
          <SqlEditor value={readonlyContent} readOnly={true} />
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
            {#if running}
              <span class="running-state"><span class="spinner"></span> Running {runningDuration()}</span>
            {:else if result}
              <span>{result.rowCount}{result.hasMore ? '+' : ''} rows · {result.durationMs} ms</span>
            {:else}
              <span>No query run</span>
            {/if}
          </div>
          <div class="results-actions">
            <Dropdown
              value={exportFormat}
              options={EXPORT_OPTIONS}
              ariaLabel="Export format"
              disabled={!result?.rows.length}
              compact
              onSelect={(format) => (exportFormat = format as typeof exportFormat)}
            />
            <button disabled={!result?.rows.length} on:click={() => copyResult(exportFormat)}><Icon name="copy" size={14} /> Copy</button>
            <button disabled={!result?.rows.length} on:click={() => downloadResult(exportFormat)}><Icon name="download" size={14} /> Save</button>
            <button disabled={!result?.rows.length || !lastExecution} on:click={() => downloadResult(exportFormat, true)} title="Re-run the query and export up to 10,000 rows">Up to 10k</button>
            {#if editMode}
              <button on:click={previewUpdates}>Preview SQL</button>
              <button disabled={!canApplyUpdates()} on:click={applyUpdates}>Apply</button>
              <button on:click={discardUpdates}>Discard</button>
              <button on:click={disableEditMode}>Stop Edit</button>
            {:else}
              <button disabled={!result?.rows.length} on:click={enableEditMode}><Icon name="edit" size={14} /> Edit</button>
            {/if}
          </div>
        </div>
        {#if editMode}
          <div class="edit-bar">
            {pendingChanges.length} pending changes.
            {#if detectEditTarget()}
              Apply will update {detectEditTarget()?.schema}.{detectEditTarget()?.table}.
            {:else}
              Apply disabled: source table ambiguous or unavailable.
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
        {:else if result && result.rows.length}
          <div class="table-wrap">
            <table>
              <thead>
                <tr>{#each result.columns as column (column.name)}<th>{column.name}<small>{column.typeName}</small></th>{/each}</tr>
              </thead>
              <tbody>
                {#each result.rows as row, index (index)}
                  <tr>
                    {#each result.columns as column (column.name)}
                      <td
                        role="button"
                        tabindex={editMode ? 0 : -1}
                        class:editable-cell={editMode}
                        class:changed-cell={isChanged(index, column.name)}
                        on:click={() => editCell(index, column.name)}
                        on:keydown={(event) => {
                          if (event.key === 'Enter' || event.key === ' ') editCell(index, column.name);
                        }}
                      >{formatCell(row[column.name])}</td>
                    {/each}
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
          <footer class="results-foot">
            {result.rowCount}{result.hasMore ? '+' : ''} rows · {result.durationMs} ms {result.autoLimited ? '· auto-limited' : ''}
            {#if result.hasMore}<button on:click={loadMoreRows}>Load more rows</button>{/if}
          </footer>
        {:else if result}
          <p class="empty">Query completed in {result.durationMs} ms with no rows.</p>
        {:else}
          <div class="empty-state results-empty">
            <span class="empty-state-icon"><Icon name="play" size={22} /></span>
            <strong>Run a query to see results</strong>
            <p>Place the cursor in a statement and press <kbd>⌘↵</kbd>.</p>
          </div>
        {/if}
      </section>
    </section>
  </section>

  <footer class="status-bar">
    <span class="status-connection"><span class:connected={Boolean(activeConnection)} class="status-dot"></span>{activeConnection ? `${activeConnection.host}/${activeConnection.database}` : 'No connection'}</span>
    <span>{poolStats ? `Pool ${settings.poolSize}: ${poolStats.active} active, ${poolStats.idle} idle` : `Pool ${settings.poolSize}`}</span>
    <span>Schema {schemaItems.length ? `${schemaItems.length} objects` : 'not loaded'}</span>
    {#if running}<span>Running {runningDuration()}</span>{/if}
    <span class="status-flash">{statusFlash}</span>
  </footer>
</main>

{#if toastVisible}
  <div class="toast" role="status">
    <span class="toast-icon"><Icon name="check" size={15} /></span>
    <span>{statusFlash}</span>
    <button aria-label="Dismiss message" on:click={() => (toastVisible = false)}><Icon name="close" size={14} /></button>
  </div>
{/if}

{#if showConnectionModal}
  <div class="modal-backdrop">
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
  </div>
{/if}
