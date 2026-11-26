import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';

let server: ViteDevServer;
let renderDialog: () => string;
let renderBatch: (statements: string[]) => string;

before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('..', import.meta.url)), server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: 'custom' });
  const { default: Dialog } = await server.ssrLoadModule('/src/components/ResumeConnectionDialog.svelte');
  const { default: Batch } = await server.ssrLoadModule('/src/components/RunBatchDialog.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  const connection = { id: 'production', name: 'Production <admin>', username: 'postgres', password: 'must-not-render-secret', database: 'production_db', host: 'prod.example.invalid', port: 5432, ssl: true };
  const appearance = { color: 'red', environment: 'PROD ROOT' };
  renderDialog = () => render(Dialog, { props: {
    connections: [connection], initialConnectionId: 'production', appearances: { production: appearance },
    statements: ['select 42;'], themeKey: 'tuxedo', onConfirm: () => {}, onClose: () => {}
  } }).body;
  renderBatch = statements => render(Batch, { props: {
    connection, appearance, statements, themeKey: 'tuxedo', onConfirm: () => {}, onClose: () => {}
  } }).body;
});

test('batch review shows each of the first five statements separately and counts the rest', () => {
  const statements = Array.from({ length: 7 }, (_, index) => `SELECT ${101 + index};`);
  const html = renderBatch(statements);
  assert.match(html, /Run 7 statements/);
  assert.match(html, /and 2 more/);
  assert.equal((html.match(/aria-label="SQL preview"/g) ?? []).length, 5);
  for (const sql of statements.slice(0, 5)) assert.ok(html.includes(sql), sql);
  assert.doesNotMatch(html, /SELECT 10[67];/);
});
after(async () => { await server?.close(); });

test('connection review exposes environment, color and exact target without credentials', () => {
  const html = renderDialog();
  assert.match(html, /data-conn="red"/);
  assert.match(html, /red connection color/);
  for (const text of ['PROD ROOT', 'postgres', 'production_db', 'prod.example.invalid:5432', 'Required', 'select 42;', 'Connect &amp; run']) assert.ok(html.includes(text), text);
  assert.match(html, /Production &lt;admin(?:>|&gt;)/);
  assert.doesNotMatch(html, /<admin>/);
  assert.doesNotMatch(html, /must-not-render-secret/);
  assert.ok(html.indexOf('>Cancel</button>') < html.indexOf('>Connect &amp; run</button>'));
});
