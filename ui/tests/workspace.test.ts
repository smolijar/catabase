import assert from 'node:assert/strict';
import test from 'node:test';
import { archiveTab, canUseResult, createDraftWriter, createHistoryTab, createQueryTab, duplicateQueryTab, reopenArchivedTab, restoreWorkspace, serializeWorkspace } from '../src/lib/workspace.ts';
import type { HistoryEntry, QueryResult } from '../src/lib/types.ts';

const failedHistory: HistoryEntry = {
  id: 42, connectionId: 'docker', query: 'select * from missing_relation', durationMs: 12,
  isError: true, errorMessage: 'relation "missing_relation" does not exist', hasResult: false,
  executedAt: '2026-10-27T10:00:00Z', queryHash: 'query-hash', querySize: 30,
  executionSize: 100, queryReferences: 1, resultReferences: 0
};

test('failed history opens exact SQL and its saved error in a read-only tab without fake result rows', () => {
  const tab = createHistoryTab(failedHistory, null, 'zenbase docker');
  assert.equal(tab.kind, 'reference');
  assert.equal(tab.sql, failedHistory.query);
  assert.equal(tab.connectionId, 'docker');
  assert.deepEqual(tab.error, { message: failedHistory.errorMessage });
  assert.equal(tab.session, null);
  assert.equal(canUseResult(tab.session, tab.connectionId), false);
  assert.deepEqual(tab.historyContext, {
    execution: { sql: failedHistory.query, start: 0, end: failedHistory.query.length },
    connectionId: 'docker', connectionName: 'zenbase docker',
    executedAt: failedHistory.executedAt, durationMs: 12, snapshot: true
  });
  assert.deepEqual(tab.pendingChanges, []);
  assert.equal(tab.editMode, false);
});

test('old failures with missing details or deleted connections still open with an honest fallback', () => {
  const tab = createHistoryTab({ ...failedHistory, errorMessage: null, connectionId: null }, null, 'Unavailable connection');
  assert.match(tab.error!.message, /failed, but no error message was saved/);
  assert.equal(tab.connectionId, null);
  assert.equal(tab.historyContext?.connectionName, 'Unavailable connection');
});

test('successful history still opens row snapshots; stale rows never masquerade as a failed result', () => {
  const result: QueryResult = { columns: [], rows: [{ answer: 42 }], rowCount: 1, durationMs: 3, autoLimited: false, hasMore: false };
  const success = createHistoryTab({ ...failedHistory, isError: false, errorMessage: null, hasResult: true }, result, 'Docker');
  assert.equal(success.session?.data, result);
  assert.equal(success.error, null);
  assert.equal(success.session?.snapshot, true);
  assert.equal(canUseResult(success.session, 'docker'), false);
  assert.equal(createHistoryTab(failedHistory, result, 'Docker').session, null);
});

test('history previews survive tab capture but never contaminate fresh query drafts or saved workspace', () => {
  const preview = createHistoryTab(failedHistory, null, 'Docker');
  const captured = { ...preview, viewState: null };
  assert.deepEqual(captured.error, preview.error);
  assert.deepEqual(captured.historyContext, preview.historyContext);
  const copy = duplicateQueryTab(preview, 'new', [preview]);
  assert.equal(copy.kind, 'query');
  assert.equal(copy.sql, failedHistory.query);
  assert.equal(copy.error, null);
  assert.equal(copy.historyContext, undefined);
  const stored = serializeWorkspace([copy, preview], preview.id);
  assert.equal(stored.includes('historyContext'), false);
  assert.equal(stored.includes('does not exist'), false);
  assert.equal(restoreWorkspace(stored)?.tabs.length, 1);
});

test('duplicating copies the draft and connection, never query results or staged edits', () => {
  const source = createQueryTab('one', 'Orders', 'select * from orders;', 'db-one');
  source.editMode = true;
  source.error = { message: 'Old error', code: 'test', position: null };
  source.pendingChanges = [{ rowIndex: 0, column: 'private', oldValue: 'secret', newValue: 'changed' }];
  source.originalRows = [{ private: 'secret' }];
  source.session = { data: { rows: source.originalRows, columns: [], rowCount: 1, durationMs: 1, autoLimited: false, hasMore: false }, execution: { sql: source.sql, start: 0, end: source.sql.length }, connectionId: 'db-one', connectionName: 'One', executedAt: '', snapshot: false };
  const copy = duplicateQueryTab(source, 'two', [source]);
  assert.deepEqual(copy, createQueryTab('two', 'Orders copy', source.sql, 'db-one'));
  assert.equal(source.pendingChanges.length, 1);
  assert.equal(source.session.data.rows[0].private, 'secret');
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
  tab.pendingChanges = [{ rowIndex: 0, column: 'private', oldValue: 'secret', newValue: 'changed' }];
  const reference = { ...createQueryTab('history', 'History', 'select 2;'), kind: 'reference' as const };
  const stored = serializeWorkspace([tab, reference], 'history');
  assert.ok(!stored.includes('secret'));
  const restored = restoreWorkspace(stored)!;
  assert.equal(restored.activeId, 'one');
  assert.equal(restored.tabs.length, 1);
  assert.deepEqual(restored.tabs[0].pendingChanges, []);
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
  source.error = { message: 'private error' };
  source.originalRows = [{ secret: 'secret row' }];
  source.pendingChanges = [{ rowIndex: 0, column: 'secret', oldValue: 'private old', newValue: 'private new' }];
  source.editMode = true;
  const archived = archiveTab(source, '2026-10-27T10:00:00Z')!;
  assert.notEqual(archived.viewState, source.viewState);
  const stored = serializeWorkspace([createQueryTab('open', 'Open')], 'open', [archived]);
  assert.doesNotMatch(stored, /private|secret|pendingChanges|editMode|originalRows|historyContext/);
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
  assert.equal(reopened.session, null);
  assert.equal(reopened.error, null);
  assert.equal(reopened.editMode, false);
  assert.deepEqual(reopened.pendingChanges, []);
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
