import assert from 'node:assert/strict';
import test from 'node:test';
import { setTimeout as wait } from 'node:timers/promises';
import { createHistorySearch, formatBytes, groupHistory, historyEntryBytes, transactionLabel, transactionStatus } from '../src/lib/history.ts';
import type { HistorySearchState } from '../src/lib/history.ts';
import type { HistoryEntry, HistoryPage } from '../src/lib/types.ts';
import { pickerMatches } from '../src/lib/workspacePicker.ts';

const entry: HistoryEntry = { id: 17, query: 'select 1', durationMs: 1, hasResult: true, isError: false, executedAt: '2026-01-01', queryHash: 'sql-hash', resultHash: 'response-hash', querySize: 8, resultSize: 300, executionSize: 100, queryReferences: 2, resultReferences: 2 };

test('transactions group across interleaved entries/pages without hiding standalone statements', () => {
  const transaction = { id: 'tx-1', kind: 'manual', status: 'active', statementCount: 70 } as const;
  const entries = [{ ...entry, id: 100, transaction }, { ...entry, id: 99 }, { ...entry, id: 98, transaction }, { ...entry, id: 98, transaction }];
  const groups = groupHistory(entries);
  assert.equal(groups.length, 2);
  assert.deepEqual(groups[0].entries.map(entry => entry.id), [100, 98]);
  assert.equal(groups[0].transaction?.statementCount, 70);
  assert.equal(groups[1].entries[0].id, 99);
  assert.equal(groups[1].transaction, null);
  assert.equal(transactionLabel(transaction), 'Transaction');
  assert.equal(transactionLabel({ ...transaction, kind: 'review' }), 'Reviewed edits');
  assert.equal(transactionLabel({ ...transaction, kind: 'batch' }), 'SQL batch');
  assert.equal(transactionStatus(transaction), 'Active');
  assert.equal(transactionStatus({ ...transaction, status: 'finished' }), 'Finished');
  assert.match(transactionStatus({ ...transaction, status: 'interrupted' }), /unconfirmed/);
});

test('history sizes distinguish bytes, KB, MB and GB without invalid values', () => {
  for (const [bytes, label] of [[0, '0 B'], [999, '999 B'], [1000, '1.0 KB'], [1_234_567, '1.2 MB'], [2_500_000_000, '2.5 GB'], [-1, '0 B'], [Infinity, '0 B']] as const) assert.equal(formatBytes(bytes), label);
  assert.equal(historyEntryBytes(entry), 408);
  assert.equal(historyEntryBytes({ ...entry, resultSize: null, hasResult: false }), 108);
});

test('large schemas cannot crowd history or commands out of global search', () => {
  const tables = Array.from({ length: 300 }, (_, i) => ({ id: `table:${i}`, label: `table_${i}`, detail: '', group: 'Tables & views' }));
  const history = { id: 'history:1', label: 'select old SQL', detail: 'Snapshot', group: 'History', matched: true };
  const command = { id: 'command:new', label: 'New query', detail: '', group: 'Commands' };
  const matches = pickerMatches([...tables, history, command], '');
  assert.equal(matches.filter(item => item.group === 'Tables & views').length, 20);
  assert.ok(matches.includes(history));
  assert.ok(matches.includes(command));
  // The server also searches errors, which need not appear in the SQL label.
  assert.deepEqual(pickerMatches([history, command], 'missing_relation'), [history]);
});

test('global search finds closed tabs by their full SQL and keeps all groups discoverable', () => {
  const archived = { id: 'closed-tab:1', label: 'Monthly report', detail: 'Reopen · Docker', group: 'Closed tabs', searchText: `select ${'x'.repeat(400)} from archived_report_table` };
  assert.deepEqual(pickerMatches([archived], 'archived_report_table'), [archived]);
  assert.deepEqual(pickerMatches([archived], 'monthly'), [archived]);
  assert.deepEqual(pickerMatches([archived], 'docker'), [archived]);
  assert.deepEqual(pickerMatches([archived], 'unrelated'), []);
  const items = ['Open tabs', 'Closed tabs', 'Tables & views', 'History', 'Commands'].flatMap(group => Array.from({ length: 30 }, (_, i) => ({ id: `${group}:${i}`, label: `${group} ${i}`, detail: '', group })));
  const matches = pickerMatches(items, '');
  for (const group of new Set(items.map(item => item.group))) assert.equal(matches.filter(item => item.group === group).length, 20);
});

test('history search debounces input and ignores responses after a newer request or closing', async () => {
  const requests: Array<{ term: string; resolve: (page: HistoryPage) => void }> = [];
  let state: HistorySearchState = { entries: [], total: 0, loading: false, error: '' };
  const search = createHistorySearch(term => new Promise(resolve => requests.push({ term, resolve })), next => state = next, String, 0);
  search.search('discarded');
  search.search('old');
  await wait(5);
  assert.deepEqual(requests.map(item => item.term), ['old']);
  search.search(' newest ');
  assert.equal(state.loading, true);
  assert.deepEqual(state.entries, []);
  await wait(5);
  requests[1].resolve({ entries: [entry], total: 100 });
  await wait(0);
  assert.equal(state.total, 100);
  assert.equal(requests[1].term, 'newest');
  requests[0].resolve({ entries: [], total: 0 });
  await wait(0);
  assert.equal(state.total, 100);
  search.search('closed');
  await wait(5);
  search.cancel();
  const closedState = state;
  requests[2].resolve({ entries: [entry], total: 200 });
  await wait(0);
  assert.equal(state, closedState);
});

test('history search exposes failures instead of silently claiming no matches', async () => {
  let state: HistorySearchState | undefined;
  const search = createHistorySearch(async () => { throw new Error('Storage unavailable'); }, next => state = next, error => (error as Error).message, 0);
  search.search('select');
  await wait(5);
  assert.equal(state?.error, 'Storage unavailable');
  assert.equal(state?.loading, false);
  search.cancel();
});
