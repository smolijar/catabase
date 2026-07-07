import type { QueryResult } from './types';

export function serializeRows(result: QueryResult, format: 'csv' | 'tsv' | 'json'): string {
  if (format === 'json') return JSON.stringify(result.rows, null, 2);

  const delimiter = format === 'csv' ? ',' : '\t';
  const columns = result.columns.map((column) => column.name);
  const header = columns.map((value) => escapeCell(value, delimiter)).join(delimiter);
  const lines = result.rows.map((row) =>
    columns.map((column) => escapeCell(formatValue(row[column]), delimiter)).join(delimiter)
  );
  return [header, ...lines].join('\n');
}

function formatValue(value: unknown): string {
  if (value === null || value === undefined) return '';
  if (typeof value === 'object') return JSON.stringify(value);
  return String(value);
}

function escapeCell(value: string, delimiter: string): string {
  if (!value.includes(delimiter) && !value.includes('"') && !value.includes('\n')) return value;
  return `"${value.replaceAll('"', '""')}"`;
}
