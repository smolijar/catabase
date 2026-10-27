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
    grouped, connections: [], onRestore() {}, onOpen() {}, onStorage() {}, onDelete() {}
  } }).body;
});

after(async () => { await server?.close(); });

test('failed entries allow Open result without a row snapshot, standalone and in transaction groups', () => {
  for (const grouped of [false, true]) {
    for (const errorMessage of ['syntax error at or near "UPDATE"', null]) {
      const html = renderCard({ isError: true, hasResult: false, errorMessage }, grouped);
      assert.match(html, /<button(?![^>]*disabled)[^>]*>Open result<\/button>/);
      assert.match(html, />Error<\/span>/);
    }
  }
});

test('successful row snapshots remain openable; successful SQL-only entries keep the existing disabled action', () => {
  assert.match(renderCard({ isError: false, hasResult: true }), /<button(?![^>]*disabled)[^>]*>Open result<\/button>/);
  assert.match(renderCard({ isError: false, hasResult: false }), /<button[^>]*disabled[^>]*>Open result<\/button>/);
});

test('saved error messages are text, never executable markup', () => {
  const html = renderCard({ isError: true, errorMessage: '<script>bad()</script>' });
  assert.doesNotMatch(html, /<script>/);
  assert.match(html, /&lt;script>/);
});
