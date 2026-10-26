import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';

let server: ViteDevServer;
let renderDialog: (props: Record<string, unknown>) => string;
before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('..', import.meta.url)), server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: 'custom' });
  const { default: Dialog } = await server.ssrLoadModule('/src/components/HistoryStorageDialog.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  renderDialog = props => render(Dialog, { props: { onClose() {}, ...props } }).body;
});
after(async () => { await server?.close(); });

test('history storage distinguishes real disk pages, shared payload savings and logical content', () => {
  const html = renderDialog({ stats: { allocatedBytes: 2_000_000, storedBytes: 1_000_000, savedBytes: 3_000_000, queryBytes: 2000, resultBytes: 997000, executionBytes: 1000, uniqueQueries: 4, uniqueResults: 2 } });
  for (const label of ['History on disk', '2.0 MB', 'Duplicate payload avoided', '3.0 MB', '4 distinct queries', '2 distinct responses', 'Total logical content', 'SQLite may retain freed pages']) assert.ok(html.includes(label), label);
});

test('entry sizes explain shared ownership and expose hashes without claiming exclusive disk bytes', () => {
  const html = renderDialog({ entry: { id: 1, querySize: 1000, resultSize: 2000, executionSize: 300, queryReferences: 3, resultReferences: 2, hasResult: true, queryHash: 'query-sha', resultHash: 'response-sha' } });
  for (const label of ['3.3 KB', 'Shared by 3 executions', 'Shared by 2 executions', '300 B', 'query-sha', 'response-sha', 'not exclusive disk usage']) assert.ok(html.includes(label), label);
});

test('missing snapshots and loading/errors are explicit and escaped', () => {
  assert.match(renderDialog({ entry: { id: 1, querySize: 3, executionSize: 100, queryReferences: 1, hasResult: false, queryHash: 'hash' } }), /No snapshot stored/);
  assert.match(renderDialog({}), /Calculating history storage/);
  const html = renderDialog({ error: '<script>bad</script>' });
  assert.match(html, /role="alert"/);
  assert.doesNotMatch(html, /<script>/);
});
