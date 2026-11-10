import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';
import type { ConnectionAppearance } from '../src/lib/appearance.ts';

let server: ViteDevServer;
let renderCard: (active?: boolean, appearance?: ConnectionAppearance, disabled?: boolean) => string;
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
  renderCard = (active = false, appearance, disabled = false) => render(Card, { props: {
    connection: { id: 'zenbase', name: 'zenbase docker', username: 'zenbase', host: 'localhost', port: 5432, database: 'zenbase' },
    active, appearance, disabled, onActivate: () => {}, onEdit: () => {}, onDelete: () => {}, onDuplicate: () => {}, onCopy: () => {}, onPaste: () => {}
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

test('inactive rows have one button and no visible or keyboard-focusable context menu trigger', () => {
  const html = renderCard();
  assert.equal((html.match(/<button\b/g) ?? []).length, 1);
  assert.doesNotMatch(html, /dropdown-trigger/);
  assert.match(html, /aria-haspopup="menu"/);
  assert.match(html, /Option\/Alt-click, right-click or Shift\+F10/);
  assert.match(html, /Click or press Enter to use and test/);
  assert.match(html, /Double-click or F2 to edit/);
  assert.match(html, /Cmd\/Ctrl\+C to copy/);
  assert.match(html, /Cmd\/Ctrl\+V to duplicate/);
  assert.match(html, /data-connection-id="zenbase"/);
});

test('only the selected row gets a separate disconnect button', () => {
  const html = renderCard(true);
  assert.match(html, /aria-pressed="true"/);
  assert.match(html, /connection-item[^\"]*active/);
  assert.equal((html.match(/<button\b/g) ?? []).length, 2);
  assert.match(html, /<\/button>\s*<!--[\s\S]*?<button[^>]*aria-label="Disconnect zenbase docker"/);
  assert.doesNotMatch(renderCard(), /aria-label="Disconnect zenbase docker"/);
  assert.match(renderCard(true, undefined, true), /<button[^>]*disconnect-button[^>]* disabled[^>]*aria-label="Disconnect zenbase docker"/);
});

test('appearance is visible on inactive rows, including colors without labels and labels without colors', () => {
  const styled = renderCard(false, { color: 'red', environment: 'PROD' });
  assert.match(styled, /class="connection-color[^"]*" data-conn="red"/);
  assert.match(styled, /class="environment[^"]*" data-conn="red" title="PROD">PROD<\/span>/);
  assert.match(renderCard(false, { color: 'blue', environment: '' }), /class="connection-color[^"]*" data-conn="blue"/);
  const neutral = renderCard(false, { color: 'none', environment: 'LOCAL' });
  assert.doesNotMatch(neutral, /connection-color|data-conn=/);
  assert.match(neutral, /class="environment[^"]*" title="LOCAL">LOCAL<\/span>/);
});

test('context-only menus hide their trigger without changing other action menus', () => {
  assert.doesNotMatch(renderMenu(true), /<button\b/);
  assert.match(renderMenu(false), /<button\b[^>]*class="dropdown-trigger/);
});
