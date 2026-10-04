import assert from 'node:assert/strict';
import test from 'node:test';
import { cellText, parseCellDraft, prettyCell, selectionText, sortedRows, visibleRowWindow } from '../src/lib/resultsGrid.ts';
import type { ColumnMeta, QueryResult } from '../src/lib/types.ts';

const column = (typeName = 'text', more: Partial<ColumnMeta> = {}): ColumnMeta => ({ name: 'value', typeName, typeOid: 0, nullable: true, generated: false, primaryKey: false, ...more });
const result = (values: unknown[], typeName = 'text'): QueryResult => ({ columns: [column(typeName)], rows: values.map((value) => ({ value })), rowCount: values.length, durationMs: 1, autoLimited: false, hasMore: false });

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
