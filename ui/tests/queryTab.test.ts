import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';

let server: ViteDevServer;
let renderTab: (props?: Record<string, unknown>) => string;

before(async () => {
  server = await createServer({
    root: fileURLToPath(new URL('..', import.meta.url)),
    server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: 'custom'
  });
  const { default: Tab } = await server.ssrLoadModule('/src/components/QueryTab.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  renderTab = (props = {}) => render(Tab, { props: {
    id: 'query', title: 'My query', connectionName: 'zenbase docker',
    onSelect: () => {}, onRename: () => {}, onDuplicate: () => {}, onClose: () => {},
    onRenameValue: () => {}, onRenameKeydown: () => {}, onRenameBlur: () => {}, ...props
  } }).body;
});

after(async () => { await server?.close(); });

test('tabs expose context actions without adding another visible menu button', () => {
  const html = renderTab({ active: true });
  assert.equal((html.match(/<button\b/g) ?? []).length, 2);
  assert.match(html, /data-tab-id="query"/);
  assert.match(html, /aria-pressed="true"/);
  assert.match(html, /aria-haspopup="menu"/);
  assert.match(html, /Right-click or Shift\+F10/);
  assert.match(html, /Double-click or F2 to rename/);
  assert.match(html, /Cmd\/Ctrl\+C to copy; Cmd\/Ctrl\+V to duplicate/);
  assert.doesNotMatch(html, /dropdown-trigger/);
});

test('running tabs cannot be closed and show their activity dot alongside staged edits', () => {
  const html = renderTab({ running: true, closeDisabled: true, pending: true });
  assert.match(html, /aria-label="Close My query" disabled/);
  assert.match(html, /class="operation-dot" role="img" aria-label="Operation running"/);
  assert.match(html, /Staged edits/);
});

test('tabs can show a steady failure indicator for a query that finished in the background', () => {
  assert.match(renderTab({ failed: true }), /class="tab-error" role="img" aria-label="Query failed"/);
  assert.doesNotMatch(renderTab(), /tab-error/);
});

test('rename is still an input without a confirm button; reference tabs keep their read-only label', () => {
  const html = renderTab({ renaming: true, renameValue: 'New title' });
  assert.match(html, /aria-label="Tab name" value="New title"/);
  assert.doesNotMatch(html, /<button\b/);
  assert.match(renderTab({ readonly: true }), /read only/);
});

test('tab labels expose saved-order drag and keyboard reorder affordances', () => {
  const html = renderTab({ dragging: true, dropSide: 'before' });
  assert.match(html, /data-tab-chip-id="query"/);
  assert.match(html, /tab-chip[^\"]*dragging[^\"]*drop-before/);
  assert.match(html, /Drag or Alt\+Shift\+Left\/Right to reorder/);
});

test('connected tab identity is visible in its own color, even when inactive or disconnected', () => {
  const props = { connectionName: 'Production <admin>', connectionColor: 'red', hasConnection: true, connectionLocked: true };
  const html = renderTab(props);
  assert.match(html, /data-conn="red"/);
  assert.match(html, /tab-chip[^\"]*connection-colored/);
  assert.match(html, /class="tab-lock/);
  assert.match(html, /Connection locked to Production &lt;admin>/);
  assert.match(html, /Right-click this tab to unlock; connection switching is disabled/);
  assert.doesNotMatch(html, /<admin>/);
  assert.doesNotMatch(renderTab({ ...props, connectionLocked: false }), /class="tab-lock/);
});
