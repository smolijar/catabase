import assert from 'node:assert/strict';
import test from 'node:test';
import { connectionTargets, newTabConnection, restoreConnectionScope, switchTabConnection } from '../src/lib/connectionScope.ts';
import { createQueryTab } from '../src/lib/workspace.ts';

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

test('switching restores staged values without changing the result source or mutating the old tab', () => {
  const tab = createQueryTab('a', 'A', '', 'dev');
  tab.originalRows = [{ id: 1, value: 'original' }];
  tab.pendingChanges = [{ rowIndex: 0, column: 'value', oldValue: 'original', newValue: 'edited' }];
  tab.editMode = true;
  tab.session = { connectionId: 'dev', connectionName: 'Development', execution: { sql: 'select 1', start: 0, end: 8 }, executedAt: '', snapshot: false,
    data: { columns: [], rows: [{ id: 1, value: 'edited' }], rowCount: 1, durationMs: 1, autoLimited: false, hasMore: false } };
  const switched = switchTabConnection(tab, 'other');
  assert.equal(switched.connectionId, 'other');
  assert.deepEqual(switched.pendingChanges, []);
  assert.equal(switched.editMode, false);
  assert.equal(switched.session?.connectionId, 'dev');
  assert.equal(switched.session?.data.rows[0].value, 'original');
  assert.equal(tab.session.data.rows[0].value, 'edited');
  assert.notEqual(switched.session?.data.rows, tab.originalRows);
  const history = { ...tab, kind: 'reference' as const, pendingChanges: [], originalRows: [], session: { ...tab.session, snapshot: true } };
  assert.equal(switchTabConnection(history, 'other').session, history.session);
});
