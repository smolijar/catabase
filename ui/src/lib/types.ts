export interface AppError {
  message: string;
  severity?: string | null;
  detail?: string | null;
  hint?: string | null;
  code?: string | null;
  position?: number | null;
  connectionFailure?: 'connect' | 'closed' | null;
  statements?: BatchStatement[];
}

export interface ConnectionFields {
  name: string;
  host: string;
  port: number;
  database: string;
  username: string;
  password?: string | null;
  ssl: boolean;
  options?: ConnectionOptions;
}

export interface ConnectionOptions {
  connectTimeoutSeconds: number;
  queryTimeoutSeconds: number;
  lockTimeoutSeconds: number;
  readOnly: boolean;
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
  commitMode: CommitMode;
}

export type CommitMode = 'auto' | 'manual' | 'smart';
export type CommitOutcome = 'pending' | 'committed' | 'rolledBack' | 'interrupted';

export interface CommitFeedback {
  outcome: CommitOutcome;
  transactionId: string | null;
  affectedRows: number | null;
  rowVerb: 'affected' | 'deleted' | 'updated' | null;
}

export interface TxCompletion {
  id: string;
  outcome: Exclude<CommitOutcome, 'pending'>;
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
  commitFeedback?: CommitFeedback;
  historyEntryIds?: number[];
  autoLimited: boolean;
  hasMore: boolean;
  /** New results preserve JSON/JSONB as raw text; older history contains decoded values. */
  jsonValuesAsText?: boolean;
}

export interface BatchStatement {
  statement: StatementRange;
  result: QueryResult | null;
  durationMs: number;
  historyEntryId: number | null;
}

export interface QueryExecution extends QueryResult {
  statements?: BatchStatement[];
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
  defaultColumns?: string[];
  original: Record<string, unknown>;
  overwriteChangedCells?: boolean;
}

export interface ApplyUpdatesResult {
  updatedRows: number;
  inTransaction: boolean;
  defaultValues?: (Record<string, unknown> | null)[];
  commitFeedback: CommitFeedback;
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
  startedAt?: string | null;
  queryHash: string;
  resultHash?: string | null;
  querySize: number;
  executionSize: number;
  queryReferences: number;
  resultReferences: number;
  transaction?: HistoryTransaction | null;
}

export interface HistoryTransaction {
  id: string;
  kind: 'manual' | 'auto' | 'review' | 'batch';
  status: 'active' | 'finished' | 'rolledBack' | 'interrupted';
  statementCount: number;
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

export interface HistoryStorageStats {
  storedBytes: number;
  allocatedBytes: number;
  savedBytes: number;
  queryBytes: number;
  resultBytes: number;
  executionBytes: number;
  uniqueQueries: number;
  uniqueResults: number;
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
  selectedSql?: string[];
}

export interface TxStatus {
  active: boolean;
  automatic?: boolean;
  startedAt?: string | null;
  transactionId?: string | null;
  rowActivity?: { inserted: number; updated: number; deleted: number; tables: number } | null;
  completion?: TxCompletion;
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
