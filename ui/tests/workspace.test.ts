import assert from 'node:assert/strict';
import test from 'node:test';
import { archiveTab, canUseResult, createDraftWriter, createHistoryTab, createHistoryTransactionTab, createQueryTab, createResultTab, duplicateQueryTab, placeResults, reopenArchivedTab, restoreWorkspace, serializeWorkspace } from '../src/lib/workspace.ts';
import type { HistoryEntry, QueryResult } from '../src/lib/types.ts';

test('connection locks survive save, duplicate, archive and reopen', () => {
  const tab = { ...createQueryTab('locked', 'Production', 'select 1', 'prod'), connectionLocked: true };
  assert.equal(duplicateQueryTab(tab, 'copy', [tab]).connectionLocked, true);
  const archived = archiveTab(tab, '2026-11-19T10:00:00Z')!;
  const restored = restoreWorkspace(serializeWorkspace([tab], tab.id, [ { ...archived, id: 'closed' } ]))!;
  assert.equal(restored.tabs[0].connectionLocked, true);
  assert.equal(reopenArchivedTab(restored.archivedTabs[0]).connectionLocked, true);
});

test('legacy tabs default to unlocked and a lock without an association is rejected', () => {
  const legacy = { id: 'old', title: 'Old', sql: '', connectionId: 'db' };
  const invalid = { ...legacy, id: 'invalid', connectionId: null, connectionLocked: true };
  const missing = { ...legacy, id: 'deleted', connectionId: 'deleted-db', connectionLocked: true };
  const restored = restoreWorkspace(JSON.stringify({ version: 2, tabs: [legacy, invalid, missing] }))!;
  assert.deepEqual(restored.tabs.map(tab => tab.connectionLocked), [false, false, true]);
});

const failedHistory: HistoryEntry = {
  id: 42, connectionId: 'docker', query: 'select * from missing_relation', durationMs: 12,
  isError: true, errorMessage: 'relation "missing_relation" does not exist', hasResult: false,
  executedAt: '2026-10-27T10:00:00Z', queryHash: 'query-hash', querySize: 30,
  executionSize: 100, queryReferences: 1, resultReferences: 0
};

function liveResult(id: string, sql = 'select 1;') {
  return createResultTab(id, {
    execution: { sql, start: 0, end: sql.length }, connectionId: 'db-one', connectionName: 'One', executedAt: '', snapshot: false
  }, { rows: [{ private: 'secret' }], columns: [], rowCount: 1, durationMs: 1, autoLimited: false, hasMore: false });
}

test('failed history opens exact SQL and its saved error in a read-only tab without fake result rows', () => {
  const tab = createHistoryTab(failedHistory, null, 'zenbase docker');
  const [result] = tab.results;
  assert.equal(tab.kind, 'reference');
  assert.equal(tab.sql, failedHistory.query);
  assert.equal(tab.connectionId, 'docker');
  assert.equal(tab.activeResultId, result.id);
  assert.deepEqual(result.error, { message: failedHistory.errorMessage });
  assert.equal(result.data, null);
  assert.deepEqual(result.context, {
    execution: { sql: failedHistory.query, start: 0, end: failedHistory.query.length },
    connectionId: 'docker', connectionName: 'zenbase docker',
    executedAt: failedHistory.executedAt, historyEntryIds: [failedHistory.id], durationMs: 12, snapshot: true
  });
  assert.deepEqual(result.pendingChanges, []);
  assert.equal(result.editMode, false);
});

test('old failures with missing details or deleted connections still open with an honest fallback', () => {
  const tab = createHistoryTab({ ...failedHistory, errorMessage: null, connectionId: null }, null, 'Unavailable connection');
  assert.match(tab.results[0].error!.message, /failed, but no error message was saved/);
  assert.equal(tab.connectionId, null);
  assert.equal(tab.results[0].context.connectionName, 'Unavailable connection');
});

test('successful history still opens row snapshots; stale rows never masquerade as a failed result', () => {
  const result: QueryResult = { columns: [], rows: [{ answer: 42 }], rowCount: 1, durationMs: 3, autoLimited: false, hasMore: false };
  const [success] = createHistoryTab({ ...failedHistory, isError: false, errorMessage: null, hasResult: true }, result, 'Docker').results;
  assert.equal(success.data, result);
  assert.equal(success.error, null);
  assert.equal(success.context.snapshot, true);
  assert.equal(canUseResult({ ...success.context, data: result }, 'docker'), false);
  assert.equal(createHistoryTab(failedHistory, result, 'Docker').results[0].data, null);
});

test('grouped history opens each statement in order with the saved SQL, failure, or snapshot identity', () => {
  const entries = [
    { ...failedHistory, id: 40, query: 'BEGIN', isError: false, hasResult: false },
    { ...failedHistory, id: 41, query: 'SELECT loaded\nLIMIT 200', isError: false, hasResult: true },
    failedHistory
  ];
  const tab = createHistoryTransactionTab('batch-2026', 'SQL batch', entries, 'Docker');
  assert.equal(tab.id, 'history-transaction:batch-2026');
  assert.equal(tab.kind, 'reference');
  assert.equal(tab.activeResultId, 'history:40');
  assert.equal(tab.sql, entries.map(entry => entry.query).join('\n\n'));
  assert.deepEqual(tab.results.map(result => result.context.historyEntryIds), [[40], [41], [42]]);
  for (const result of tab.results) {
    const { start, end, sql } = result.context.execution;
    assert.equal(tab.sql.slice(start, end), sql);
    assert.equal(result.context.snapshot, true);
    assert.equal(result.data, null);
  }
  assert.equal(tab.results[0].savedSnapshot, undefined);
  assert.deepEqual(tab.results[1].savedSnapshot, { entryId: 41, state: 'unloaded' });
  assert.deepEqual(tab.results[2].error, { message: failedHistory.errorMessage });
  assert.equal(tab.results[2].savedSnapshot, undefined);
});

test('history previews survive tab capture but never contaminate fresh query drafts or saved workspace', () => {
  const preview = createHistoryTab(failedHistory, null, 'Docker');
  const captured = { ...preview, viewState: null };
  assert.deepEqual(captured.results, preview.results);
  const copy = duplicateQueryTab(preview, 'new', [preview]);
  assert.equal(copy.kind, 'query');
  assert.equal(copy.sql, failedHistory.query);
  assert.deepEqual(copy.results, []);
  assert.equal(copy.activeResultId, null);
  const stored = serializeWorkspace([copy, preview], preview.id);
  assert.equal(stored.includes('activeResultId'), false);
  assert.equal(stored.includes('does not exist'), false);
  assert.equal(restoreWorkspace(stored)?.tabs.length, 1);
});

test('duplicating copies the draft and connection, never query results or staged edits', () => {
  const source = createQueryTab('one', 'Orders', 'select * from orders;', 'db-one');
  const result = liveResult('result', source.sql);
  result.editMode = true;
  result.pendingChanges = [{ rowIndex: 0, column: 'private', oldValue: 'secret', newValue: 'changed' }];
  source.results = [result, { ...liveResult('failed'), data: null, error: { message: 'Old error' } }];
  source.activeResultId = result.id;
  const copy = duplicateQueryTab(source, 'two', [source]);
  assert.deepEqual(copy, createQueryTab('two', 'Orders copy', source.sql, 'db-one'));
  assert.equal(source.results[0].pendingChanges.length, 1);
  assert.equal(source.results[0].data?.rows[0].private, 'secret');
});

test('a run replaces all previous results and selects the first result of the new batch', () => {
  const [a, b, c, first, second] = ['a', 'b', 'c', 'first', 'second'].map(id => liveResult(id));
  const tab = { ...createQueryTab('editor', 'Query'), results: [a, b, c], activeResultId: b.id };
  const update = placeResults(tab, [first, second], false);
  assert.deepEqual(update.results, [first, second]);
  assert.equal(update.activeResultId, first.id);
  const failed = { ...second, error: {message: 'Invalid SQL'}, data: null };
  assert.equal(placeResults(tab, [first, failed], false).activeResultId, failed.id);
});

test('an append run preserves existing results and replaces only its pending placeholder', () => {
  const [a, b, pending, first, second] = ['a', 'b', 'pending', 'first', 'second'].map(id => liveResult(id));
  const tab = { ...createQueryTab('editor', 'Query'), results: [a, b], activeResultId: b.id };
  const appended = { ...tab, ...placeResults(tab, [pending], true) };
  assert.deepEqual(appended.results, [a, b, pending]);
  assert.equal(appended.activeResultId, pending.id);
  const completed = placeResults(appended, [first, second], true, pending.id);
  assert.deepEqual(completed.results, [a, b, first, second]);
  assert.equal(completed.activeResultId, first.id);
  const switched = placeResults({ ...appended, activeResultId: a.id }, [first, second], true, pending.id);
  assert.deepEqual(switched.results, [a, b, first, second]);
  assert.equal(switched.activeResultId, a.id);
});

test('duplicate names are unique and references become fresh editable query tabs', () => {
  const source = { ...createQueryTab('one', 'Orders', 'select 1;'), kind: 'reference' as const };
  const copies = [source, createQueryTab('two', 'Orders copy'), createQueryTab('three', 'Orders copy 2')];
  assert.equal(duplicateQueryTab(source, 'four', copies).title, 'Orders copy 3');
  assert.equal(duplicateQueryTab(source, 'four', copies).kind, 'query');
  assert.equal(duplicateQueryTab(source, 'four', copies).connectionId, null);
});

test('duplicate editor positions are independent from the original tab', () => {
  const source = createQueryTab('one', 'Orders');
  source.viewState = { cursorState: [], viewState: { scrollTop: 20 } } as unknown as NonNullable<typeof source.viewState>;
  const copy = duplicateQueryTab(source, 'two', [source]);
  assert.deepEqual(copy.viewState, source.viewState);
  assert.notEqual(copy.viewState, source.viewState);
  assert.notEqual(copy.viewState?.viewState, source.viewState.viewState);
});

test('restored tabs retain their SQL and connection, including an intentionally empty draft', () => {
  const first = createQueryTab('one', 'Orders', 'select 1;', 'db-one');
  const second = createQueryTab('two', 'Blank', '', 'db-two');
  const restored = restoreWorkspace(serializeWorkspace([first, second], 'two'))!;
  assert.equal(restored.activeId, 'two');
  assert.deepEqual(restored.tabs.map(({ sql, connectionId }) => ({ sql, connectionId })), [{ sql: 'select 1;', connectionId: 'db-one' }, { sql: '', connectionId: 'db-two' }]);
});

test('result rows and pending edits are not persisted; reference tabs do not replace query drafts', () => {
  const tab = createQueryTab('one', 'Query', 'select 1;', 'db');
  const result = liveResult('result');
  result.pendingChanges = [{ rowIndex: 0, column: 'private', oldValue: 'secret', newValue: 'changed' }];
  tab.results = [result];
  tab.activeResultId = result.id;
  const reference = { ...createQueryTab('history', 'History', 'select 2;'), kind: 'reference' as const };
  const stored = serializeWorkspace([tab, reference], 'history');
  assert.ok(!stored.includes('secret'));
  const restored = restoreWorkspace(stored)!;
  assert.equal(restored.activeId, 'one');
  assert.equal(restored.tabs.length, 1);
  assert.deepEqual(restored.tabs[0].results, []);
  assert.equal(restored.tabs[0].activeResultId, null);
});

test('historical results and results from another connection cannot be extended or edited', () => {
  const session = { data: { rows: [], columns: [], rowCount: 0, durationMs: 0, autoLimited: false, hasMore: false }, execution: { sql: 'select 1;', start: 0, end: 9 }, connectionId: 'one', connectionName: 'One', executedAt: '', snapshot: false };
  assert.equal(canUseResult(session, 'one'), true);
  assert.equal(canUseResult(session, 'two'), false);
  assert.equal(canUseResult({ ...session, snapshot: true }, 'one'), false);
  assert.equal(canUseResult(null, 'one'), false);
});

test('invalid storage and duplicate ids are handled without losing valid tabs', () => {
  assert.equal(restoreWorkspace('not json'), null);
  const restored = restoreWorkspace(JSON.stringify({ version: 1, tabs: [{ id: 'a', title: 'A', sql: '' }, { id: 'a', title: 'Duplicate', sql: 'bad' }, { id: 3 }] }))!;
  assert.equal(restored.tabs.length, 1);
  assert.equal(restored.tabs[0].title, 'A');
});

test('closing the last query while references remain restores a blank workspace', () => {
  const reference = { ...createQueryTab('history', 'History', 'select 2;', 'db'), kind: 'reference' as const };
  const restored = restoreWorkspace(serializeWorkspace([reference], 'history'))!;
  assert.ok(restored);
  assert.equal(restored.tabs.length, 1);
  assert.equal(restored.tabs[0].kind, 'query');
  assert.equal(restored.tabs[0].sql, '');
  assert.equal(restored.activeId, restored.tabs[0].id);
  assert.equal(restoreWorkspace(JSON.stringify({ version: 1, tabs: [{ invalid: true }] })), null);
});

test('workspace saves cannot overwrite a newer draft by completing out of order', async () => {
  const started: string[] = [];
  let finishFirst: () => void = () => {};
  let saved = '';
  const write = createDraftWriter(async (content) => {
    started.push(content);
    if (content === 'older') await new Promise<void>((resolve) => { finishFirst = resolve; });
    saved = content;
  });
  const first = write('older');
  const latest = write('latest');
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(started, ['older']);
  finishFirst();
  await Promise.all([first, latest]);
  assert.deepEqual(started, ['older', 'latest']);
  assert.equal(saved, 'latest');
});

test('closing can retry a draft after an earlier autosave failure', async () => {
  let calls = 0;
  let saved = '';
  const write = createDraftWriter(async (content) => {
    calls += 1;
    if (calls === 1) throw new Error('Temporary storage failure');
    saved = content;
  });
  await assert.rejects(write('unsaved'), /Temporary storage failure/);
  await write('retry on close');
  assert.equal(saved, 'retry on close');
});

test('closed tabs persist exact SQL, names, connections and editor positions, never results or staged changes', () => {
  const source = createQueryTab('closed', 'Monthly report', 'select 1;\n\nselect 2;', 'docker');
  source.viewState = { cursorState: [], viewState: { scrollTop: 42 } } as unknown as NonNullable<typeof source.viewState>;
  const result = liveResult('result', source.sql);
  result.pendingChanges = [{ rowIndex: 0, column: 'private', oldValue: 'secret', newValue: 'private new' }];
  result.editMode = true;
  source.results = [result, { ...liveResult('failed'), data: null, error: { message: 'private error' } }];
  source.activeResultId = result.id;
  const archived = archiveTab(source, '2026-10-27T10:00:00Z')!;
  assert.notEqual(archived.viewState, source.viewState);
  const stored = serializeWorkspace([createQueryTab('open', 'Open')], 'open', [archived]);
  assert.doesNotMatch(stored, /private|secret|pendingChanges|editMode|originalRows|activeResultId/);
  const restored = restoreWorkspace(stored)!;
  assert.equal(restored.tabs.length, 1);
  assert.deepEqual(restored.archivedTabs, [archived]);
  const reopened = reopenArchivedTab(restored.archivedTabs[0]);
  assert.equal(reopened.id, source.id);
  assert.equal(reopened.sql, source.sql);
  assert.equal(reopened.title, source.title);
  assert.equal(reopened.connectionId, 'docker');
  assert.deepEqual(reopened.viewState, source.viewState);
  assert.notEqual(reopened.viewState, archived.viewState);
  assert.deepEqual(reopened.results, []);
  assert.equal(reopened.activeResultId, null);
});

test('empty queries are archived, references and history previews are not', () => {
  assert.equal(archiveTab(createQueryTab('blank', 'Blank'), '2026-10-27')?.sql, '');
  assert.equal(archiveTab(createHistoryTab(failedHistory, null, 'Docker'), '2026-10-27'), null);
  assert.equal(archiveTab({ ...createQueryTab('ddl', 'DDL'), kind: 'reference' }, '2026-10-27'), null);
});

test('archives migrate from v1, retain newest-first order and reject malformed/duplicate records', () => {
  const open = createQueryTab('one', 'Open');
  assert.deepEqual(restoreWorkspace(JSON.stringify({ version: 1, tabs: [open] }))?.archivedTabs, []);
  const old = archiveTab(createQueryTab('old', 'Old'), '2026-10-25')!;
  const newer = archiveTab(createQueryTab('new', 'New'), '2026-10-27')!;
  const restored = restoreWorkspace(JSON.stringify({ version: 2, tabs: [open], activeId: 'new', archivedTabs: [old, newer, { ...old, title: 'duplicate' }, { ...old, id: 'one' }, { ...old, id: 'bad', closedAt: 'invalid' }, { ...old, id: 'ref', kind: 'reference' }, null] }))!;
  assert.deepEqual(restored.archivedTabs, [newer, old]);
  assert.equal(restored.activeId, 'one');
});

test('closing the last draft retains its archive and creates a distinct blank tab after restart', () => {
  const archived = archiveTab(createQueryTab('playground', 'Saved', 'select 42'), '2026-10-27')!;
  const restored = restoreWorkspace(serializeWorkspace([], archived.id, [archived]))!;
  assert.equal(restored.tabs.length, 1);
  assert.equal(restored.tabs[0].sql, '');
  assert.notEqual(restored.activeId, archived.id);
  assert.deepEqual(restored.archivedTabs, [archived]);
});

test('a reopened tab is not also persisted as closed; repeated closes replace the same identity', () => {
  const first = archiveTab(createQueryTab('a', 'A', 'select 1'), '2026-10-26')!;
  const reopened = reopenArchivedTab(first);
  const stored = restoreWorkspace(serializeWorkspace([reopened], reopened.id, [first]))!;
  assert.deepEqual(stored.archivedTabs, []);
  reopened.sql = 'select 2';
  const again = archiveTab(reopened, '2026-10-27')!;
  assert.equal(again.id, first.id);
  assert.equal(again.sql, 'select 2');
});
