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
  assert.doesNotMatch(html, /dropdown-trigger/);
});

test('running tabs cannot be closed and keep their progress indicator', () => {
  const html = renderTab({ running: true, closeDisabled: true, pending: true });
  assert.match(html, /aria-label="Close My query" disabled/);
  assert.match(html, /spinner/);
  assert.match(html, /Staged edits/);
});

test('rename is still an input without a confirm button; reference tabs keep their read-only label', () => {
  const html = renderTab({ renaming: true, renameValue: 'New title' });
  assert.match(html, /aria-label="Tab name" value="New title"/);
  assert.doesNotMatch(html, /<button\b/);
  assert.match(renderTab({ readonly: true }), /read only/);
});
