import type { ColumnMeta, QueryResult } from './types';

export type CellPosition = { row: number; column: number };
export type GridSort = { column: string; direction: 'asc' | 'desc' } | null;
export type IndexedRow = { row: Record<string, unknown>; originalIndex: number };
export type CellKind = 'boolean' | 'integer' | 'numeric' | 'json' | 'text';

export function positionAfterReorder(previousRows: IndexedRow[], nextRows: IndexedRow[], position: CellPosition | null): CellPosition | null {
  if (!position) return null;
  const originalIndex = previousRows[position.row]?.originalIndex;
  const row = nextRows.findIndex((entry) => entry.originalIndex === originalIndex);
  return row < 0 ? null : { row, column: position.column };
}

export function visibleRowWindow(total: number, scrollTop: number, viewportHeight: number, rowHeight = 33, headerHeight = 49, overscan = 10) {
  const visibleStart = Math.floor(Math.max(0, scrollTop - headerHeight) / rowHeight);
  const first = Math.min(Math.max(0, total - 1), Math.max(0, visibleStart - overscan));
  const last = Math.min(total, Math.max(first, Math.ceil(Math.max(0, scrollTop + viewportHeight - headerHeight) / rowHeight) + overscan));
  return { first, last, top: first * rowHeight, bottom: (total - last) * rowHeight };
}

export function cellKind(column: ColumnMeta): CellKind {
  const type = column.typeName.toLowerCase().replace(/^pg_catalog\./, '');
  if (type === 'bool' || type === 'boolean') return 'boolean';
  if (['int2', 'int4', 'int8', 'smallint', 'integer', 'bigint', 'oid'].includes(type)) return 'integer';
  if (['numeric', 'decimal', 'float4', 'float8', 'real', 'double precision', 'money'].includes(type)) return 'numeric';
  if (type === 'json' || type === 'jsonb') return 'json';
  return 'text';
}

export function cellText(value: unknown, column?: ColumnMeta, jsonValuesAsText = false): string {
  if (value == null) return 'NULL';
  if (column && cellKind(column) === 'json' && !jsonValuesAsText) return JSON.stringify(value);
  if (typeof value === 'object') return JSON.stringify(value);
  return String(value);
}

// Format JSON tokens without parsing their numbers into JavaScript doubles.
export function prettyCell(value: unknown, column: ColumnMeta, jsonValuesAsText = false): string {
  const text = cellText(value, column, jsonValuesAsText);
  if (value == null || (cellKind(column) !== 'json' && typeof value !== 'object')) return text;
  try {
    JSON.parse(text);
  } catch {
    return text;
  }
  let formatted = '';
  let depth = 0;
  let inString = false;
  let escaped = false;
  const compact = text.replace(/"(?:\\.|[^"\\])*"|\s+/g, (token) => token.startsWith('"') ? token : '');
  for (let index = 0; index < compact.length; index += 1) {
    const char = compact[index];
    if (inString) {
      formatted += char;
      if (escaped) escaped = false;
      else if (char === '\\') escaped = true;
      else if (char === '"') inString = false;
    } else if (char === '"') {
      inString = true;
      formatted += char;
    } else if (char === '{' || char === '[') {
      formatted += char;
      depth += 1;
      if (compact[index + 1] !== '}' && compact[index + 1] !== ']') formatted += `\n${'  '.repeat(depth)}`;
    } else if (char === '}' || char === ']') {
      depth -= 1;
      if (compact[index - 1] !== '{' && compact[index - 1] !== '[') formatted += `\n${'  '.repeat(depth)}`;
      formatted += char;
    } else if (char === ',') formatted += `,\n${'  '.repeat(depth)}`;
    else if (char === ':') formatted += ': ';
    else formatted += char;
  }
  return formatted;
}

type Decimal = { negative: boolean; digits: string; rank: bigint };

function decimal(value: unknown): Decimal | null {
  const match = /^([+-]?)(\d*)(?:\.(\d*))?(?:e([+-]?\d+))?$/i.exec(String(value).trim());
  if (!match || (!match[2] && !match[3])) return null;
  const combined = `${match[2]}${match[3] ?? ''}`;
  const digits = combined.replace(/^0+/, '').replace(/0+$/, '');
  if (!digits) return { negative: false, digits: '0', rank: 0n };
  const leadingZeroes = combined.length - combined.replace(/^0+/, '').length;
  return {
    negative: match[1] === '-',
    digits,
    rank: BigInt(match[2].length - leadingZeroes) + BigInt(match[4] ?? '0')
  };
}

function compareDecimals(a: Decimal, b: Decimal): number {
  if (a.negative !== b.negative) return a.negative ? -1 : 1;
  let comparison = 0;
  if (a.digits === '0' || b.digits === '0') comparison = a.digits === b.digits ? 0 : a.digits === '0' ? -1 : 1;
  else if (a.rank !== b.rank) comparison = a.rank < b.rank ? -1 : 1;
  else {
    const width = Math.max(a.digits.length, b.digits.length);
    const left = a.digits.padEnd(width, '0');
    const right = b.digits.padEnd(width, '0');
    comparison = left < right ? -1 : left > right ? 1 : 0;
  }
  return a.negative ? -comparison : comparison;
}

function numericOrder(value: unknown): { category: number; value: Decimal | null } | null {
  const text = String(value).trim();
  if (/^-infinity$/i.test(text)) return { category: -1, value: null };
  if (/^\+?infinity$/i.test(text)) return { category: 1, value: null };
  if (/^[+-]?nan$/i.test(text)) return { category: 2, value: null };
  const parsed = decimal(value);
  return parsed ? { category: 0, value: parsed } : null;
}

export function sortedRows(result: QueryResult, sort: GridSort): IndexedRow[] {
  const rows = result.rows.map((row, originalIndex) => ({ row, originalIndex }));
  const column = sort && result.columns.find((candidate) => candidate.name === sort.column);
  if (!sort || !column) return rows;
  const numeric = ['integer', 'numeric'].includes(cellKind(column));
  return rows.sort((a, b) => {
    const left = a.row[column.name];
    const right = b.row[column.name];
    if (left == null || right == null) return left == null && right == null ? a.originalIndex - b.originalIndex : left == null ? 1 : -1;
    const leftNumber = numeric ? numericOrder(left) : null;
    const rightNumber = numeric ? numericOrder(right) : null;
    const comparison = leftNumber && rightNumber
      ? leftNumber.category - rightNumber.category || (leftNumber.value && rightNumber.value ? compareDecimals(leftNumber.value, rightNumber.value) : 0)
      : cellText(left, column, result.jsonValuesAsText).localeCompare(cellText(right, column, result.jsonValuesAsText), undefined, { numeric: true });
    return (sort.direction === 'asc' ? comparison : -comparison) || a.originalIndex - b.originalIndex;
  });
}

export function selectionBounds(anchor: CellPosition, focus: CellPosition) {
  return {
    firstRow: Math.min(anchor.row, focus.row), lastRow: Math.max(anchor.row, focus.row),
    firstColumn: Math.min(anchor.column, focus.column), lastColumn: Math.max(anchor.column, focus.column)
  };
}

export function selectionText(rows: IndexedRow[], columns: ColumnMeta[], anchor: CellPosition, focus: CellPosition, jsonValuesAsText = false): string {
  const bounds = selectionBounds(anchor, focus);
  const quote = (value: unknown, column: ColumnMeta) => {
    const text = cellText(value, column, jsonValuesAsText);
    return /[\t\n\r"]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
  };
  // A single cell copies its complete raw content, including tabs/newlines.
  if (bounds.firstRow === bounds.lastRow && bounds.firstColumn === bounds.lastColumn) {
    const column = columns[bounds.firstColumn];
    return cellText(rows[bounds.firstRow]?.row[column?.name], column, jsonValuesAsText);
  }
  return rows.slice(bounds.firstRow, bounds.lastRow + 1)
    .map(({ row }) => columns.slice(bounds.firstColumn, bounds.lastColumn + 1).map((column) => quote(row[column.name], column)).join('\t'))
    .join('\n');
}

export function parseCellDraft(column: ColumnMeta, draft: string, isNull: boolean): unknown {
  if (column.generated) throw new Error('Generated columns cannot be edited.');
  if (isNull) {
    if (column.nullable === false) throw new Error('This column does not allow NULL.');
    return null;
  }
  const kind = cellKind(column);
  if (kind === 'boolean') {
    if (draft !== 'true' && draft !== 'false') throw new Error('Choose true or false.');
    return draft === 'true';
  }
  if (kind === 'integer') {
    if (!/^[+-]?\d+$/.test(draft.trim())) throw new Error('Enter a whole number, or explicitly select NULL.');
    const value = BigInt(draft.trim());
    const ranges: Record<string, [bigint, bigint]> = {
      int2: [-32768n, 32767n], smallint: [-32768n, 32767n],
      int4: [-2147483648n, 2147483647n], integer: [-2147483648n, 2147483647n],
      int8: [-9223372036854775808n, 9223372036854775807n], bigint: [-9223372036854775808n, 9223372036854775807n],
      oid: [0n, 4294967295n]
    };
    const range = ranges[column.typeName.toLowerCase().replace(/^pg_catalog\./, '')];
    if (range && (value < range[0] || value > range[1])) throw new Error(`Value is outside the range for ${column.typeName}.`);
    return draft.trim();
  }
  if (kind === 'numeric') {
    if (!decimal(draft) && !/^[+-]?(?:Infinity|NaN)$/i.test(draft.trim())) throw new Error('Enter a valid number, or explicitly select NULL.');
    return draft.trim();
  }
  if (kind === 'json') {
    try { JSON.parse(draft); } catch { throw new Error('Enter valid JSON. Use NULL for SQL NULL, or null for a JSON null value.'); }
    // Preserve JSON number literals and PostgreSQL numeric precision through SQL generation.
    return draft;
  }
  return draft;
}
