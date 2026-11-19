import assert from 'node:assert/strict';
import test from 'node:test';
import { restoreLastConnectionId, resumeConnection } from '../src/lib/connectionRun.ts';
import type { ConnectionRecord } from '../src/lib/types.ts';

const records = [
  {id: 'production', name: 'Production', username: 'admin'},
  {id: 'development', name: 'Development', username: 'dev'}
] as ConnectionRecord[];

test('a disconnected tab reviews its own saved connection instead of another tab’s latest choice', () => {
  assert.equal(resumeConnection(records, 'development', 'production')?.id, 'development');
  assert.equal(resumeConnection(records, null, 'development')?.id, 'development');
});

test('missing or deleted connection IDs never silently select the first connection', () => {
  assert.equal(resumeConnection(records, null, null), null);
  assert.equal(resumeConnection(records, 'deleted', 'also-deleted'), null);
  assert.equal(resumeConnection(records, 'deleted', 'development')?.id, 'development');
  for (const value of [null, '', ' ', 'x'.repeat(129)]) assert.equal(restoreLastConnectionId(value), null);
  assert.equal(restoreLastConnectionId('development'), 'development');
});

test('a locked tab can reconnect only its saved connection, never the latest different connection', () => {
  assert.equal(resumeConnection(records, 'development', 'production', true)?.id, 'development');
  assert.equal(resumeConnection(records, 'deleted', 'production', true), null);
});
