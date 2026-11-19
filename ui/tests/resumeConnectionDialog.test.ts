import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';

let server: ViteDevServer;
let renderDialog: () => string;

before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('..', import.meta.url)), server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: 'custom' });
  const { default: Dialog } = await server.ssrLoadModule('/src/components/ResumeConnectionDialog.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  renderDialog = () => render(Dialog, { props: {
    connections: [{ id: 'production', name: 'Production <admin>', username: 'postgres', password: 'must-not-render-secret', database: 'production_db', host: 'prod.example.invalid', port: 5432, ssl: true }],
    initialConnectionId: 'production', appearances: { production: { color: 'red', environment: 'PROD ROOT' } },
    query: 'select 42;', onConfirm: () => {}, onClose: () => {}
  } }).body;
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
