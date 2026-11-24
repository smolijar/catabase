import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';
import type { ConnectionTestState } from '../src/lib/connectionTest.ts';

let server: ViteDevServer;
let renderPill: (state: ConnectionTestState | null, compact?: boolean, now?: number, extras?: Record<string, unknown>) => string;

before(async () => {
  server = await createServer({
    root: fileURLToPath(new URL('..', import.meta.url)),
    server: { middlewareMode: true, watch: null, hmr: false, ws: false },
    appType: 'custom'
  });
  const { default: Pill } = await server.ssrLoadModule('/src/components/ConnectionPill.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  renderPill = (state, compact = false, now = 12345, extras = {}) => render(Pill, { props: {
    connection: { id: 'zenbase', name: 'zenbase docker' },
    state, compact, now, onTest: () => {}, onChoose: () => {}, ...extras
  } }).body;
});

after(async () => { await server?.close(); });

test('a failed connection keeps its name, shows the error in a red pill and remains retryable', () => {
  const html = renderPill({ status: 'error', origin: 'test', message: 'Connection refused', checkedAt: 12345 });
  assert.match(html, /connection-chip[^\"]*failed/);
  assert.match(html, /zenbase docker/);
  assert.match(html, /role="status">Connection refused/);
  assert.match(html, /Click to test again/);
  assert.doesNotMatch(html, / disabled/);
  assert.doesNotMatch(html, /connection-chip[^\"]*connected/);
});

test('a connection lost during a run reports Disconnected without repeating the query error', () => {
  for (const compact of [false, true]) {
    const html = renderPill({ status: 'error', origin: 'query', message: 'Connection closed by peer', checkedAt: 12345 }, compact);
    assert.match(html, /class="connection-name[^"]*">Disconnected/);
    assert.doesNotMatch(html, /Connection closed by peer/);
    assert.doesNotMatch(html, /connection-chip[^"]*connected/);
  }
});

test('the navbar keeps the manual test error and retry target after disconnecting', () => {
  const state = { status: 'error', origin: 'test', message: 'Connection refused', checkedAt: 12345 } as const;
  const html = renderPill(state, false, 12345, { online: false });
  assert.match(html, /class="connection-name[^"]*">Disconnected/);
  assert.match(html, /role="status">zenbase docker: Connection refused/);
  assert.match(html, /Click to test again/);
  assert.doesNotMatch(renderPill(state, true, 12345, { online: false }), /role="status">zenbase docker/);
});

test('testing displays progress and disables duplicate clicks', () => {
  const html = renderPill({ status: 'testing' });
  assert.match(html, /connection-chip[^\"]*testing/);
  assert.match(html, /aria-busy="true"/);
  assert.match(html, / disabled/);
  assert.match(html, /role="status">Testing…/);
});

test('both connection pills signal query activity with a dot and accessible label', () => {
  for (const compact of [false, true]) {
    const running = renderPill({ status: 'running' }, compact);
    assert.match(running, /connection-chip[^"]*running/);
    assert.match(running, /aria-label="Running on zenbase docker"/);
    assert.match(running, /aria-busy="true"/);
    assert.match(running, / disabled/);
    assert.match(running, /class="status-dot\b/);
    assert.doesNotMatch(running, /connection-message/);
    assert.doesNotMatch(running, /Testing/);
    const cancelling = renderPill({ status: 'testing' }, compact, 12345, { activity: 'cancelling' });
    assert.match(cancelling, /aria-label="Cancelling on zenbase docker"/);
    assert.doesNotMatch(cancelling, /connection-message/);
    assert.doesNotMatch(cancelling, /Testing/);
  }
});

test('navbar has separate retry and switch buttons; only the arrow opens a dialog', () => {
  const html = renderPill({ status: 'success', checkedAt: 12345 }, false, 12345, { switcher: true });
  assert.equal((html.match(/<button\b/g) ?? []).length, 2);
  assert.match(html, /aria-label="Test zenbase docker"/);
  assert.match(html, /class="switch-arrow[^>]*aria-label="Switch connection"[^>]*aria-haspopup="dialog"/);
  assert.equal((html.match(/aria-haspopup="dialog"/g) ?? []).length, 1);
});

test('only successful tests show green; selecting an untested connection is neutral', () => {
  assert.match(renderPill({ status: 'success', checkedAt: 12345 }), /connection-chip[^\"]*connected/);
  const untested = renderPill(null);
  assert.doesNotMatch(untested, /connection-chip[^\"]*connected/);
  assert.match(untested, /Click to check the connection/);
});

test('database errors are escaped, not treated as markup or instructions', () => {
  const html = renderPill({ status: 'error', origin: 'test', message: '<script>alert("bad")</script>', checkedAt: 12345 });
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
    const failed = renderPill({ status: 'error', origin: 'test', message: 'Connection refused', checkedAt: 12345 }, compact);
    assert.match(failed, /connection-chip[^\"]*failed/);
    assert.match(failed, /Connection refused/);
    assert.match(renderPill({ status: 'testing' }, compact), /aria-busy="true"/);
  }
});

test('the footer shortcut stays clickable and the toolbar pill remains in the Tab order', () => {
  const health = { status: 'success', checkedAt: 12345 } as const;
  const footer = renderPill(health, true);
  assert.match(footer, /<button[^>]*tabindex="-1"[^>]*aria-label="Test zenbase docker"/);
  assert.doesNotMatch(footer, / disabled/);
  assert.doesNotMatch(renderPill(health), /tabindex="-1"/);
});

test('a connection lock disables switching while leaving its health check available', () => {
  const extras = { switcher: true, connectionLocked: true, lockedConnectionName: 'zenbase docker' };
  const html = renderPill({ status: 'success', checkedAt: 12345 }, false, 12345, extras);
  const buttons = html.match(/<button\b[^>]*>/g)!;
  assert.doesNotMatch(buttons[0], / disabled/);
  assert.match(buttons[1], / disabled/);
  assert.match(html, /Connection locked to zenbase docker\. Right-click the tab to unlock/);
  const disconnected = renderPill(null, false, 12345, { ...extras, connection: null });
  assert.equal((disconnected.match(/ disabled/g) ?? []).length, 2);
  assert.match(disconnected, /Run a query to review and reconnect/);
});
