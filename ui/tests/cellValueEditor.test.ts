import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';

let server: ViteDevServer;
let renderEditor: (props?: Record<string, unknown>) => string;
let renderOption: (props?: Record<string, unknown>) => string;
const column = { name: 'parent_id', typeName: 'text', typeOid: 25, generated: false, primaryKey: false, nullable: true };

before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('..', import.meta.url)), server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: 'custom' });
  const { default: Editor } = await server.ssrLoadModule('/src/components/CellValueEditor.svelte');
  const { default: Option } = await server.ssrLoadModule('/src/components/ForeignKeyOption.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  renderEditor = (props = {}) => render(Editor, { props: { column, draft: 'parent-1', ...props } }).body;
  renderOption = (props = {}) => render(Option, { props: { row: { id: 'parent-1', name: 'Parent One', description: null }, keyColumns: ['id'], previewColumns: ['name', 'description'], onPick: () => {}, ...props } }).body;
});
after(async () => { await server?.close(); });

test('FK values are read-only output without free-form inputs or a NULL checkbox', () => {
  const html = renderEditor({ pickerOnly: true });
  assert.match(html, /<output[^>]*aria-label="Selected value for parent_id"/);
  assert.match(html, />parent-1<\/output>/);
  assert.doesNotMatch(html, /<textarea|<input|Edit value|empty value is an empty string/);
  const nullHtml = renderEditor({ pickerOnly: true, isNull: true });
  assert.match(nullHtml, /Selected SQL NULL for parent_id/);
  assert.match(nullHtml, />NULL<\/output>/);
  assert.doesNotMatch(nullHtml, /<textarea|<input|Edit value/);
});

test('non-FK cells retain typed editing without a NULL checkbox', () => {
  assert.match(renderEditor(), /<textarea[^>]*aria-label="New value for parent_id"/);
  assert.match(renderEditor({ column: { ...column, typeName: 'int8' } }), /inputmode="decimal"/);
  const boolHtml = renderEditor({ column: { ...column, typeName: 'bool' }, draft: 'false' });
  assert.equal((boolHtml.match(/type="radio"/g) ?? []).length, 2);
  for (const html of [renderEditor(), boolHtml]) assert.doesNotMatch(html, /type="checkbox"/);
  assert.match(renderEditor({ error: 'Invalid' }), /aria-invalid="true"/);
});

test('array editors explain native array syntax, empty arrays and the different NULL forms', () => {
  const html = renderEditor({ column: { ...column, typeName: '_text' }, draft: '{"one",NULL}' });
  assert.match(html, /<textarea/);
  assert.match(html, /PostgreSQL array/);
  assert.match(html, /\{\} is empty/);
  assert.match(html, /Dimensions and numeric precision are preserved/);
  assert.doesNotMatch(html, /empty value is an empty string/);
});

test('non-FK NULL drafts have an Edit value action, including a disabled state', () => {
  const html = renderEditor({ isNull: true });
  assert.match(html, /No value will be stored/);
  assert.match(html, /<button[^>]*>Edit value<\/button>/);
  assert.doesNotMatch(html, /<textarea|type="checkbox"/);
  assert.match(renderEditor({ isNull: true, disabled: true }), /<button[^>]*disabled[^>]*>Edit value/);
});

test('compact FK options put the row preview and key inline, with full details in the tooltip', () => {
  const html = renderOption({ selected: true });
  assert.match(html, /<strong[^>]*>Parent One<\/strong>/);
  assert.match(html, /class="fk-key[^\"]*"[^>]*>parent-1<\/span>/);
  assert.match(html, /title="id: parent-1\nname: Parent One"/);
  assert.match(html, /aria-pressed="true"/);
  assert.doesNotMatch(html, /description:|NULL|<small/);
});

test('compact FK options support key-only/composite rows and escape preview text', () => {
  const html = renderOption({ row: { id: '9223372036854775807', tenant: '<team>' }, keyColumns: ['id', 'tenant'], previewColumns: [], disabled: true });
  assert.match(html, /9223372036854775807 · &lt;team(?:>|&gt;)/);
  assert.match(html, /class="[^\"]*\bkey-only\b[^\"]*"/);
  assert.match(html, /disabled/);
  assert.doesNotMatch(html, /class="fk-key|<team>/);
  assert.doesNotMatch(renderOption({ row: { id: '1', name: '<script>' } }), /<script>/);
});
