export interface AppError {
  message: string;
  severity?: string | null;
  detail?: string | null;
  hint?: string | null;
  code?: string | null;
  position?: number | null;
}

export interface ConnectionFields {
  name: string;
  host: string;
  port: number;
  database: string;
  username: string;
  password?: string | null;
  ssl: boolean;
}

export interface ConnectionInput extends ConnectionFields {
  id?: string | null;
}

export interface ConnectionRecord extends ConnectionFields {
  id: string;
  isActive: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface Settings {
  poolSize: number;
  aiModel?: string | null;
}

export interface ColumnMeta {
  name: string;
  sourceName?: string | null;
  typeOid: number;
  typeName: string;
  tableOid?: number | null;
  tableSchema?: string | null;
  tableName?: string | null;
  columnId?: number | null;
  nullable?: boolean | null;
  generated: boolean;
  primaryKey: boolean;
}

export interface QueryResult {
  columns: ColumnMeta[];
  /** Complete catalog keys, not merely PK flags on projected columns. */
  rowKeys?: RowKey[];
  rows: Record<string, unknown>[];
  rowCount: number;
  durationMs: number;
  autoLimited: boolean;
  hasMore: boolean;
  /** New results preserve JSON/JSONB as raw text; older history contains decoded values. */
  jsonValuesAsText?: boolean;
}

export interface RowKey {
  schema: string;
  table: string;
  columns: string[];
  primaryKey: boolean;
  nullsNotDistinct: boolean;
}

export interface RowUpdate {
  schema: string;
  table: string;
  keyColumns: string[];
  values: Record<string, unknown>;
  original: Record<string, unknown>;
}

export interface ApplyUpdatesResult {
  updatedRows: number;
  inTransaction: boolean;
}

export interface CancelResult {
  cancelled: boolean;
  message: string;
}

export interface HistoryEntry {
  id: number;
  connectionId?: string | null;
  query: string;
  durationMs: number;
  rowCount?: number | null;
  resultSize?: number | null;
  isError: boolean;
  errorMessage?: string | null;
  hasResult: boolean;
  executedAt: string;
}

export interface HistoryPage {
  entries: HistoryEntry[];
  total: number;
}

export interface HistoryStats {
  total: number;
  errors: number;
  lastExecutedAt?: string | null;
}

export interface SchemaColumn {
  name: string;
  typeName: string;
  nullable: boolean;
  generated: boolean;
  primaryKey: boolean;
}

export interface SchemaItem {
  schema: string;
  name: string;
  kind: string;
  columns: SchemaColumn[];
  foreignKeys?: SchemaForeignKey[];
}

export interface SchemaForeignKey {
  name: string;
  columns: string[];
  referencedSchema: string;
  referencedTable: string;
  referencedColumns: string[];
}

export interface SchemaTree {
  schemas: SchemaItem[];
}

export interface AiHealth {
  state: string;
  message?: string | null;
}

export interface StatementRange {
  start: number;
  end: number;
  text: string;
}

export interface ExecutionTarget {
  sql: string;
  start: number;
  end: number;
}

export interface TxStatus {
  active: boolean;
  startedAt?: string | null;
}

export interface ConnectionStats {
  active: number;
  idle: number;
  poolSize: number;
}

export interface AiCompletionRequest {
  prefix: string;
  suffix: string;
  language: string;
  position: number;
  schemaContext?: string | null;
  model?: string | null;
}
