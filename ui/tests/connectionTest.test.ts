import assert from 'node:assert/strict';
import test from 'node:test';
import { connectionHealth, createConnectionTester, queryReachedServer } from '../src/lib/connectionTest.ts';
import type { ConnectionTestState } from '../src/lib/connectionTest.ts';

function deferred() {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function harness() {
  const states = new Map<string, ConnectionTestState | null>();
  const changes: Array<[string, ConnectionTestState | null]> = [];
  const tester = createConnectionTester((id, state) => {
    states.set(id, state);
    changes.push([id, state]);
  }, (error) => (error as Error).message, () => 12345);
  return { ...tester, states, changes };
}

test('testing starts immediately, reports failure, and a retry clears the error before succeeding', async () => {
  const tester = harness();
  const first = deferred();
  const result = tester.test('zenbase', () => first.promise);
  assert.deepEqual(tester.states.get('zenbase'), { status: 'testing' });
  first.reject(new Error('Connection refused'));
  await result;
  assert.deepEqual(tester.states.get('zenbase'), { status: 'error', message: 'Connection refused', checkedAt: 12345 });

  const retry = deferred();
  const retried = tester.test('zenbase', () => retry.promise);
  assert.deepEqual(tester.states.get('zenbase'), { status: 'testing' });
  retry.resolve();
  await retried;
  assert.deepEqual(tester.states.get('zenbase'), { status: 'success', checkedAt: 12345 });
});

test('repeated clicks share the in-flight test instead of opening duplicate connections', async () => {
  const tester = harness();
  const probe = deferred();
  let calls = 0;
  const first = tester.test('zenbase', () => { calls++; return probe.promise; });
  const second = tester.test('zenbase', () => { calls++; return Promise.resolve(); });
  assert.equal(first, second);
  await Promise.resolve();
  assert.equal(calls, 1);
  probe.resolve();
  await first;
  assert.deepEqual(tester.changes.map(([, state]) => state?.status), ['testing', 'success']);
});

test('late results stay scoped to their connection rather than another visible tab', async () => {
  const tester = harness();
  const first = deferred();
  const second = deferred();
  const a = tester.test('first', () => first.promise);
  const b = tester.test('second', () => second.promise);
  second.resolve();
  await b;
  first.reject(new Error('First connection failed'));
  await a;
  assert.deepEqual(tester.states.get('first'), { status: 'error', message: 'First connection failed', checkedAt: 12345 });
  assert.deepEqual(tester.states.get('second'), { status: 'success', checkedAt: 12345 });
});

test('editing a connection invalidates an old probe even if it finishes after the replacement', async () => {
  const tester = harness();
  const old = deferred();
  const oldResult = tester.test('zenbase', () => old.promise);
  tester.invalidate('zenbase');
  assert.equal(tester.states.get('zenbase'), null);
  await tester.test('zenbase', () => Promise.resolve());
  old.reject(new Error('Stale credentials'));
  await oldResult;
  assert.deepEqual(tester.states.get('zenbase'), { status: 'success', checkedAt: 12345 });
});

test('deleting a connection prevents its late success from restoring a health indicator', async () => {
  const tester = harness();
  const probe = deferred();
  const result = tester.test('zenbase', () => probe.promise);
  tester.invalidate('zenbase');
  probe.resolve();
  await result;
  assert.equal(tester.states.get('zenbase'), null);
});

test('synchronous probe failures also become retryable error states', async () => {
  const tester = harness();
  await tester.test('zenbase', () => { throw new Error('Connection test timed out.'); });
  assert.deepEqual(tester.states.get('zenbase'), { status: 'error', message: 'Connection test timed out.', checkedAt: 12345 });
  await tester.test('zenbase', () => Promise.resolve());
  assert.deepEqual(tester.states.get('zenbase'), { status: 'success', checkedAt: 12345 });
});

test('checks turn stale at one minute while testing/untested states stay distinct', () => {
  assert.equal(connectionHealth(null, 999999), 'unknown');
  assert.equal(connectionHealth({ status: 'testing' }, 999999), 'testing');
  for (const state of [{ status: 'success', checkedAt: 1000 }, { status: 'error', message: 'offline', checkedAt: 1000 }] as const) {
    assert.equal(connectionHealth(state, 60999), state.status);
    assert.equal(connectionHealth(state, 61000), 'stale');
  }
});

test('queries refresh the shared connection state without an extra probe or changing their result', async () => {
  const tester = harness();
  const pending = deferred();
  const query = tester.query('zenbase', () => pending.promise);
  assert.equal(tester.states.get('zenbase')?.status, 'running');
  assert.equal(connectionHealth(tester.states.get('zenbase')!, 999999), 'running');
  let probes = 0;
  await tester.test('zenbase', async () => { probes++; });
  assert.equal(probes, 0, 'a running query must not open another test connection');
  pending.resolve();
  await query;
  assert.deepEqual(tester.states.get('zenbase'), { status: 'success', checkedAt: 12345 });
  assert.equal(await tester.query('zenbase', async () => 42), 42);
});

test('SQL errors and cancellations keep health green; transport/authentication failures turn it red', async () => {
  const tester = harness();
  for (const code of ['42601', '42P01', '57014', '40001']) {
    const error = { code, severity: 'ERROR', message: 'Statement failed' };
    assert.equal(queryReachedServer(error), true);
    await assert.rejects(tester.query('zenbase', async () => { throw error; }));
    assert.equal(tester.states.get('zenbase')?.status, 'success');
  }
  for (const error of [{ code: '08006', severity: 'ERROR', message: 'Disconnected' }, { code: '28P01', severity: 'FATAL', message: 'Bad password' }, new Error('Connection refused')]) {
    assert.equal(queryReachedServer(error), false);
    await assert.rejects(tester.query('zenbase', async () => { throw error; }));
    assert.equal(tester.states.get('zenbase')?.status, 'error');
  }
});

test('a newer query supersedes a pending check; deletion invalidates pending query health', async () => {
  const tester = harness();
  const old = deferred();
  const checking = tester.test('zenbase', () => old.promise);
  await tester.query('zenbase', async () => 1);
  old.reject(new Error('Obsolete failure'));
  await checking;
  assert.equal(tester.states.get('zenbase')?.status, 'success');
  const pending = deferred();
  const query = tester.query('zenbase', () => pending.promise);
  tester.invalidate('zenbase');
  pending.resolve();
  await query;
  assert.equal(tester.states.get('zenbase'), null);
});
