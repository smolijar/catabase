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
  supermavenEnabled: boolean;
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
  /** Absent on results saved before timing instrumentation was added. */
  timings?: QueryTimings;
  autoLimited: boolean;
  hasMore: boolean;
  /** New results preserve JSON/JSONB as raw text; older history contains decoded values. */
  jsonValuesAsText?: boolean;
}

export interface QueryTimings {
  prepareMs: number;
  metadataMs: number;
  executeFetchMs: number;
  decodeMs: number;
  totalMs: number;
  prepareMode?: 'separate' | 'combined';
  metadataSource: 'cache' | 'refreshed' | 'notNeeded';
  metadataQueries: number;
  command?: { setupMs: number; historyMs: number; totalMs: number };
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
  columnId?: number;
  typeOid?: number;
  typeName: string;
  nullable: boolean;
  generated: boolean;
  primaryKey: boolean;
}

export interface SchemaItem {
  schema: string;
  name: string;
  kind: string;
  tableOid?: number;
  columns: SchemaColumn[];
  rowKeys?: RowKey[];
  foreignKeys?: SchemaForeignKey[];
}

export interface SchemaForeignKey {
  name: string;
  columns: string[];
  referencedSchema: string;
  referencedTable: string;
  referencedColumns: string[];
}

export interface ForeignKeyLookup {
  schema: string;
  table: string;
  constraint: string;
  search: string;
}

export interface ForeignKeyValues {
  foreignKey: SchemaForeignKey;
  previewColumns: string[];
  rows: Record<string, string | null>[];
  hasMore: boolean;
}

export interface SchemaTree {
  schemas: SchemaItem[];
}

export interface SupermavenStatus {
  enabled: boolean;
  state: string;
  message: string;
  activationUrl?: string | null;
  serviceTier?: string | null;
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

export interface SupermavenRequest {
  requestId: string;
  documentId: string;
  text: string;
  schemaContext?: string;
  position: number;
}

export interface SupermavenCompletion {
  text: string;
  deleteBefore: number;
}
