import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';
import type { ConnectionTestState } from '../src/lib/connectionTest.ts';

let server: ViteDevServer;
let renderPill: (state: ConnectionTestState | null) => string;

before(async () => {
  server = await createServer({
    root: fileURLToPath(new URL('..', import.meta.url)),
    server: { middlewareMode: true, watch: null, hmr: false, ws: false },
    appType: 'custom'
  });
  const { default: Pill } = await server.ssrLoadModule('/src/components/ConnectionPill.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  renderPill = (state) => render(Pill, { props: {
    connection: { id: 'zenbase', name: 'zenbase docker' },
    state, onTest: () => {}, onChoose: () => {}
  } }).body;
});

after(async () => { await server?.close(); });

test('a failed connection keeps its name, shows the error in a red pill and remains retryable', () => {
  const html = renderPill({ status: 'error', message: 'Connection refused' });
  assert.match(html, /connection-chip[^\"]*failed/);
  assert.match(html, /zenbase docker/);
  assert.match(html, /role="status">Connection refused/);
  assert.match(html, /Click to test again/);
  assert.doesNotMatch(html, / disabled/);
  assert.doesNotMatch(html, /connection-chip[^\"]*connected/);
});

test('testing displays progress and disables duplicate clicks', () => {
  const html = renderPill({ status: 'testing' });
  assert.match(html, /connection-chip[^\"]*testing/);
  assert.match(html, /aria-busy="true"/);
  assert.match(html, / disabled/);
  assert.match(html, /role="status">Testing…/);
});

test('only successful tests show green; selecting an untested connection is neutral', () => {
  assert.match(renderPill({ status: 'success' }), /connection-chip[^\"]*connected/);
  const untested = renderPill(null);
  assert.doesNotMatch(untested, /connection-chip[^\"]*connected/);
  assert.match(untested, /Click to check the connection/);
});

test('database errors are escaped, not treated as markup or instructions', () => {
  const html = renderPill({ status: 'error', message: '<script>alert("bad")</script>' });
  assert.doesNotMatch(html, /<script>/);
  assert.match(html, /&lt;script>/);
});
