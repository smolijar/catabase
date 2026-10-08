import assert from 'node:assert/strict';
import test from 'node:test';
import { assessRowEditing, buildRowUpdates, originalEditColumns } from '../src/lib/rowEditing.ts';
import type { ColumnMeta, QueryResult, RowKey } from '../src/lib/types.ts';

const column = (name: string, more: Partial<ColumnMeta> = {}): ColumnMeta => ({ name, sourceName: name, tableSchema: 'public', tableName: 'items', typeName: 'text', typeOid: 25, generated: false, primaryKey: false, ...more });
const key = (columns: string[], more: Partial<RowKey> = {}): RowKey => ({ schema: 'public', table: 'items', columns, primaryKey: true, nullsNotDistinct: false, ...more });
const data = (columns: ColumnMeta[] = [column('id', { primaryKey: true }), column('name')], rowKeys: RowKey[] = [key(['id'])], rows: Record<string, unknown>[] = [{ id: 'one', name: 'old' }]): QueryResult => ({ columns, rowKeys, rows, rowCount: rows.length, durationMs: 1, hasMore: false, autoLimited: false });

test('a partial composite PK is disabled even when a projected column has its PK flag', () => {
  const result = data([column('id', { primaryKey: true }), column('name')], [key(['id', 'tenant_id'])]);
  assert.equal(assessRowEditing(result).target, null);
  assert.match(assessRowEditing(result).reason, /Missing a complete primary key or unique key/);
  result.columns.push(column('tenant_id'));
  result.rows[0].tenant_id = 'tenant';
  assert.deepEqual(assessRowEditing(result).target?.key.columns, ['id', 'tenant_id']);
});

test('a complete unique key works without the PK and a complete PK is preferred', () => {
  const unique = key(['name'], { primaryKey: false });
  const result = data([column('id'), column('name')], [unique, key(['id'])]);
  assert.equal(assessRowEditing(result).target?.key.primaryKey, true);
  result.columns = [column('name')];
  assert.equal(assessRowEditing(result).target?.key.primaryKey, false);
});

test('composite unique keys require all components and safe values in every loaded row', () => {
  const result = data([column('id'), column('name')], [key(['id', 'name'], { primaryKey: false })]);
  assert.ok(assessRowEditing(result).target);
  result.rows.push({ id: 'two', name: null });
  assert.equal(assessRowEditing(result).target, null);
  result.rowKeys![0].nullsNotDistinct = true;
  assert.ok(assessRowEditing(result).target);
  delete result.rows[1].name;
  assert.equal(assessRowEditing(result).target, null);
});

test('duplicate source rows and ambiguous aliases cannot enter edit mode', () => {
  const result = data();
  result.rows.push({ id: 'one', name: 'other' });
  assert.match(assessRowEditing(result).reason, /repeats the same row key/);
  result.rows.pop();
  result.columns.push(column('alias', { sourceName: 'id' }));
  assert.match(assessRowEditing(result).reason, /ambiguous/);
  result.columns[2] = column('name', { sourceName: 'other' });
  assert.match(assessRowEditing(result).reason, /ambiguous/);
});

test('expressions, generated values and unsupported placeholders are read-only', () => {
  const result = data();
  result.columns.push(column('expression', { sourceName: null, tableSchema: null, tableName: null }), column('generated', { generated: true }), column('array', { typeName: '_text' }));
  assert.deepEqual(assessRowEditing(result).target?.editableColumns.map((column) => column.name), ['id', 'name']);
  result.columns[0].typeName = 'custom_enum';
  assert.match(assessRowEditing(result).reason, /type that cannot be safely edited/);
});

test('multi-table, no-key and legacy results fail closed rather than guessing uniqueness', () => {
  const result = data();
  result.columns[1].tableName = 'other';
  assert.equal(assessRowEditing(result).target, null);
  result.columns[1].tableName = 'items';
  delete result.rowKeys;
  assert.equal(assessRowEditing(result).target, null);
  result.rowKeys = [key(['id'], { table: 'other' })];
  assert.equal(assessRowEditing(result).target, null);
});

test('aliased keys and edits use source names, only key plus changed originals are sent', () => {
  const result = data([column('item_id', { sourceName: 'id' }), column('label', { sourceName: 'name' }), column('unrelated')], [key(['id'])], [{ item_id: 'old-key', label: 'old', unrelated: '<_text>' }]);
  const target = assessRowEditing(result).target!;
  const changes = [{ rowIndex: 0, column: 'label', oldValue: 'old', newValue: 'new' }];
  assert.deepEqual(buildRowUpdates(target, result.rows, changes), [{ schema: 'public', table: 'items', keyColumns: ['id'], values: { name: 'new' }, original: { id: 'old-key', name: 'old' } }]);
  changes.push({ rowIndex: 0, column: 'item_id', oldValue: 'old-key', newValue: 'new-key' });
  assert.equal(originalEditColumns(target, changes).length, 2);
  assert.equal(buildRowUpdates(target, result.rows, changes)[0].original.id, 'old-key');
  assert.equal(buildRowUpdates(target, result.rows, changes)[0].values.id, 'new-key');
  assert.throws(() => buildRowUpdates(target, result.rows, [{ ...changes[0], column: 'computed' }]), /not editable/);
});
