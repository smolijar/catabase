import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';
import type { HistoryEntry } from '../src/lib/types.ts';

let server: ViteDevServer;
let renderCard: (entry: Partial<HistoryEntry>, grouped?: boolean) => string;

before(async () => {
  server = await createServer({
    root: fileURLToPath(new URL('..', import.meta.url)),
    server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: 'custom'
  });
  const { default: Card } = await server.ssrLoadModule('/src/components/HistoryEntryCard.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  renderCard = (entry, grouped = false) => render(Card, { props: {
    entry: { id: 1, query: 'select 1', durationMs: 1, executedAt: '2026-10-27', querySize: 8, executionSize: 10, ...entry },
    grouped, connections: [], onCopy() {}, onOpen() {}, onDelete() {}
  } }).body;
});

after(async () => { await server?.close(); });

test('failed entries allow Open without a row snapshot, standalone and in transaction groups', () => {
  for (const grouped of [false, true]) {
    for (const errorMessage of ['syntax error at or near "UPDATE"', null]) {
      const html = renderCard({ isError: true, hasResult: false, errorMessage }, grouped);
      assert.match(html, /<button(?![^>]*disabled)[^>]*aria-label="Open"/);
      assert.match(html, /aria-label="Error"/);
    }
  }
});

test('Open is available for successful snapshots and SQL-only executions', () => {
  for (const hasResult of [true, false]) {
    assert.match(renderCard({ isError: false, hasResult }), /<button(?![^>]*disabled)[^>]*aria-label="Open"/);
  }
});

test('compact entries expose only copy-preview, open and delete buttons with accessible labels', () => {
  for (const grouped of [false, true]) {
    const html = renderCard({ query: 'select 1;\nselect 2;', rowCount: 2, queryReferences: 3 }, grouped);
    assert.equal((html.match(/<button\b/g) ?? []).length, 3);
    assert.match(html, /aria-label="Copy SQL"/);
    assert.match(html, /title="Copy SQL\nselect 1;\nselect 2;"/);
    assert.match(html, /aria-label="Open"/);
    assert.match(html, /aria-label="Delete history entry"/);
    assert.doesNotMatch(html, /New query|Open result|history-storage-link/);
    assert.match(html, /2 rows/);
    assert.match(html, /shared by 3 executions/);
    assert.match(html, /Logical size, not exclusive disk usage/);
  }
});

test('only standalone entries repeat connection names; errors and SQL remain available in tooltips', () => {
  const entry = { errorMessage: 'the saved error', query: 'select the_query' };
  assert.match(renderCard(entry), /class="history-connection"/);
  assert.doesNotMatch(renderCard(entry, true), /class="history-connection"/);
  assert.match(renderCard(entry), /title="the saved error"/);
});

test('saved error messages are text, never executable markup', () => {
  const html = renderCard({ isError: true, errorMessage: '<script>bad()</script>' });
  assert.doesNotMatch(html, /<script>/);
  assert.match(html, /&lt;script>/);
});
