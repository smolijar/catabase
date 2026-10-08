import assert from 'node:assert/strict';
import test from 'node:test';
import { formatRunningDuration } from '../src/lib/status.ts';

test('running duration advances from the operation start as clock ticks arrive', () => {
  const startedAt = 10_000;
  assert.deepEqual([10_000, 10_100, 11_200, 75_400].map((now) => formatRunningDuration(startedAt, now)), ['0.0s', '0.1s', '1.2s', '65.4s']);
});

test('new operations reset elapsed time and absent or backwards clocks never show negative time', () => {
  assert.equal(formatRunningDuration(75_400, 75_400), '0.0s');
  assert.equal(formatRunningDuration(null, 75_400), '0.0s');
  assert.equal(formatRunningDuration(undefined, 75_400), '0.0s');
  assert.equal(formatRunningDuration(75_400, 75_000), '0.0s');
});

test('transaction duration uses its server timestamp and advances while idle between queries', () => {
  const startedAt = '2026-10-08T13:45:00+02:00';
  const startMs = Date.parse(startedAt);
  assert.deepEqual([0, 100, 1200, 65_400].map((elapsed) => formatRunningDuration(startedAt, startMs + elapsed)), ['0.0s', '0.1s', '1.2s', '65.4s']);
  // Starting another query does not reset the open transaction's clock.
  assert.equal(formatRunningDuration(startMs + 65_400, startMs + 66_600), '1.2s');
  assert.equal(formatRunningDuration(startedAt, startMs + 66_600), '66.6s');
  assert.equal(formatRunningDuration('2026-10-08T11:45:00Z', startMs + 66_600), '66.6s');
});

test('missing, malformed or future transaction timestamps do not display NaN or negative durations', () => {
  const nowMs = Date.parse('2026-10-08T11:45:00Z');
  for (const startedAt of [null, undefined, '', 'not a date', Number.NaN, '2026-10-08T11:46:00Z']) {
    assert.equal(formatRunningDuration(startedAt, nowMs), '0.0s');
  }
});
