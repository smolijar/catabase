import assert from 'node:assert/strict';
import { test } from 'node:test';
import { get } from 'svelte/store';

test('clearing the log persists across reloads and finished operations stay cleared', async (context) => {
  const saved = new Map<string, string>([['catabase.activity', JSON.stringify([{
    id: 'older', category: 'clipboard', title: 'Copy SQL', status: 'running', startedAt: '2026-10-29T10:00:00Z'
  }])]]);
  const originalStorage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: { getItem: (key: string) => saved.get(key) ?? null, setItem: (key: string, value: string) => saved.set(key, value) }
  });
  context.after(() => {
    if (originalStorage) Object.defineProperty(globalThis, 'localStorage', originalStorage);
    else Reflect.deleteProperty(globalThis, 'localStorage');
  });

  const { activityLog, clearActivityLog, startActivity } = await import('../src/lib/activity.ts');
  assert.equal(get(activityLog).clearedAt, null);
  assert.equal(get(activityLog).events[0].status, 'interrupted');

  const finishOld = startActivity('schema', 'Load schema');
  const beforeClear = Date.now();
  clearActivityLog();
  const cleared = get(activityLog);
  assert.deepEqual(cleared.events, []);
  assert.ok(cleared.clearedAt !== null && cleared.clearedAt >= beforeClear);

  finishOld('completed');
  assert.deepEqual(get(activityLog), cleared);

  const finishNew = startActivity('connection', 'Check connection', 'Bookshop');
  finishNew('completed', 'Connection healthy');
  const reopened = await import(new URL('../src/lib/activity.ts?reopened', import.meta.url).href) as typeof import('../src/lib/activity.ts');
  assert.deepEqual(get(reopened.activityLog), get(activityLog));
  assert.equal(get(reopened.activityLog).clearedAt, cleared.clearedAt);
  assert.deepEqual(get(reopened.activityLog).events.map(event => event.title), ['Check connection']);
});
