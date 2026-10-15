import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';
import type { ColumnMeta, QueryResult, SchemaItem } from '../src/lib/types.ts';

let server: ViteDevServer;
let renderGrid: () => string;

before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('..', import.meta.url)), server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: 'custom' });
  const { default: Grid } = await server.ssrLoadModule('/src/components/ResultsGrid.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  const columns: ColumnMeta[] = [
    { name: 'id', typeName: 'int8', sourceName: 'id', primaryKey: true },
    { name: 'parent', typeName: 'text', sourceName: 'parent_id' },
    { name: 'text', typeName: 'text' },
    { name: 'date', typeName: 'timestamptz' },
    { name: 'enabled', typeName: 'bool' },
    { name: 'disabled', typeName: 'bool' },
    { name: 'missing', typeName: 'text' }
  ].map((column) => ({ typeOid: 0, generated: false, primaryKey: false, tableSchema: 'public', tableName: 'children', ...column }));
  const result: QueryResult = {
    columns, rows: [{ id: '9007199254740993', parent: 'parent-1', text: '0123', date: '2026-10-15T12:00:00+02:00', enabled: true, disabled: false, missing: null }],
    rowCount: 1, durationMs: 1, autoLimited: false, hasMore: false
  };
  const schemaItems: SchemaItem[] = [{ schema: 'public', name: 'children', kind: 'table', columns: [], foreignKeys: [{ name: 'parent_fk', columns: ['parent_id'], referencedSchema: 'public', referencedTable: 'parents', referencedColumns: ['id'] }] }];
  renderGrid = () => render(Grid, { props: { result, schemaItems, changedCells: [{ rowIndex: 0, column: 'id' }], onForeignKeyLookup: async () => { throw new Error('Rendering must not query the database'); } } }).body;
});

after(async () => { await server?.close(); });

test('compact headers keep type/source/key information in accessible tooltips, with PK and FK icons', () => {
  const html = renderGrid();
  const header = html.slice(html.indexOf('<thead'), html.indexOf('</thead>'));
  assert.doesNotMatch(header, /<small/);
  assert.match(header, /column-key/);
  assert.match(header, /column-link/);
  assert.match(header, /title="Primary key"/);
  assert.match(header, /title="Foreign key"/);
  assert.match(header, /Type: int8/);
  assert.match(header, /Source: public.children.parent_id/);
  assert.match(header, /Foreign key \(parent_id\) → public.parents \(id\)/);
  assert.match(header, /aria-sort="none"/);
  assert.match(header, /Resize id column/);
});

test('typed cells retain exact values, native booleans, NULL and their staged-change indicator', () => {
  const html = renderGrid();
  for (const type of ['number', 'string', 'date', 'boolean', 'null']) assert.match(html, new RegExp(`cell value-${type}`));
  assert.match(html, /value-boolean[^\"]*false-value/);
  assert.match(html, />9007199254740993<\/button>/);
  assert.match(html, />0123<\/button>/);
  assert.match(html, />2026-10-15T12:00:00\+02:00<\/button>/);
  assert.match(html, />true<\/button>/);
  assert.match(html, />false<\/button>/);
  assert.match(html, />NULL<\/button>/);
  assert.match(html, /<td class="[^\"]*\bchanged\b/);
  assert.match(html, /Row 1, id \(int8\): 9007199254740993/);
});
