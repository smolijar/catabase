import assert from 'node:assert/strict';
import test from 'node:test';
import { formatTiming, timingRows } from '../src/lib/queryTiming.ts';
import type { QueryResult, QueryTimings } from '../src/lib/types.ts';

const timing: QueryTimings = { prepareMs: 10, metadataMs: 1, executeFetchMs: 420, decodeMs: 12, totalMs: 443, metadataSource: 'cache', metadataQueries: 0, command: { setupMs: 20, historyMs: 30, totalMs: 493 } };
const result: QueryResult = { columns: [], rows: [], rowCount: 199, durationMs: 443, autoLimited: true, hasMore: false, timings: timing };
const client = { targetMs: 2, invokeMs: 510, uiMs: 8, totalMs: 520 };

test('breakdown accounts for the query, surrounding request work and DOM update without double counting', () => {
  const rows = timingRows(result, client);
  assert.deepEqual(rows.map((row) => [row.label, row.ms]), [
    ['Choose SQL', 2], ['Connection & setup', 20], ['Prepare SQL', 10], ['Result metadata', 1],
    ['Execute & fetch', 420], ['Decode rows', 12], ['Save history', 30], ['Response & IPC', 17], ['Result UI update', 8]
  ]);
  assert.equal(rows.reduce((sum, row) => sum + row.ms, 0), client.totalMs);
  assert.equal(rows.filter((row) => row.estimated).length, 1);
  assert.match(rows.find((row) => row.label === 'Execute & fetch')!.note, /cannot be separated/);
  assert.match(rows.at(-1)!.note, /paint is not included/);
});

test('reflection cache hits, refreshes and computed results explain metadata work accurately', () => {
  const metadata = (more: Partial<QueryTimings>) => timingRows({ ...result, timings: { ...timing, ...more } }, client).find((row) => row.label === 'Result metadata')!;
  assert.match(metadata({}).note, /cache hit · 0 metadata queries/);
  assert.match(metadata({ metadataSource: 'refreshed', metadataQueries: 2 }).note, /refreshed · 2 metadata queries/);
  assert.match(metadata({ metadataSource: 'refreshed', metadataQueries: 2 }).note, /60 seconds/);
  assert.match(metadata({ metadataSource: 'notNeeded' }).note, /No source-table metadata/);
});

test('old snapshots and core-only timings have an honest combined residual rather than invented stages', () => {
  const legacy = timingRows({ ...result, timings: undefined }, client);
  assert.deepEqual(legacy.map((row) => row.ms), [2, 443, 67, 8]);
  assert.match(legacy[1].note, /unavailable/);
  const core = timingRows({ ...result, timings: { ...timing, command: undefined } }, client);
  assert.equal(core.find((row) => row.label === 'Other request work')!.ms, 67);
  assert.ok(!core.some((row) => row.label === 'Save history'));
});

test('IPC residual is marked estimated and never negative; durations preserve fractional milliseconds', () => {
  const rows = timingRows(result, { ...client, invokeMs: 492.95 });
  const bridge = rows.find((row) => row.label === 'Response & IPC')!;
  assert.equal(bridge.ms, 0);
  assert.ok(bridge.estimated);
  assert.equal(formatTiming(0.07), '0.1 ms');
  assert.equal(formatTiming(443.26), '443.3 ms');
  for (const ms of [-1, NaN, Infinity]) assert.equal(formatTiming(ms), '0.0 ms');
});

test('combined preparation explains the eliminated round trip and places metadata after fetching', () => {
  const rows = timingRows({ ...result, timings: { ...timing, prepareMode: 'combined', prepareMs: 0 } }, client);
  assert.equal(rows.find((row) => row.label === 'Prepare SQL')!.ms, 0);
  assert.match(rows.find((row) => row.label === 'Prepare SQL')!.note, /share one database request/);
  assert.match(rows.find((row) => row.label === 'Execute & fetch')!.note, /Includes SQL preparation/);
  assert.ok(rows.findIndex((row) => row.label === 'Execute & fetch') < rows.findIndex((row) => row.label === 'Result metadata'));
});

test('empty results describe their headers without claiming the query was executed twice', () => {
  const rows = timingRows({ ...result, timings: { ...timing, prepareMode: 'combined', prepareMs: 10 } }, client);
  assert.equal(rows.find((row) => row.label === 'Describe empty result')!.ms, 10);
  assert.match(rows.find((row) => row.label === 'Describe empty result')!.note, /not executed again/);
});
