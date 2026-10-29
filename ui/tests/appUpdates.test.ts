import assert from 'node:assert/strict';
import test from 'node:test';
import { createUpdateChecker, initialUpdateState, UPDATE_CHECK_INTERVAL, updateInstallBlock } from '../src/lib/appUpdates.ts';
import type { UpdateCheck } from '../src/lib/appUpdates.ts';

const idle = { ready: true, busy: false, transactionKnown: true, transactionActive: false, stagedEdits: 0, cellEditing: false };
test('installation requires a restored, idle workspace and no transaction or staged edits', () => {
  assert.equal(updateInstallBlock(idle), '');
  for (const block of [{ ready: false }, { busy: true }, { transactionKnown: false }, { transactionActive: true }, { stagedEdits: 3 }, { cellEditing: true }]) {
    assert.notEqual(updateInstallBlock({ ...idle, ...block }), '');
  }
});

function harness() {
  let now = 100;
  let state = initialUpdateState();
  let calls = 0;
  let fail = false;
  let enabled = true;
  let resolve!: (value: UpdateCheck) => void;
  const checker = createUpdateChecker({
    status: async () => ({ currentVersion: '0.1.0', enabled, unavailableReason: enabled ? null : 'Signing is not configured.' }),
    check: () => {
      calls++;
      if (fail) return Promise.reject(new Error('Offline'));
      return new Promise<UpdateCheck>(yes => resolve = yes);
    }
  }, next => state = next, String, () => now);
  return {
    checker,
    state: () => state,
    calls: () => calls,
    advance() { now += UPDATE_CHECK_INTERVAL; },
    fail() { fail = true; },
    disable() { enabled = false; },
    resolve() { resolve({ currentVersion: '0.1.0', update: { version: '0.1.1', notes: 'Changes', publishedAt: null } }); }
  };
}

test('automatic checks require foreground, idle and opted-in; manual checks coalesce', async () => {
  const h = harness();
  await h.checker.initialize();
  for (const context of [
    { automatic: false, foreground: true, idle: true },
    { automatic: true, foreground: false, idle: true },
    { automatic: true, foreground: true, idle: false }
  ]) h.checker.poll(context);
  assert.equal(h.calls(), 0);
  h.checker.poll({ automatic: true, foreground: true, idle: true });
  const pending = h.checker.check();
  assert.equal(pending, h.checker.check());
  await Promise.resolve();
  assert.equal(h.calls(), 1);
  h.resolve();
  await pending;
  assert.equal(h.state().update?.version, '0.1.1');
  assert.equal(h.state().checking, false);
  h.checker.poll({ automatic: true, foreground: true, idle: true });
  assert.equal(h.calls(), 1);
});

test('failed checks keep a discovered update and back off before automatic retry', async () => {
  const h = harness();
  await h.checker.initialize();
  const first = h.checker.check();
  await Promise.resolve();
  h.resolve();
  await first;
  h.fail();
  h.advance();
  await h.checker.check();
  assert.match(h.state().error, /Offline/);
  assert.equal(h.state().update?.version, '0.1.1');
  h.checker.poll({ automatic: true, foreground: true, idle: true });
  assert.equal(h.calls(), 2);
  h.advance();
  h.checker.poll({ automatic: true, foreground: true, idle: true });
  await h.checker.check();
  assert.equal(h.calls(), 3);
});

test('unconfigured builds never contact the update endpoint', async () => {
  const h = harness();
  h.disable();
  await h.checker.check();
  assert.equal(h.calls(), 0);
  assert.equal(h.state().status?.enabled, false);
  assert.equal(h.state().checking, false);
});

test('late responses after disposal never change mounted UI', async () => {
  const h = harness();
  await h.checker.initialize();
  const pending = h.checker.check();
  await Promise.resolve();
  const previous = h.state();
  h.checker.dispose();
  h.resolve();
  await pending;
  assert.equal(h.state(), previous);
  await h.checker.check();
  assert.equal(h.calls(), 1);
});

test('status initialization failure is retryable and never contacts update endpoint', async () => {
  let attempts = 0;
  let checks = 0;
  let state = initialUpdateState();
  const checker = createUpdateChecker({
    status: async () => {
      if (++attempts === 1) throw new Error('Status unavailable');
      return { enabled: true, currentVersion: '0.1.0', unavailableReason: null };
    },
    check: async () => { checks++; return { currentVersion: '0.1.0', update: null }; }
  }, next => state = next, String);
  await checker.check();
  assert.equal(checks, 0);
  assert.match(state.error, /Status unavailable/);
  await checker.check();
  assert.equal(checks, 1);
  assert.equal(state.error, '');
  assert.ok(state.checkedAt);
});
