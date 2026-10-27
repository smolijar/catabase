import assert from 'node:assert/strict';
import test from 'node:test';
import { createConnectionHealthcheck, createWindowFocusTracker } from '../src/lib/connectionHealthcheck.ts';
import type { HealthcheckContext } from '../src/lib/connectionHealthcheck.ts';
import { createConnectionTester } from '../src/lib/connectionTest.ts';
import type { ConnectionTestState } from '../src/lib/connectionTest.ts';

test('native focus snapshots cannot overwrite a newer blur, focus event, or disposal', async () => {
  const calls: boolean[] = [];
  let resolve!: (value: boolean) => void;
  let reject!: (error: Error) => void;
  const tracker = createWindowFocusTracker(() => new Promise<boolean>((yes, no) => { resolve = yes; reject = no; }), value => calls.push(value));
  const initial = tracker.refresh();
  tracker.set(false);
  resolve(true);
  await initial;
  assert.deepEqual(calls, [false]);

  const failed = tracker.refresh();
  tracker.set(true);
  reject(new Error('snapshot failed'));
  await failed;
  assert.deepEqual(calls, [false, true]);

  const disposed = tracker.refresh();
  tracker.dispose();
  resolve(true);
  await disposed;
  tracker.set(true);
  assert.deepEqual(calls, [false, true]);
});

test('DOM focus is confirmed natively and can recover without another native focus event', async () => {
  let nativeFocused = false;
  let focused = false;
  const tracker = createWindowFocusTracker(async () => nativeFocused, value => focused = value);
  await tracker.refresh();
  assert.equal(focused, false);
  nativeFocused = true;
  await tracker.refresh();
  assert.equal(focused, true);
  tracker.set(false);
  await tracker.refresh();
  assert.equal(focused, true);
});

test('failed native focus lookup defaults to background, never an automatic probe', async () => {
  let focused = true;
  const tracker = createWindowFocusTracker(async () => { throw new Error('unavailable'); }, value => focused = value);
  await tracker.refresh();
  assert.equal(focused, false);
});

function harness() {
  let now = 1000;
  const states = new Map<string, ConnectionTestState | null>();
  const context: Omit<HealthcheckContext, 'state'> = {
    connectionId: 'docker', ready: true, busy: false, transactionIdle: true
  };
  const calls: string[] = [];
  let failure: Error | null = null;
  let pending: Promise<void> = Promise.resolve();
  const tester = createConnectionTester((id, state) => states.set(id, state), String, () => now);
  const monitor = createConnectionHealthcheck(() => ({
    ...context, state: states.get(context.connectionId ?? '') ?? null
  }), id => {
    calls.push(id);
    pending = tester.test(id, async () => { if (failure) throw failure; });
  }, () => now);
  return {
    monitor, context, calls, tester, states,
    advance(ms: number) { now += ms; },
    foreground() { monitor.setVisible(true); monitor.setFocused(true); },
    settled: () => pending,
    fail() { failure = new Error('offline'); }
  };
}

test('initial foreground check and one-minute refresh share the existing tester without duplicate probes', async () => {
  const h = harness();
  h.monitor.poll();
  assert.equal(h.calls.length, 0);
  h.foreground();
  h.monitor.poll();
  h.foreground();
  assert.deepEqual(h.calls, ['docker']);
  await h.settled();
  h.advance(59_999);
  h.monitor.poll();
  assert.equal(h.calls.length, 1);
  h.advance(1);
  h.monitor.poll();
  assert.equal(h.calls.length, 2);
});

test('background, hidden and minimized windows never probe; returning to foreground checks immediately', async () => {
  const h = harness();
  h.foreground();
  await h.settled();
  h.monitor.setFocused(false);
  h.advance(120_000);
  h.monitor.poll();
  h.monitor.setVisible(false);
  h.monitor.setFocused(true);
  h.monitor.poll();
  assert.equal(h.calls.length, 1);
  h.monitor.setVisible(true);
  assert.equal(h.calls.length, 2);
  await h.settled();
  h.monitor.setFocused(false);
  h.advance(120_000);
  h.monitor.setFocused(true);
  assert.equal(h.calls.length, 3);
});

test('startup/closing, queries/cancellation/mutations/dialogs, and active or unknown transactions defer checks', async () => {
  for (const blocked of [{ ready: false }, { busy: true }, { transactionIdle: false }, { connectionId: null }]) {
    const h = harness();
    Object.assign(h.context, blocked);
    h.foreground();
    h.advance(120_000);
    h.monitor.poll();
    assert.equal(h.calls.length, 0, JSON.stringify(blocked));
    Object.assign(h.context, { ready: true, busy: false, transactionIdle: true, connectionId: 'docker' });
    h.monitor.poll();
    assert.equal(h.calls.length, 1);
    await h.settled();
  }
});

test('query traffic refreshes health, so the next focus/poll does not add a database request', async () => {
  const h = harness();
  h.foreground();
  await h.settled();
  h.advance(120_000);
  h.context.busy = true;
  h.monitor.poll();
  await h.tester.query('docker', async () => 1);
  h.context.busy = false;
  h.foreground();
  h.monitor.poll();
  assert.equal(h.calls.length, 1);
});

test('in-flight manual tests and queries are never superseded by an automatic check', async () => {
  for (const status of ['testing', 'running'] as const) {
    const h = harness();
    h.states.set('docker', { status });
    h.foreground();
    h.advance(120_000);
    h.monitor.poll();
    assert.equal(h.calls.length, 0);
  }
});

test('failed checks wait a full minute before retrying, even with repeated focus events', async () => {
  const h = harness();
  h.fail();
  h.foreground();
  await h.settled();
  assert.equal(h.states.get('docker')?.status, 'error');
  h.foreground();
  h.monitor.poll();
  h.advance(59_999);
  h.monitor.poll();
  assert.equal(h.calls.length, 1);
  h.advance(1);
  h.monitor.poll();
  assert.equal(h.calls.length, 2);
});

test('only the current connection is tested; disconnecting or disposing disables future checks', async () => {
  const h = harness();
  h.foreground();
  await h.settled();
  h.context.connectionId = null;
  h.advance(120_000);
  h.monitor.poll();
  assert.deepEqual(h.calls, ['docker']);
  h.context.connectionId = 'other';
  h.monitor.poll();
  await h.settled();
  assert.deepEqual(h.calls, ['docker', 'other']);
  h.monitor.dispose();
  h.advance(120_000);
  h.foreground();
  h.monitor.poll();
  assert.equal(h.calls.length, 2);
});

test('a check already in flight can finish after blur but cannot schedule background retries', async () => {
  const h = harness();
  h.foreground();
  h.monitor.setFocused(false);
  await h.settled();
  h.advance(120_000);
  h.monitor.poll();
  assert.equal(h.calls.length, 1);
  assert.equal(h.states.get('docker')?.status, 'success');
});
