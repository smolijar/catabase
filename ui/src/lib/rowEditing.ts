import type { ColumnMeta, QueryResult, RowKey, RowUpdate } from './types';
import type { CellEdit, PendingChange } from './workspace';
import { arrayElementType, parsePgArray } from './pgArray.ts';
import { sameCellValue } from './resultsGrid.ts';

export interface EditTarget {
  schema: string;
  table: string;
  key: RowKey;
  keyColumns: ColumnMeta[];
  editableColumns: ColumnMeta[];
}

// Match the types decoded faithfully by pg::cell_to_json. Legacy array and
// unsupported extension-type placeholders must never become original values.
const EDIT_TYPES = new Set(['bool', 'int2', 'int4', 'int8', 'numeric', 'float4', 'float8', 'text', 'varchar', 'bpchar', 'name', 'uuid', 'json', 'jsonb', 'date', 'timestamp', 'timestamptz']);

function supportedColumn(column: ColumnMeta, rows: Record<string, unknown>[]) {
  const element = arrayElementType(column.typeName);
  if (!element) return EDIT_TYPES.has(column.typeName);
  if (!EDIT_TYPES.has(element)) return false;
  return rows.every((row) => {
    const value = row[column.name];
    if (value === null) return true;
    if (typeof value !== 'string') return false;
    try { parsePgArray(value); return true; } catch { return false; }
  });
}

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
    if (keyColumns.some((column) => !supportedColumn(column, rows))) {
      reason = 'The selected key uses a type that cannot be safely edited.';
      continue;
    }
    if (rows.some((row) => keyColumns.some((column) => !Object.hasOwn(row, column.name) || row[column.name] === undefined || (row[column.name] === null && (key.primaryKey || !key.nullsNotDistinct))))) continue;
    const identities = rows.map((row) => JSON.stringify(keyColumns.map((column) => row[column.name])));
    if (new Set(identities).size !== rows.length) {
      reason = 'The result repeats the same row key. Return each source row once before editing.';
      continue;
    }
    const editableColumns = columns.filter((column) => !column.generated && supportedColumn(column, rows));
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

export function buildRowUpdates(target: EditTarget, originalRows: Record<string, unknown>[], changes: PendingChange[], checkOriginalValues = true): RowUpdate[] {
  const sourceNames = new Map(target.editableColumns.map(column => [column.name, column.sourceName!]));
  return [...new Set(changes.map((change) => change.rowIndex))].map((rowIndex) => {
    const rowChanges = changes.filter((change) => change.rowIndex === rowIndex);
    const guardedColumns = originalEditColumns(target, rowChanges);
    const originalColumns = checkOriginalValues ? guardedColumns : target.keyColumns;
    const defaultColumns = rowChanges.filter(change => change.useDefault).map(change => sourceNames.get(change.column)!);
    return {
      schema: target.schema,
      table: target.table,
      keyColumns: target.key.columns,
      ...(checkOriginalValues ? {} : { overwriteChangedCells: true }),
      values: Object.fromEntries(rowChanges.filter(change => !change.useDefault).map(change => [sourceNames.get(change.column)!, change.newValue])),
      ...(defaultColumns.length ? { defaultColumns } : {}),
      original: Object.fromEntries(originalColumns.map((column) => [column.sourceName!, originalRows[rowIndex][column.name]]))
    };
  });
}

export function stageCellEdits(
  result: QueryResult,
  originalRow: Record<string, unknown>,
  pending: PendingChange[],
  rowIndex: number,
  edits: Record<string, CellEdit>
) {
  const row = { ...result.rows[rowIndex] };
  const changes = pending.filter(change => change.rowIndex !== rowIndex || !Object.hasOwn(edits, change.column));
  for (const [column, edit] of Object.entries(edits)) {
    const oldValue = originalRow[column];
    if ('action' in edit) {
      row[column] = oldValue;
      if (edit.action === 'default') changes.push({ rowIndex, column, oldValue, newValue: oldValue, useDefault: true });
    } else {
      const meta = result.columns.find(item => item.name === column)!;
      const unchanged = sameCellValue(oldValue, edit.value, meta, result.jsonValuesAsText, true);
      row[column] = unchanged ? oldValue : edit.value;
      if (!unchanged) changes.push({ rowIndex, column, oldValue, newValue: edit.value });
    }
  }
  return { row, changes };
}
