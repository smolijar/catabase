import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';

let server: ViteDevServer;
let renderCard: (active?: boolean) => string;
let renderMenu: (contextOnly: boolean) => string;

before(async () => {
  server = await createServer({
    root: fileURLToPath(new URL('..', import.meta.url)),
    server: { middlewareMode: true, watch: null, hmr: false, ws: false },
    appType: 'custom'
  });
  const { default: Card } = await server.ssrLoadModule('/src/components/ConnectionCard.svelte');
  const { default: Menu } = await server.ssrLoadModule('/src/components/ActionMenu.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  renderCard = (active = false) => render(Card, { props: {
    connection: { id: 'zenbase', name: 'zenbase docker', username: 'zenbase', host: 'localhost', port: 5432, database: 'zenbase' },
    active, onActivate: () => {}, onEdit: () => {}, onDelete: () => {}, onDuplicate: () => {}, onCopy: () => {}, onPaste: () => {}
  } }).body;
  renderMenu = (contextOnly) => render(Menu, { props: {
    contextOnly, ariaLabel: 'Actions', items: [{ id: 'edit', label: 'Edit' }]
  } }).body;
});

after(async () => { await server?.close(); });

test('connection rows display a bold name followed by a parenthesized username', () => {
  const html = renderCard();
  assert.match(html, /<strong[^>]*>zenbase docker<\/strong><span[^>]*> \(zenbase\)<\/span>/);
  assert.doesNotMatch(html, /class="connection-address/);
  assert.doesNotMatch(html, /current-badge/);
});

test('the row is the only button; its action menu has no visible or keyboard-focusable trigger', () => {
  const html = renderCard();
  assert.equal((html.match(/<button\b/g) ?? []).length, 1);
  assert.doesNotMatch(html, /dropdown-trigger/);
  assert.match(html, /aria-haspopup="menu"/);
  assert.match(html, /Option\/Alt-click, right-click or Shift\+F10/);
  assert.match(html, /Cmd\/Ctrl\+C to copy/);
  assert.match(html, /Cmd\/Ctrl\+V to duplicate/);
  assert.match(html, /data-connection-id="zenbase"/);
});

test('active connection rows retain an accessible selected state without a second-line badge', () => {
  const html = renderCard(true);
  assert.match(html, /aria-pressed="true"/);
  assert.match(html, /connection-item[^\"]*active/);
  assert.equal((html.match(/<button\b/g) ?? []).length, 1);
});

test('context-only menus hide their trigger without changing other action menus', () => {
  assert.doesNotMatch(renderMenu(true), /<button\b/);
  assert.match(renderMenu(false), /<button\b[^>]*class="dropdown-trigger/);
});
