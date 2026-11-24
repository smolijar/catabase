import type { QueryResult } from './types';

export const exportFormats = ['tsv', 'csv', 'json'] as const;
export type ExportFormat = typeof exportFormats[number];
export type ExportScope = 'selection' | 'loaded' | 'query';
export interface ExportOptions {
  format: ExportFormat;
  target: 'clipboard' | 'file';
  scope: ExportScope;
  batchSize: number;
  rowLimit: number;
}
export type ExportFileRequest =
  | { action: 'open'; path: string }
  | { action: 'append'; content: string }
  | { action: 'finish' | 'discard' };

export const DEFAULT_EXPORT_BATCH = 5_000;
export const DEFAULT_EXPORT_LIMIT = 100_000;
export const MAX_EXPORT_BATCH = 10_000; // Matches the PostgreSQL page size clamp.
export const MAX_EXPORT_LIMIT = 0xffff_ffff; // The IPC offset is an unsigned 32-bit integer.

export function isExportFormat(value: string | null): value is ExportFormat {
  return exportFormats.some(format => format === value);
}

export function validExportPagination(batchSize: number, rowLimit: number) {
  return Number.isInteger(batchSize) && batchSize >= 1 && batchSize <= MAX_EXPORT_BATCH
    && Number.isInteger(rowLimit) && rowLimit >= 1 && rowLimit <= MAX_EXPORT_LIMIT;
}

export interface ExportPageProgress {
  rows: number;
  queries: number;
  currentPage: { offset: number; batchSize: number } | null;
}

export interface ResultExportProgress extends ExportPageProgress {
  phase: 'choosing-file' | 'running' | 'cancelling' | 'finishing' | 'complete' | 'cancelled' | 'failed';
  maxRows: number;
  message?: string;
}

export async function exportQueryPages(
  source: QueryResult,
  options: Pick<ExportOptions, 'batchSize' | 'rowLimit'>,
  signal: AbortSignal,
  fetchPage: (offset: number, batchSize: number) => Promise<QueryResult>,
  writePage: (page: QueryResult) => Promise<void>,
  onProgress: (progress: ExportPageProgress) => void
) {
  if (!validExportPagination(options.batchSize, options.rowLimit)) throw new Error('Choose valid export limits.');
  let rows = 0;
  let queries = 0;
  let hasMore = true;
  while (rows < options.rowLimit && hasMore) {
    signal.throwIfAborted();
    const batchSize = Math.min(options.batchSize, options.rowLimit - rows);
    onProgress({ rows, queries, currentPage: { offset: rows, batchSize } });
    const page = await fetchPage(rows, batchSize);
    signal.throwIfAborted();
    if (!page.autoLimited || page.rows.length > batchSize || (page.hasMore && page.rows.length !== batchSize)) {
      throw new Error('The query did not return a valid page for export.');
    }
    if (page.columns.length !== source.columns.length || page.columns.some((column, index) =>
      column.name !== source.columns[index].name || column.typeOid !== source.columns[index].typeOid)) {
      throw new Error('The query columns changed while exporting. Run the query again to refresh the results.');
    }
    await writePage(page);
    rows += page.rows.length;
    queries++;
    hasMore = page.hasMore;
    onProgress({ rows, queries, currentPage: null });
  }
  signal.throwIfAborted();
  return { rows, capped: hasMore };
}

export function serializeRows(result: QueryResult, format: ExportFormat): string {
  const encoder = createExportEncoder(format);
  return encoder.append(result) + encoder.finish();
}

export function createExportEncoder(format: ExportFormat) {
  let started = false;
  let hasRows = false;
  return {
    append(result: QueryResult) {
      const delimiter = format === 'csv' ? ',' : '\t';
      const columns = result.columns.map(column => column.name);
      const header = started ? '' : format === 'json' ? '[' : columns.map(value => escapeCell(value, delimiter)).join(delimiter);
      const body = format === 'json' ? serializeJsonRows(result) : result.rows
        .map(row => columns.map(column => escapeCell(formatValue(row[column]), delimiter)).join(delimiter)).join('\n');
      const separator = result.rows.length ? (format === 'json' && hasRows ? ',\n' : '\n') : '';
      started = true;
      hasRows ||= result.rows.length > 0;
      return header + separator + body;
    },
    finish() {
      return format === 'json' ? (hasRows ? '\n]' : started ? ']' : '[]') : '';
    }
  };
}

function serializeJsonRows(result: QueryResult): string {
  const jsonColumns = new Set(result.columns
    .filter((column) => ['json', 'jsonb'].includes(column.typeName.toLowerCase().replace(/^pg_catalog\./, '')))
    .map((column) => column.name));
  const rows = result.rows.map((row) => {
    const fields = Object.entries(row)
      .filter(([, value]) => value !== undefined)
      .map(([name, value]) => {
        const encoded = encodeJsonValue(value, result.jsonValuesAsText === true && jsonColumns.has(name));
        return `    ${JSON.stringify(name)}: ${encoded.replace(/\n/g, '\n    ')}`;
      });
    return fields.length ? `  {\n${fields.join(',\n')}\n  }` : '  {}';
  });
  return rows.join(',\n');
}

function encodeJsonValue(value: unknown, isJsonColumn: boolean): string {
  if (isJsonColumn && typeof value === 'string') {
    try {
      // Validate raw PostgreSQL JSON without serializing the parsed value:
      // JavaScript numbers would round large integers and exact decimals.
      JSON.parse(value);
      return value.trim();
    } catch {
      // Older history may contain an already-decoded JSON string scalar.
    }
  }
  return JSON.stringify(value, null, 2) ?? 'null';
}

function formatValue(value: unknown): string {
  if (value === null || value === undefined) return '';
  if (typeof value === 'object') return JSON.stringify(value);
  return String(value);
}

function escapeCell(value: string, delimiter: string): string {
  if (!value.includes(delimiter) && !value.includes('"') && !value.includes('\n') && !value.includes('\r')) return value;
  return `"${value.replaceAll('"', '""')}"`;
}
