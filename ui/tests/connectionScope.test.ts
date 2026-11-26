import assert from 'node:assert/strict';
import test from 'node:test';
import { connectionTargets, newTabConnection, restoreConnectionScope, switchTabConnection } from '../src/lib/connectionScope.ts';
import { createQueryTab, createResultTab } from '../src/lib/workspace.ts';

const tabs = [createQueryTab('a', 'A', 'select 1', 'dev'), createQueryTab('b', 'B', 'select 2', 'prod'), createQueryTab('c', 'C')];
tabs[1].connectionLocked = true;

test('connection switching defaults to the focused tab, including unrecognized preferences', () => {
  for (const raw of [null, '', 'tab', 'invalid']) assert.equal(restoreConnectionScope(raw), 'tab');
  assert.equal(restoreConnectionScope('global'), 'global');
  assert.deepEqual(connectionTargets(tabs, 'a', 'tab').map(tab => tab.id), ['a']);
});

test('global switching includes blank tabs but excludes locks; a focused lock blocks selection entirely', () => {
  assert.deepEqual(connectionTargets(tabs, 'a', 'global').map(tab => tab.id), ['a', 'c']);
  for (const scope of ['tab', 'global'] as const) {
    assert.deepEqual(connectionTargets(tabs, 'b', scope), []);
    assert.deepEqual(connectionTargets(tabs, 'missing', scope), []);
  }
});

test('a new tab uses the global selection even while a locked exception is focused', () => {
  assert.equal(newTabConnection('global', 'dev', 'prod', true), 'dev');
  assert.equal(newTabConnection('global', null, 'prod', true), null);
  assert.equal(newTabConnection('global', null, 'dev', false), 'dev');
  assert.equal(newTabConnection('tab', 'dev', 'prod', true), 'prod');
});

test('switching restores staged values across result tabs while retaining each result source', () => {
  const tab = createQueryTab('a', 'A', '', 'dev');
  const context = { connectionId: 'dev', connectionName: 'Development', execution: { sql: 'select 1', start: 0, end: 8 }, executedAt: '', snapshot: false };
  const data = { columns: [], rows: [{ id: 1, value: 'original' }], rowCount: 1, durationMs: 1, autoLimited: false, hasMore: false };
  const result = createResultTab('live', context, data);
  result.pendingChanges = [{ rowIndex: 0, column: 'value', oldValue: 'original', newValue: 'edited' }];
  result.editMode = true;
  result.data = { ...data, rows: [{ id: 1, value: 'edited' }] };
  const snapshot = createResultTab('saved', { ...context, snapshot: true }, data);
  tab.results = [result, { ...result, id: 'other-live' }, snapshot];
  const switched = switchTabConnection(tab, 'other');
  assert.equal(switched.connectionId, 'other');
  for (const item of switched.results!.slice(0, 2)) {
    assert.deepEqual(item.pendingChanges, []);
    assert.equal(item.editMode, false);
    assert.equal(item.context.connectionId, 'dev');
    assert.equal(item.data?.rows[0].value, 'original');
  }
  assert.equal(tab.results[0].data?.rows[0].value, 'edited');
  assert.equal(switched.results![2], snapshot);
});
