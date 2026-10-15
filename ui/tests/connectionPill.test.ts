import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';
import type { ConnectionTestState } from '../src/lib/connectionTest.ts';

let server: ViteDevServer;
let renderPill: (state: ConnectionTestState | null, compact?: boolean, now?: number) => string;

before(async () => {
  server = await createServer({
    root: fileURLToPath(new URL('..', import.meta.url)),
    server: { middlewareMode: true, watch: null, hmr: false, ws: false },
    appType: 'custom'
  });
  const { default: Pill } = await server.ssrLoadModule('/src/components/ConnectionPill.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  renderPill = (state, compact = false, now = 12345) => render(Pill, { props: {
    connection: { id: 'zenbase', name: 'zenbase docker' },
    state, compact, now, onTest: () => {}, onChoose: () => {}
  } }).body;
});

after(async () => { await server?.close(); });

test('a failed connection keeps its name, shows the error in a red pill and remains retryable', () => {
  const html = renderPill({ status: 'error', message: 'Connection refused', checkedAt: 12345 });
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
  assert.match(renderPill({ status: 'success', checkedAt: 12345 }), /connection-chip[^\"]*connected/);
  const untested = renderPill(null);
  assert.doesNotMatch(untested, /connection-chip[^\"]*connected/);
  assert.match(untested, /Click to check the connection/);
});

test('database errors are escaped, not treated as markup or instructions', () => {
  const html = renderPill({ status: 'error', message: '<script>alert("bad")</script>', checkedAt: 12345 });
  assert.doesNotMatch(html, /<script>/);
  assert.match(html, /&lt;script>/);
});

test('navbar and compact status-bar pills share stale, error and checking semantics', () => {
  for (const compact of [false, true]) {
    const stale = renderPill({ status: 'success', checkedAt: 1000 }, compact, 61000);
    assert.match(stale, /connection-chip[^\"]*stale/);
    assert.match(stale, /Check stale/);
    assert.match(stale, /over one minute old/);
    assert.doesNotMatch(stale, /connection-chip[^\"]*connected/);
    assert.doesNotMatch(stale, / disabled/);
    const failed = renderPill({ status: 'error', message: 'Connection refused', checkedAt: 12345 }, compact);
    assert.match(failed, /connection-chip[^\"]*failed/);
    assert.match(failed, /Connection refused/);
    assert.match(renderPill({ status: 'testing' }, compact), /aria-busy="true"/);
  }
});
