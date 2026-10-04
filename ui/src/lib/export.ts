import type { QueryResult } from './types';

export function serializeRows(result: QueryResult, format: 'csv' | 'tsv' | 'json'): string {
  if (format === 'json') return serializeJsonRows(result);

  const delimiter = format === 'csv' ? ',' : '\t';
  const columns = result.columns.map((column) => column.name);
  const header = columns.map((value) => escapeCell(value, delimiter)).join(delimiter);
  const lines = result.rows.map((row) =>
    columns.map((column) => escapeCell(formatValue(row[column]), delimiter)).join(delimiter)
  );
  return [header, ...lines].join('\n');
}

function serializeJsonRows(result: QueryResult): string {
  const jsonColumns = new Set(result.columns
    .filter((column) => ['json', 'jsonb'].includes(column.typeName.toLowerCase().replace(/^pg_catalog\./, '')))
    .map((column) => column.name));
  if (!result.rows.length) return '[]';

  const rows = result.rows.map((row) => {
    const fields = Object.entries(row)
      .filter(([, value]) => value !== undefined)
      .map(([name, value]) => {
        const encoded = encodeJsonValue(value, result.jsonValuesAsText === true && jsonColumns.has(name));
        return `    ${JSON.stringify(name)}: ${encoded.replace(/\n/g, '\n    ')}`;
      });
    return fields.length ? `  {\n${fields.join(',\n')}\n  }` : '  {}';
  });
  return `[\n${rows.join(',\n')}\n]`;
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
