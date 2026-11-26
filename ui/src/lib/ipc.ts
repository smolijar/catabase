import { invoke } from '@tauri-apps/api/core';
import type { UpdaterStatus, UpdateCheck } from './appUpdates';
import type { ExplainRequest, ExplainResult, ShareExplainRequest } from './explain';
import type { DiagnosticReport } from './diagnostics';
import type { ExportFileRequest } from './export';
import type {
  SupermavenStatus,
  SupermavenRequest,
  SupermavenCompletion,
  AppError,
  ApplyUpdatesResult,
  CancelResult,
  CommitMode,
  ConnectionFields,
  ConnectionInput,
  ConnectionRecord,
  ConnectionStats,
  HistoryEntry,
  HistoryPage,
  HistoryStats,
  HistoryStorageStats,
  ForeignKeyLookup,
  ForeignKeyValues,
  QueryExecution,
  QueryResult,
  RowUpdate,
  SchemaTree,
  Settings,
  StatementRange,
  TxStatus
} from './types';

export const api = {
  diagnosticsReport: (currentSession = false) => invoke<DiagnosticReport>('diagnostics_report', { currentSession }),
  acknowledgeDiagnostics: (id: string) => invoke<void>('acknowledge_diagnostics', { id }),
  openDiagnosticIssue: (title: string, body: string) => invoke<void>('open_diagnostic_issue', { title, body }),
  updaterStatus: () => invoke<UpdaterStatus>('updater_status'),
  checkForUpdate: () => invoke<UpdateCheck>('check_for_update'),
  installUpdate: (version: string) => invoke<void>('install_update', { version }),
  openReleases: () => invoke<void>('open_releases'),
  openReleaseLink: (url: string) => invoke<void>('open_release_link', { url }),
  listConnections: () => invoke<ConnectionRecord[]>('list_connections'),
  saveConnection: (input: ConnectionInput) => invoke<ConnectionRecord>('save_connection', { input }),
  duplicateConnection: (id: string) => invoke<ConnectionRecord>('duplicate_connection', { id }),
  deleteConnection: (id: string) => invoke<void>('delete_connection', { id }),
  activateConnection: (id: string) => invoke<ConnectionRecord>('activate_connection', { id }),
  disconnectConnection: (id: string) => invoke<void>('disconnect_connection', { id }),
  testConnection: (input: ConnectionInput) => invoke<void>('test_connection', { input }),
  parseConnectionUrl: (input: string) => invoke<ConnectionFields>('parse_connection_url', { input }),
  buildConnectionUrl: (fields: ConnectionFields) => invoke<string>('build_connection_url', { fields }),
  getSettings: () => invoke<Settings>('get_settings'),
  setCommitMode: (mode: CommitMode) => invoke<CommitMode>('set_commit_mode', { mode }),
  setPoolSize: (poolSize: number) => invoke<number>('set_pool_size', { poolSize }),
  setSupermavenEnabled: (enabled: boolean) => invoke<SupermavenStatus>('set_supermaven_enabled', { enabled }),
  splitSql: (sql: string, editorBoundaries = true) => invoke<StatementRange[]>('split_sql', { sql, editorBoundaries }),
  executeQuery: (sql: string, queryToken: string, offset = 0, pageSize = 200, connectionId?: string, readOnlyReplay = false, captureBatchResults = false) =>
    invoke<QueryExecution>('execute_query', { sql, queryToken, offset, pageSize, connectionId, readOnlyReplay, captureBatchResults }),
  explainQuery: (request: ExplainRequest) => invoke<ExplainResult>('explain_query', { request }),
  shareExplain: (request: ShareExplainRequest) => invoke<{ url: string }>('share_explain', { request }),
  openExplainLink: (url: string) => invoke<void>('open_explain_link', { url }),
  applyRowUpdates: (connectionId: string, queryToken: string, updates: RowUpdate[]) =>
    invoke<ApplyUpdatesResult>('apply_row_updates', { connectionId, queryToken, updates }),
  cancelQuery: (queryToken: string) => invoke<CancelResult>('cancel_query', { queryToken }),
  connectionStats: (connectionId?: string) => invoke<ConnectionStats | null>('connection_stats', { connectionId }),
  txStatus: () => invoke<TxStatus>('tx_status'),
  txCommit: () => invoke<TxStatus>('tx_commit'),
  txRollback: () => invoke<TxStatus>('tx_rollback'),
  introspectSchema: (connectionId?: string) => invoke<SchemaTree>('introspect_schema', { connectionId }),
  foreignKeyValues: (connectionId: string, queryToken: string, request: ForeignKeyLookup) =>
    invoke<ForeignKeyValues>('foreign_key_values', { connectionId, queryToken, request }),
  getDdl: (schema: string, name: string, connectionId?: string) => invoke<string>('get_ddl', { schema, name, connectionId }),
  historyPage: (search: string | null, offset = 0, limit = 50) =>
    invoke<HistoryPage>('history_page', { search, offset, limit }),
  historyTransactionPage: (id: string, offset = 0, limit = 50) =>
    invoke<HistoryPage>('history_transaction_page', { id, offset, limit }),
  historyEntries: (ids: number[]) => invoke<HistoryEntry[]>('history_entries', { ids }),
  historyStats: () => invoke<HistoryStats>('history_stats'),
  historyStorageStats: () => invoke<HistoryStorageStats>('history_storage_stats'),
  historyGetResult: (id: number) => invoke<{ result: QueryResult | null }>('history_get_result', { id }),
  historyDelete: (id: number) => invoke<void>('history_delete', { id }),
  historyPurge: () => invoke<void>('history_purge'),
  saveDraft: (tabKind: string, content: string) => invoke<void>('save_draft', { tabKind, content }),
  loadDraft: (tabKind: string) => invoke<string | null>('load_draft', { tabKind }),
  enableExitGuard: () => invoke<void>('enable_exit_guard'),
  finishExit: () => invoke<void>('finish_exit'),
  supermavenStatus: () => invoke<SupermavenStatus>('supermaven_status'),
  supermavenComplete: (request: SupermavenRequest) => invoke<SupermavenCompletion | null>('supermaven_complete', { request }),
  supermavenCancel: (requestId: string) => invoke<void>('supermaven_cancel', { requestId }),
  supermavenUseFree: () => invoke<void>('supermaven_use_free'),
  supermavenOpenActivation: () => invoke<void>('supermaven_open_activation'),
  exportFile: (fileId: string, request: ExportFileRequest) => invoke<void>('export_file', { fileId, request })
};

export function errorMessage(error: unknown): string {
  const appError = error as AppError | undefined;
  if (appError?.message) return appError.message;
  if (error instanceof Error) return error.message;
  return String(error);
}
