import assert from 'node:assert/strict';
import test from 'node:test';
import { reorderTabs, tabDropTarget, tabScrollDelta } from '../src/lib/tabDrag.ts';
import { createQueryTab, restoreWorkspace, serializeWorkspace } from '../src/lib/workspace.ts';

const tabs = ['a', 'b', 'c', 'd'].map((id) => createQueryTab(id, id, `select '${id}'`, 'zenbase'));
const ids = (items: typeof tabs) => items.map((tab) => tab.id);

test('dragging tabs supports both directions, the beginning and the end', () => {
  assert.deepEqual(ids(reorderTabs(tabs, 'a', { id: 'c', side: 'after' })), ['b', 'c', 'a', 'd']);
  assert.deepEqual(ids(reorderTabs(tabs, 'd', { id: 'a', side: 'before' })), ['d', 'a', 'b', 'c']);
  assert.deepEqual(ids(reorderTabs(tabs, 'b', { id: 'd', side: 'after' })), ['a', 'c', 'd', 'b']);
  assert.deepEqual(ids(reorderTabs(tabs, 'c', { id: 'b', side: 'before' })), ['a', 'c', 'b', 'd']);
});

test('cancelled, stale, same-tab and unchanged-position drops do not reorder', () => {
  for (const drop of [null, { id: 'gone', side: 'before' as const }, { id: 'b', side: 'before' as const }, { id: 'a', side: 'after' as const }]) {
    assert.equal(reorderTabs(tabs, 'b', drop), tabs);
  }
  assert.equal(reorderTabs(tabs, 'gone', { id: 'a', side: 'before' }), tabs);
  assert.equal(reorderTabs(tabs, 'b', { id: 'c', side: 'before' }), tabs);
});

test('reordering preserves the exact tab objects, staged changes, active ID and persisted order', () => {
  const source = createQueryTab('source', 'Source', 'select 1', 'zenbase');
  source.pendingChanges = [{ rowIndex: 0, column: 'name', oldValue: 'old', newValue: 'new' }];
  const reordered = reorderTabs([source, ...tabs], 'source', { id: 'd', side: 'after' });
  assert.equal(reordered.at(-1), source);
  assert.equal(reordered[0], tabs[0]);
  assert.equal(source.pendingChanges.length, 1);
  const restored = restoreWorkspace(serializeWorkspace(reordered, 'source'))!;
  assert.equal(restored.activeId, 'source');
  assert.deepEqual(restored.tabs.map((tab) => tab.id), ['a', 'b', 'c', 'd', 'source']);
});

test('drop indicators follow midpoint insertion boundaries including gaps and strip ends', () => {
  const bounds = [{ id: 'a', left: 0, right: 100 }, { id: 'c', left: 110, right: 210 }];
  assert.deepEqual(tabDropTarget(bounds, -20), { id: 'a', side: 'before' });
  assert.deepEqual(tabDropTarget(bounds, 49), { id: 'a', side: 'before' });
  assert.deepEqual(tabDropTarget(bounds, 105), { id: 'c', side: 'before' });
  assert.deepEqual(tabDropTarget(bounds, 161), { id: 'c', side: 'after' });
  assert.deepEqual(tabDropTarget(bounds, 300), { id: 'c', side: 'after' });
  assert.equal(tabDropTarget([], 0), null);
  assert.equal(tabDropTarget(bounds, NaN), null);
});

test('overflowing tab strips auto-scroll only at the edges with bounded speed', () => {
  assert.equal(tabScrollDelta(100, 600, 300), 0);
  assert.equal(tabScrollDelta(100, 600, 100), -12);
  assert.equal(tabScrollDelta(100, 600, 600), 12);
  assert.equal(tabScrollDelta(100, 600, 90), -12);
  assert.equal(tabScrollDelta(100, 600, 620), 12);
  assert.ok(tabScrollDelta(100, 600, 130) < 0);
  assert.ok(tabScrollDelta(100, 600, 580) > 0);
  assert.equal(tabScrollDelta(100, 100, 100), 0);
  assert.equal(tabScrollDelta(100, 600, NaN), 0);
});
