import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';

let server: ViteDevServer;
let renderPicker: (props?: Record<string, unknown>) => string;
const target = { schema: 'public', table: 'children', key: { name: 'parent_fk', columns: ['parent_id'], referencedSchema: 'public', referencedTable: 'parents', referencedColumns: ['id'] }, mappings: [], unavailableReason: '' };

before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('..', import.meta.url)), server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: 'custom' });
  const { default: Picker } = await server.ssrLoadModule('/src/components/ForeignKeyPicker.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  renderPicker = (props = {}) => render(Picker, { props: { targets: [target], onLookup: async () => {}, onCancel: () => {}, onPick: () => {}, ...props } }).body;
});
after(async () => { await server?.close(); });

test('the FK preview has a bounded search input and explains staged/committed-row semantics', () => {
  const html = renderPicker();
  assert.match(html, /aria-label="Foreign key values"/);
  assert.match(html, /public.parents \(id\)/);
  assert.match(html, /aria-label="Search referenced values"/);
  assert.match(html, /maxlength="512"/);
  assert.match(html, /Preview uses committed rows/);
  assert.match(html, /Choose a row, then stage the change/);
});

test('unavailable composite mappings show their explanation without a misleading search field', () => {
  const html = renderPicker({ targets: [{ ...target, unavailableReason: 'Missing: tenant_id' }] });
  assert.match(html, /Missing: tenant_id/);
  assert.doesNotMatch(html, /Search referenced values/);
});

test('multiple relationships can be chosen and schema names are escaped as data', () => {
  const html = renderPicker({ targets: [target, { ...target, key: { ...target.key, name: 'other', referencedTable: '<script>' } }] });
  assert.match(html, /Foreign key relationship/);
  assert.doesNotMatch(html, /<script>/);
  assert.match(renderPicker({ disabled: true }), /maxlength="512"[^>]*disabled/);
});
