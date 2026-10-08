import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';

let server: ViteDevServer;
let renderResizer: (width: number, workspaceWidth: number) => string;

before(async () => {
  server = await createServer({
    root: fileURLToPath(new URL('..', import.meta.url)),
    server: { middlewareMode: true, watch: null, hmr: false, ws: false },
    appType: 'custom'
  });
  const { default: Resizer } = await server.ssrLoadModule('/src/components/SidebarResizer.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  renderResizer = (width, workspaceWidth) => render(Resizer, { props: { width, workspaceWidth } }).body;
});

after(async () => { await server?.close(); });

test('sidebar divider exposes a keyboard-focusable vertical separator with its width and controls', () => {
  const html = renderResizer(484, 1780);
  assert.match(html, /role="separator"/);
  assert.match(html, /aria-label="Resize sidebar"/);
  assert.match(html, /aria-orientation="vertical"/);
  assert.match(html, /aria-controls="workspace-sidebar"/);
  assert.match(html, /tabindex="0"/);
  assert.match(html, /aria-valuemin="220"/);
  assert.match(html, /aria-valuemax="640"/);
  assert.match(html, /aria-valuenow="484"/);
  assert.match(html, /Double-click to reset/);
});

test('accessible width follows the visible clamped layout, not an offscreen stored preference', () => {
  const html = renderResizer(640, 960);
  assert.match(html, /aria-valuemax="415"/);
  assert.match(html, /aria-valuenow="415"/);
  assert.match(html, /aria-valuetext="415 pixels"/);
});
