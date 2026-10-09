import assert from 'node:assert/strict';
import test from 'node:test';
import { createForeignKeyLookup, foreignKeyDraft, foreignKeyTargets, foreignKeyValuesMatch } from '../src/lib/foreignKeyEditing.ts';
import type { ForeignKeyLookupState } from '../src/lib/foreignKeyEditing.ts';
import type { ColumnMeta, ForeignKeyLookup, ForeignKeyValues, SchemaForeignKey, SchemaItem } from '../src/lib/types.ts';

const column = (sourceName: string, name = sourceName, more: Partial<ColumnMeta> = {}): ColumnMeta => ({ name, sourceName, typeName: 'text', typeOid: 25, tableSchema: 'public', tableName: 'children', nullable: true, generated: false, primaryKey: false, ...more });
const parent = column('parent_id', 'parent');
const tenant = column('tenant_id', 'tenant');
const key: SchemaForeignKey = { name: 'parent_fk', columns: ['parent_id', 'tenant_id'], referencedSchema: 'pms', referencedTable: 'parents', referencedColumns: ['id', 'tenant'] };
const source: SchemaItem = { schema: 'public', name: 'children', kind: 'table', columns: [], foreignKeys: [key] };
const data: ForeignKeyValues = { foreignKey: key, previewColumns: ['name'], rows: [{ id: '9223372036854775807', tenant: 'team', name: 'Parent' }], hasMore: false };
const request: ForeignKeyLookup = { schema: 'public', table: 'children', constraint: 'parent_fk', search: '' };

test('FK previews use exact source metadata and map composite keys to projected aliases', () => {
  const targets = foreignKeyTargets(parent, [parent, tenant], [source], ['parent', 'tenant']);
  assert.equal(targets.length, 1);
  assert.equal(targets[0].unavailableReason, '');
  assert.deepEqual(foreignKeyDraft(targets[0], data.rows[0]), { parent: '9223372036854775807', tenant: 'team' });
  assert.ok(foreignKeyValuesMatch(targets[0], data));
  assert.equal(foreignKeyTargets(column('name'), [parent, tenant], [source], ['parent', 'tenant']).length, 0);
  assert.equal(foreignKeyTargets({ ...parent, sourceName: null }, [parent], [source], ['parent']).length, 0);
  assert.equal(foreignKeyTargets({ ...parent, tableSchema: 'other' }, [parent], [source], ['parent']).length, 0);
});

test('partial, ambiguous and non-editable FK components explain why choosing a tuple is disabled', () => {
  for (const [columns, editable] of [
    [[parent], ['parent']], [[parent, tenant], ['parent']], [[parent, tenant, { ...tenant, name: 'duplicate' }], ['parent', 'tenant', 'duplicate']]
  ] as [ColumnMeta[], string[]][]) {
    const target = foreignKeyTargets(parent, columns, [source], editable)[0];
    assert.match(target.unavailableReason, /Missing: tenant_id/);
    assert.throws(() => foreignKeyDraft(target, data.rows[0]), /Missing/);
  }
});

test('multiple real relationships remain separate, and malformed FK metadata is never inferred', () => {
  const other = { ...key, name: 'other_fk', referencedTable: 'other_parents' };
  assert.deepEqual(foreignKeyTargets(parent, [parent, tenant], [{ ...source, foreignKeys: [key, other] }], ['parent', 'tenant']).map((target) => target.key.name), ['parent_fk', 'other_fk']);
  for (const malformed of [{ ...key, referencedColumns: ['id'] }, { ...key, columns: ['parent_id', 'parent_id'] }, { ...key, referencedColumns: ['id', 'id'] }]) {
    assert.deepEqual(foreignKeyTargets(parent, [parent, tenant], [{ ...source, foreignKeys: [malformed] }], ['parent', 'tenant']), []);
  }
});

test('missing or NULL key components cannot be selected, and changed catalog metadata fails closed', () => {
  const target = foreignKeyTargets(parent, [parent, tenant], [source], ['parent', 'tenant'])[0];
  assert.throws(() => foreignKeyDraft(target, { id: '1' }), /incomplete key/);
  assert.throws(() => foreignKeyDraft(target, { id: null, tenant: 'team' }), /incomplete key/);
  for (const changed of [{ ...key, name: 'other' }, { ...key, referencedSchema: 'other' }, { ...key, referencedTable: 'other' }, { ...key, referencedColumns: ['tenant', 'id'] }, { ...key, columns: ['tenant_id', 'parent_id'] }]) {
    assert.equal(foreignKeyValuesMatch(target, { ...data, foreignKey: changed }), false);
  }
});

test('a newer search cancels the old lookup and late results cannot replace its preview', async () => {
  const states: ForeignKeyLookupState[] = [];
  const cancelled: string[] = [];
  let resolveFirst: (value: ForeignKeyValues) => void = () => {};
  let nextToken = 0;
  const lookup = createForeignKeyLookup((request) => request.search === 'new' ? Promise.resolve({ ...data, rows: [{ id: 'new', tenant: 'team' }] }) : new Promise((resolve) => { resolveFirst = resolve; }), (state) => states.push(state), (token) => cancelled.push(token), () => `token-${++nextToken}`);
  const first = lookup.run(request);
  await lookup.run({ ...request, search: 'new' });
  resolveFirst(data);
  await first;
  assert.deepEqual(cancelled, ['token-1']);
  assert.equal(states.at(-1)?.data?.rows[0].id, 'new');
  assert.equal(states.filter((state) => state.data).length, 1);
});

test('closing/changing cells invalidates pending lookups, and current errors remain retryable', async () => {
  const states: ForeignKeyLookupState[] = [];
  let rejectFirst: (error: unknown) => void = () => {};
  let calls = 0;
  const cancelled: string[] = [];
  const lookup = createForeignKeyLookup(() => {
    calls++;
    if (calls === 1) return new Promise((_, reject) => { rejectFirst = reject; });
    if (calls === 2) return Promise.reject({ message: 'Permission denied' });
    return Promise.resolve(data);
  }, (state) => states.push(state), (token) => cancelled.push(token), () => 'token');
  const first = lookup.run(request);
  lookup.invalidate();
  rejectFirst({ message: 'Old cancellation' });
  await first;
  assert.deepEqual(cancelled, ['token']);
  assert.equal(states.length, 1);
  await lookup.run(request);
  assert.equal(states.at(-1)?.error, 'Permission denied');
  await lookup.run(request);
  assert.equal(states.at(-1)?.data, data);
  assert.equal(states.at(-1)?.error, '');
  lookup.invalidate();
  assert.deepEqual(cancelled, ['token']); // Completed requests are never cancelled later.
});
