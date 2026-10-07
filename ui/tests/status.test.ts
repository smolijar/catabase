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
