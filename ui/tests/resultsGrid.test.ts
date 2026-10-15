import assert from 'node:assert/strict';
import test from 'node:test';
import { cellDisplayType, cellText, columnHeaderInfo, parseCellDraft, positionAfterReorder, prettyCell, sameCellValue, selectionText, sortedRows, visibleRowWindow } from '../src/lib/resultsGrid.ts';
import type { ColumnMeta, QueryResult, SchemaItem } from '../src/lib/types.ts';

const column = (typeName = 'text', more: Partial<ColumnMeta> = {}): ColumnMeta => ({ name: 'value', typeName, typeOid: 0, nullable: true, generated: false, primaryKey: false, ...more });
const result = (values: unknown[], typeName = 'text'): QueryResult => ({ columns: [column(typeName)], rows: values.map((value) => ({ value })), rowCount: values.length, durationMs: 1, autoLimited: false, hasMore: false });

test('display types follow PostgreSQL metadata, preserving exact numbers and numeric-looking strings', () => {
  for (const type of ['int8', 'numeric', 'numeric(20, 8)', 'pg_catalog.float8', 'double precision', 'oid']) assert.equal(cellDisplayType(column(type), '9007199254740993'), 'number');
  for (const value of ['0123', 'false', '2026-10-15', 'NULL']) assert.equal(cellDisplayType(column('text'), value), 'string');
  for (const type of ['date', 'time', 'timetz', 'timestamptz', 'timestamp(6) with time zone', 'time(3) without time zone', 'interval']) assert.equal(cellDisplayType(column(type), 'unchanged'), 'date');
  for (const value of [true, false]) assert.equal(cellDisplayType(column('bool'), value), 'boolean');
  assert.equal(cellDisplayType(column('jsonb'), 'null'), 'json');
  for (const type of ['_date', 'timestamp[]', 'int4[]', 'bytea']) assert.equal(cellDisplayType(column(type), 'unchanged'), 'other');
  for (const type of ['text', 'int8', 'date', 'bool', 'jsonb']) assert.equal(cellDisplayType(column(type), null), 'null');
});

const reflectedTable: SchemaItem = {
  schema: 'public', name: 'children', kind: 'table',
  columns: [
    { name: 'id', typeName: 'int8', nullable: false, generated: false, primaryKey: true },
    { name: 'tenant_id', typeName: 'text', nullable: false, generated: false, primaryKey: false },
    { name: 'parent_id', typeName: 'int8', nullable: true, generated: false, primaryKey: false }
  ],
  rowKeys: [{ schema: 'public', table: 'children', columns: ['id', 'tenant_id'], primaryKey: true, nullsNotDistinct: false }],
  foreignKeys: [{ name: 'parent_fk', columns: ['parent_id', 'tenant_id'], referencedSchema: 'public', referencedTable: 'parents', referencedColumns: ['id', 'tenant_id'] }]
};

test('header metadata uses reflected origins through aliases, including partially selected composite FKs', () => {
  const info = columnHeaderInfo(column('int8', { name: 'parent', sourceName: 'parent_id', tableSchema: 'public', tableName: 'children' }), [reflectedTable]);
  assert.equal(info.foreignKey, true);
  assert.equal(info.primaryKey, false);
  assert.match(info.tooltip, /Type: int8/);
  assert.match(info.tooltip, /Source: public.children.parent_id/);
  assert.match(info.tooltip, /Nullable/);
  assert.match(info.tooltip, /Foreign key \(parent_id, tenant_id\) → public.parents \(id, tenant_id\)/);
  const key = columnHeaderInfo(column('int8', { sourceName: 'id', tableSchema: 'public', tableName: 'children', nullable: false }), [reflectedTable]);
  assert.equal(key.primaryKey, true);
  assert.match(key.tooltip, /Primary key \(id, tenant_id\)/);
  assert.match(key.tooltip, /NOT NULL/);
});

test('header metadata never guesses FK origins from aliases, a different schema, or malformed reflection', () => {
  const alias = column('int8', { name: 'parent_id', tableSchema: 'public', tableName: 'children' });
  assert.equal(columnHeaderInfo(alias, [reflectedTable]).foreignKey, false);
  assert.equal(columnHeaderInfo({ ...alias, sourceName: 'parent_id', tableSchema: 'other' }, [reflectedTable]).foreignKey, false);
  const malformed = { ...reflectedTable, foreignKeys: [{ ...reflectedTable.foreignKeys![0], referencedColumns: [] }] };
  assert.equal(columnHeaderInfo({ ...alias, sourceName: 'parent_id' }, [malformed]).foreignKey, false);
  const saved = columnHeaderInfo(column('int8', { sourceName: 'id', tableSchema: 'public', tableName: 'children', primaryKey: true, generated: true }), [], reflectedTable.rowKeys);
  assert.equal(saved.primaryKey, true);
  assert.match(saved.tooltip, /Primary key \(id, tenant_id\)/);
  assert.match(saved.tooltip, /Generated — read-only/);
});

test('re-selecting an original FK clears the change without confusing exact text, SQL NULL or JSON null', () => {
  assert.ok(sameCellValue(42, '42', column('int4')));
  assert.ok(sameCellValue('9223372036854775807', '9223372036854775807', column('int8')));
  assert.ok(!sameCellValue('9007199254740993', '9007199254740992', column('int8')));
  assert.ok(!sameCellValue(null, 'NULL', column()));
  assert.ok(!sameCellValue('NULL', null, column()));
  assert.ok(sameCellValue(null, null, column()));
  assert.ok(!sameCellValue(null, 'null', column('jsonb'), true, true));
  assert.ok(sameCellValue({ id: 1 }, '{"id":1}', column('jsonb'), false, true));
  assert.ok(!sameCellValue('null', '"null"', column('jsonb'), true, true));
});

test('numeric sorting preserves exact int8/decimal ordering and original row identity', () => {
  const values = ['9007199254740993', '9007199254740992', null, '-0.000000000000000001', '1.000000000000000002', '1.000000000000000001', '0'];
  const rows = sortedRows(result(values, 'numeric'), { column: 'value', direction: 'asc' });
  assert.deepEqual(rows.map((entry) => entry.originalIndex), [3, 6, 5, 4, 1, 0, 2]);
  assert.deepEqual(sortedRows(result(values, 'numeric'), { column: 'value', direction: 'desc' }).map((entry) => entry.originalIndex), [0, 1, 4, 5, 6, 3, 2]);
});

test('numeric sorting handles exponents, negatives, equivalent values and zero', () => {
  const rows = sortedRows(result(['1e-10', '-3', '-10', '100e-12', '-0', '0', '1e10', '0.00000000001'], 'numeric'), { column: 'value', direction: 'asc' });
  assert.deepEqual(rows.map((entry) => entry.originalIndex), [2, 1, 4, 5, 7, 0, 3, 6]);
});

test('numeric sorting orders special PostgreSQL values numerically and keeps NULL last', () => {
  const data = result(['NaN', 'Infinity', '-Infinity', '-12', '0', '9007199254740993', null], 'numeric');
  assert.deepEqual(sortedRows(data, { column: 'value', direction: 'asc' }).map((entry) => entry.originalIndex), [2, 3, 4, 5, 1, 0, 6]);
  assert.deepEqual(sortedRows(data, { column: 'value', direction: 'desc' }).map((entry) => entry.originalIndex), [0, 1, 5, 4, 3, 2, 6]);
});

test('selection follows the same original row after a staged edit changes its sorted position', () => {
  const data = result(['10', '20', '30'], 'int8');
  const sort = { column: 'value', direction: 'asc' } as const;
  const before = sortedRows(data, sort);
  const after = sortedRows({ ...data, rows: [{ value: '40' }, data.rows[1], data.rows[2]] }, sort);
  assert.deepEqual(positionAfterReorder(before, after, { row: 0, column: 2 }), { row: 2, column: 2 });
  assert.deepEqual(positionAfterReorder(before, after, { row: 2, column: 1 }), { row: 1, column: 1 });
  assert.equal(positionAfterReorder(before, [], { row: 0, column: 0 }), null);
});

test('typed drafts distinguish empty text, SQL NULL, JSON null and preserve precision', () => {
  assert.equal(parseCellDraft(column(), '', false), '');
  assert.equal(parseCellDraft(column(), '', true), null);
  assert.equal(parseCellDraft(column('int8'), '9223372036854775807', false), '9223372036854775807');
  assert.equal(parseCellDraft(column('numeric'), '1.234567890123456789', false), '1.234567890123456789');
  assert.equal(parseCellDraft(column('jsonb'), 'null', false), 'null');
  assert.equal(parseCellDraft(column('jsonb'), '{"n":9007199254740993}', false), '{"n":9007199254740993}');
  assert.equal(parseCellDraft(column('bool'), 'false', false), false);
});

test('invalid, generated and non-nullable edits are rejected', () => {
  assert.throws(() => parseCellDraft(column('int8'), '9223372036854775808', false), /outside the range/);
  assert.throws(() => parseCellDraft(column('int4'), '1.5', false), /whole number/);
  assert.throws(() => parseCellDraft(column('numeric'), '', false), /valid number/);
  assert.throws(() => parseCellDraft(column('json'), '{', false), /valid JSON/);
  assert.throws(() => parseCellDraft(column('text', { nullable: false }), '', true), /does not allow NULL/);
  assert.throws(() => parseCellDraft(column('text', { generated: true }), 'x', false), /Generated/);
});

test('range copy uses visible sorted order, preserves null and escapes TSV content', () => {
  const data = result(['b\t"quoted"', 'a\nline', null]);
  const rows = sortedRows(data, { column: 'value', direction: 'asc' });
  assert.equal(selectionText(rows, data.columns, { row: 2, column: 0 }, { row: 0, column: 0 }), '"a\nline"\n"b\t""quoted"""\nNULL');
  assert.equal(selectionText(rows, data.columns, { row: 0, column: 0 }, { row: 0, column: 0 }), 'a\nline');
});

test('JSON inspection formats without rounding numeric tokens or altering string whitespace', () => {
  const value = '{"n": 9007199254740993, "text": "a  b", "empty": [], "nested": {"x":true}}';
  const formatted = prettyCell(value, column('jsonb'), true);
  assert.match(formatted, /9007199254740993/);
  assert.match(formatted, /"a  b"/);
  assert.match(formatted, /"empty": \[\]/);
  assert.deepEqual(JSON.parse(formatted), JSON.parse(value));
});

test('10,000 loaded rows render a bounded viewport while spacers retain the full scroll height', () => {
  for (const scrollTop of [0, 500, 100_000, 329_500]) {
    const window = visibleRowWindow(10_000, scrollTop, 500);
    assert.ok(window.last - window.first <= 37);
    assert.equal(window.top + (window.last - window.first) * 33 + window.bottom, 330_000);
  }
  const final = visibleRowWindow(10_000, 329_549, 500);
  assert.equal(final.last, 10_000);
  assert.equal(final.bottom, 0);
  assert.deepEqual(visibleRowWindow(0, 0, 300), { first: 0, last: 0, top: 0, bottom: 0 });
});

test('reversed rectangular selection copies the visible cells across multiple columns', () => {
  const columns = [column('text', { name: 'id' }), column('text', { name: 'value' })];
  const rows = [{ originalIndex: 4, row: { id: '5', value: '' } }, { originalIndex: 0, row: { id: '1', value: 'a\nb' } }];
  assert.equal(selectionText(rows, columns, { row: 1, column: 1 }, { row: 0, column: 0 }), '5\t\n1\t"a\nb"');
});

test('legacy JSON string scalars stay distinct from current raw JSON literals in display and copy', () => {
  for (const value of ['null', '123', '[1]', 'plain string', '']) {
    assert.equal(cellText(value, column('jsonb')), JSON.stringify(value));
    assert.equal(prettyCell(value, column('jsonb')), JSON.stringify(value));
    const data = result([value], 'jsonb');
    assert.equal(selectionText(sortedRows(data, null), data.columns, { row: 0, column: 0 }, { row: 0, column: 0 }), JSON.stringify(value));
  }
  assert.equal(prettyCell('null', column('jsonb'), true), 'null');
  assert.equal(prettyCell('123', column('jsonb'), true), '123');
  assert.equal(prettyCell('[1]', column('jsonb'), true), '[\n  1\n]');
  assert.equal(prettyCell(null, column('jsonb'), true), 'NULL');
});
