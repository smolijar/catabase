import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';

let server: ViteDevServer;
let renderDialog: (props?: Record<string, unknown>) => string;
const details = {
  connectionName: 'zenbase docker', executedAt: '2026-10-14T09:00:00Z', batch: false,
  result: { rowCount: 199, durationMs: 443, timings: { prepareMs: 10, metadataMs: 1, executeFetchMs: 420, decodeMs: 12, totalMs: 443, metadataSource: 'cache', metadataQueries: 0, command: { setupMs: 20, historyMs: 30, totalMs: 493 } } },
  client: { targetMs: 2, invokeMs: 510, uiMs: 8, totalMs: 520 }
};

before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('..', import.meta.url)), server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: 'custom' });
  const { default: Dialog } = await server.ssrLoadModule('/src/components/QueryTimingDialog.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  renderDialog = (props = {}) => render(Dialog, { props: { details, onClose: () => {}, ...props } }).body;
});
after(async () => { await server?.close(); });

test('timing dialog is labelled, closable, and distinguishes status duration from end-to-end time', () => {
  const html = renderDialog();
  assert.match(html, /<dialog[^>]*aria-label="Query timing"/);
  assert.match(html, /<button[^>]*>Close<\/button>/);
  assert.match(html, />443 ms<\/strong>/);
  assert.match(html, />520.0 ms<\/strong>/);
  assert.match(html, /cache hit · 0 metadata queries/);
  assert.match(html, /≈ 17.0 ms/);
  assert.match(html, /Browser paint is not included/);
});

test('paging timing is explicitly for the latest batch and connection names are escaped', () => {
  const html = renderDialog({ details: { ...details, connectionName: '<script>not markup</script>', batch: true, result: { ...details.result, rowCount: 1 } } });
  assert.match(html, /1 row in this batch/);
  assert.match(html, /Latest batch query time/);
  assert.match(html, /&lt;script/);
  assert.doesNotMatch(html, /<script>/);
});
