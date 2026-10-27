import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';
import { archiveTab, createQueryTab } from '../src/lib/workspace.ts';

let server: ViteDevServer;
let renderPanel: (props?: Record<string, unknown>) => string;
const query = createQueryTab('open', 'Current query', 'select 1', 'docker');
const closed = archiveTab(createQueryTab('closed', 'Saved report', 'select * from archived_table', 'docker'), '2026-10-27T10:00:00Z')!;

before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('..', import.meta.url)), server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: 'custom' });
  const { default: Panel } = await server.ssrLoadModule('/src/components/TabsPanel.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  renderPanel = (props = {}) => render(Panel, { props: { tabs: [query], archivedTabs: [closed], activeId: 'open', connections: [{ id: 'docker', name: 'zenbase docker' }], onSelect() {}, onReopen() {}, onNew() {}, ...props } }).body;
});
after(async () => { await server?.close(); });

test('Tabs panel distinguishes open and closed queries with reopen actions and connection context', () => {
  const html = renderPanel();
  assert.match(html, /1 open · 1 closed/);
  assert.match(html, /aria-label="Open tabs"/);
  assert.match(html, /aria-label="Closed tabs"/);
  assert.match(html, /aria-current="page"/);
  assert.match(html, /aria-label="Reopen Saved report"/);
  assert.match(html, /select \* from archived_table/);
  assert.match(html, /zenbase docker/);
  assert.match(html, /aria-label="Search tabs"/);
});

test('read-only tabs are identified and empty archives explain how to recover closed queries', () => {
  const html = renderPanel({ tabs: [{ ...query, kind: 'reference' }], archivedTabs: [] });
  assert.match(html, /Read only/);
  assert.match(html, /Closed queries are saved here/);
  assert.doesNotMatch(html, /aria-label="Reopen /);
});

test('tab names and SQL remain escaped text in archive rows', () => {
  const html = renderPanel({ archivedTabs: [{ ...closed, title: '<script>name</script>', sql: '<script>sql</script>' }] });
  assert.doesNotMatch(html, /<script>/);
  assert.match(html, /&lt;script>/);
});
