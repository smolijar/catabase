import assert from 'node:assert/strict';
import test from 'node:test';
import { canUseResult, createDraftWriter, createQueryTab, restoreWorkspace, serializeWorkspace } from '../src/lib/workspace.ts';

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

test('closing the last query while references remain restores a blank workspace', () => {
  const reference = { ...createQueryTab('history', 'History', 'select 2;', 'db'), kind: 'reference' as const };
  const restored = restoreWorkspace(serializeWorkspace([reference], 'history'))!;
  assert.ok(restored);
  assert.equal(restored.tabs.length, 1);
  assert.equal(restored.tabs[0].kind, 'query');
  assert.equal(restored.tabs[0].sql, '');
  assert.equal(restored.activeId, restored.tabs[0].id);
  assert.equal(restoreWorkspace(JSON.stringify({ version: 1, tabs: [{ invalid: true }] })), null);
});

test('workspace saves cannot overwrite a newer draft by completing out of order', async () => {
  const started: string[] = [];
  let finishFirst: () => void = () => {};
  let saved = '';
  const write = createDraftWriter(async (content) => {
    started.push(content);
    if (content === 'older') await new Promise<void>((resolve) => { finishFirst = resolve; });
    saved = content;
  });
  const first = write('older');
  const latest = write('latest');
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(started, ['older']);
  finishFirst();
  await Promise.all([first, latest]);
  assert.deepEqual(started, ['older', 'latest']);
  assert.equal(saved, 'latest');
});

test('closing can retry a draft after an earlier autosave failure', async () => {
  let calls = 0;
  let saved = '';
  const write = createDraftWriter(async (content) => {
    calls += 1;
    if (calls === 1) throw new Error('Temporary storage failure');
    saved = content;
  });
  await assert.rejects(write('unsaved'), /Temporary storage failure/);
  await write('retry on close');
  assert.equal(saved, 'retry on close');
});
