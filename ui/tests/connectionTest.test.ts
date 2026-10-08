import assert from 'node:assert/strict';
import test from 'node:test';
import { createConnectionTester } from '../src/lib/connectionTest.ts';
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
  }, (error) => (error as Error).message);
  return { ...tester, states, changes };
}

test('testing starts immediately, reports failure, and a retry clears the error before succeeding', async () => {
  const tester = harness();
  const first = deferred();
  const result = tester.test('zenbase', () => first.promise);
  assert.deepEqual(tester.states.get('zenbase'), { status: 'testing' });
  first.reject(new Error('Connection refused'));
  await result;
  assert.deepEqual(tester.states.get('zenbase'), { status: 'error', message: 'Connection refused' });

  const retry = deferred();
  const retried = tester.test('zenbase', () => retry.promise);
  assert.deepEqual(tester.states.get('zenbase'), { status: 'testing' });
  retry.resolve();
  await retried;
  assert.deepEqual(tester.states.get('zenbase'), { status: 'success' });
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
  assert.deepEqual(tester.states.get('first'), { status: 'error', message: 'First connection failed' });
  assert.deepEqual(tester.states.get('second'), { status: 'success' });
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
  assert.deepEqual(tester.states.get('zenbase'), { status: 'success' });
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
  assert.deepEqual(tester.states.get('zenbase'), { status: 'error', message: 'Connection test timed out.' });
  await tester.test('zenbase', () => Promise.resolve());
  assert.deepEqual(tester.states.get('zenbase'), { status: 'success' });
});
