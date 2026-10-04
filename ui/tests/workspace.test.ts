import assert from 'node:assert/strict';
import test from 'node:test';
import { canUseResult, createQueryTab, restoreWorkspace, serializeWorkspace } from '../src/lib/workspace.ts';

test('restored tabs retain their SQL and connection, including an intentionally empty draft', () => {
  const first = createQueryTab('one', 'Orders', 'select 1;', 'db-one');
  const second = createQueryTab('two', 'Blank', '', 'db-two');
  const restored = restoreWorkspace(serializeWorkspace([first, second], 'two'))!;
  assert.equal(restored.activeId, 'two');
  assert.deepEqual(restored.tabs.map(({ sql, connectionId }) => ({ sql, connectionId })), [{ sql: 'select 1;', connectionId: 'db-one' }, { sql: '', connectionId: 'db-two' }]);
});

test('result rows and pending edits are not persisted; reference tabs do not replace query drafts', () => {
  const tab = createQueryTab('one', 'Query', 'select 1;', 'db');
  tab.pendingChanges = [{ rowIndex: 0, column: 'private', oldValue: 'secret', newValue: 'changed' }];
  const reference = { ...createQueryTab('history', 'History', 'select 2;'), kind: 'reference' as const };
  const stored = serializeWorkspace([tab, reference], 'history');
  assert.ok(!stored.includes('secret'));
  const restored = restoreWorkspace(stored)!;
  assert.equal(restored.activeId, 'one');
  assert.equal(restored.tabs.length, 1);
  assert.deepEqual(restored.tabs[0].pendingChanges, []);
});

test('historical results and results from another connection cannot be extended or edited', () => {
  const session = { data: { rows: [], columns: [], rowCount: 0, durationMs: 0, autoLimited: false, hasMore: false }, execution: { sql: 'select 1;', start: 0, end: 9 }, connectionId: 'one', connectionName: 'One', executedAt: '', snapshot: false };
  assert.equal(canUseResult(session, 'one'), true);
  assert.equal(canUseResult(session, 'two'), false);
  assert.equal(canUseResult({ ...session, snapshot: true }, 'one'), false);
  assert.equal(canUseResult(null, 'one'), false);
});

test('invalid storage and duplicate ids are handled without losing valid tabs', () => {
  assert.equal(restoreWorkspace('not json'), null);
  const restored = restoreWorkspace(JSON.stringify({ version: 1, tabs: [{ id: 'a', title: 'A', sql: '' }, { id: 'a', title: 'Duplicate', sql: 'bad' }, { id: 3 }] }))!;
  assert.equal(restored.tabs.length, 1);
  assert.equal(restored.tabs[0].title, 'A');
});
