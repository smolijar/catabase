import type { ColumnMeta, QueryResult, RowKey, RowUpdate } from './types';
import type { PendingChange } from './workspace';

export interface EditTarget {
  schema: string;
  table: string;
  key: RowKey;
  keyColumns: ColumnMeta[];
  editableColumns: ColumnMeta[];
}

// Match the types decoded faithfully by pg::cell_to_json. Array/extension-type
// placeholders cannot be used as a row identity or an original edit value.
const EDIT_TYPES = new Set(['bool', 'int2', 'int4', 'int8', 'numeric', 'float4', 'float8', 'text', 'varchar', 'bpchar', 'name', 'uuid', 'json', 'jsonb', 'date', 'timestamp', 'timestamptz']);

export function assessRowEditing(result: QueryResult | null, rows = result?.rows ?? []): { target: EditTarget | null; reason: string } {
  const unavailable = (reason: string) => ({ target: null, reason });
  if (!result?.columns.length || !rows.length) return unavailable('Run a query that returns rows before editing.');
  const columns = result.columns.filter((column) => column.sourceName && column.tableSchema && column.tableName);
  const identities = new Set(columns.map((column) => JSON.stringify([column.tableSchema, column.tableName])));
  if (identities.size !== 1) return unavailable('Editing requires columns from a single source table and a complete primary key or unique key in the result.');
  const { tableSchema: schema, tableName: table } = columns[0];
  if (new Set(result.columns.map((column) => column.name)).size !== result.columns.length || new Set(columns.map((column) => column.sourceName)).size !== columns.length) {
    return unavailable('Selected column names are ambiguous. Select each source column once and give result columns distinct aliases.');
  }
  const keys = (result.rowKeys ?? [])
    .filter((key) => key.schema === schema && key.table === table && key.columns.length > 0 && key.columns.every((name) => columns.some((column) => column.sourceName === name)))
    .sort((a, b) => Number(b.primaryKey) - Number(a.primaryKey) || a.columns.length - b.columns.length);
  if (!keys.length) return unavailable('Missing a complete primary key or unique key in the selected columns. Include every column of one key and re-run the query.');
  let reason = 'The selected key contains NULL values and does not uniquely identify every row. Include a non-null key or a NULLS NOT DISTINCT unique key.';
  for (const key of keys) {
    const keyColumns = key.columns.map((name) => columns.find((column) => column.sourceName === name)!);
    if (keyColumns.some((column) => !EDIT_TYPES.has(column.typeName))) {
      reason = 'The selected key uses a type that cannot be safely edited.';
      continue;
    }
    if (rows.some((row) => keyColumns.some((column) => !Object.hasOwn(row, column.name) || row[column.name] === undefined || (row[column.name] === null && (key.primaryKey || !key.nullsNotDistinct))))) continue;
    const identities = rows.map((row) => JSON.stringify(keyColumns.map((column) => row[column.name])));
    if (new Set(identities).size !== rows.length) {
      reason = 'The result repeats the same row key. Return each source row once before editing.';
      continue;
    }
    const editableColumns = columns.filter((column) => !column.generated && EDIT_TYPES.has(column.typeName));
    if (!editableColumns.length) return unavailable('The result has no supported, non-generated source columns to edit.');
    return { target: { schema: schema!, table: table!, key, keyColumns, editableColumns }, reason: '' };
  }
  return unavailable(reason);
}

export function originalEditColumns(target: EditTarget, changes: PendingChange[]): ColumnMeta[] {
  const changedColumns = changes.map((change) => {
    const column = target.editableColumns.find((column) => column.name === change.column);
    if (!column) throw new Error(`Column ${change.column} is not editable.`);
    return column;
  });
  return [...new Map([...target.keyColumns, ...changedColumns].map((column) => [column.sourceName, column])).values()];
}

export function buildRowUpdates(target: EditTarget, originalRows: Record<string, unknown>[], changes: PendingChange[]): RowUpdate[] {
  return [...new Set(changes.map((change) => change.rowIndex))].map((rowIndex) => {
    const rowChanges = changes.filter((change) => change.rowIndex === rowIndex);
    const originalColumns = originalEditColumns(target, rowChanges);
    return {
      schema: target.schema,
      table: target.table,
      keyColumns: target.key.columns,
      values: Object.fromEntries(rowChanges.map((change) => [target.editableColumns.find((column) => column.name === change.column)!.sourceName!, change.newValue])),
      original: Object.fromEntries(originalColumns.map((column) => [column.sourceName!, originalRows[rowIndex][column.name]]))
    };
  });
}
