import { invoke } from '@tauri-apps/api/core';
import type {
  AiHealth,
  AiCompletionRequest,
  AppError,
  ConnectionFields,
  ConnectionInput,
  ConnectionRecord,
  HistoryPage,
  HistoryStats,
  QueryResult,
  SchemaTree,
  Settings,
  StatementRange,
  TxStatus
} from './types';

export const api = {
  listConnections: () => invoke<ConnectionRecord[]>('list_connections'),
  saveConnection: (input: ConnectionInput) => invoke<ConnectionRecord>('save_connection', { input }),
  deleteConnection: (id: string) => invoke<void>('delete_connection', { id }),
  activateConnection: (id: string) => invoke<ConnectionRecord>('activate_connection', { id }),
  testConnection: (input: ConnectionInput) => invoke<void>('test_connection', { input }),
  parseConnectionUrl: (input: string) => invoke<ConnectionFields>('parse_connection_url', { input }),
  buildConnectionUrl: (fields: ConnectionFields) => invoke<string>('build_connection_url', { fields }),
  getSettings: () => invoke<Settings>('get_settings'),
  setPoolSize: (poolSize: number) => invoke<number>('set_pool_size', { poolSize }),
  setAiModel: (model: string | null) => invoke<string | null>('set_ai_model', { model }),
  splitSql: (sql: string) => invoke<StatementRange[]>('split_sql', { sql }),
  executeQuery: (sql: string, queryToken: string, offset = 0, pageSize = 200) =>
    invoke<QueryResult>('execute_query', { sql, queryToken, offset, pageSize }),
  cancelQuery: (queryToken: string) => invoke('cancel_query', { queryToken }),
  connectionStats: () => invoke('connection_stats'),
  txStatus: () => invoke<TxStatus>('tx_status'),
  txCommit: () => invoke<TxStatus>('tx_commit'),
  txRollback: () => invoke<TxStatus>('tx_rollback'),
  introspectSchema: () => invoke<SchemaTree>('introspect_schema'),
  getDdl: (schema: string, name: string) => invoke<string>('get_ddl', { schema, name }),
  historyPage: (search: string | null, offset = 0, limit = 50) =>
    invoke<HistoryPage>('history_page', { search, offset, limit }),
  historyStats: () => invoke<HistoryStats>('history_stats'),
  historyGetResult: (id: number) => invoke<{ result: QueryResult | null }>('history_get_result', { id }),
  historyDelete: (id: number) => invoke<void>('history_delete', { id }),
  historyPurge: () => invoke<void>('history_purge'),
  saveDraft: (tabKind: string, content: string) => invoke<void>('save_draft', { tabKind, content }),
  loadDraft: (tabKind: string) => invoke<string | null>('load_draft', { tabKind }),
  aiHealth: () => invoke<AiHealth>('ai_health'),
  aiModels: () => invoke<string[]>('ai_models'),
  aiComplete: (request: AiCompletionRequest) => invoke<string | null>('ai_complete', { request }),
  exportWriteFile: (path: string, content: string) => invoke<void>('export_write_file', { path, content })
};

export function errorMessage(error: unknown): string {
  const appError = error as AppError | undefined;
  if (appError?.message) return appError.message;
  if (error instanceof Error) return error.message;
  return String(error);
}
