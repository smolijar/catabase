import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import type { ViteDevServer } from 'vite';
import { sameConnectionSettings } from '../src/lib/connectionEditor.ts';
import { connectionAppearanceDraft } from '../src/lib/appearance.ts';
import type { ConnectionInput } from '../src/lib/types';

const connection: ConnectionInput = { id: 'docker', name: 'zenbase docker', host: 'localhost', port: 5432, database: 'zenbase', username: 'zenbase', ssl: false };
let server: ViteDevServer;
let renderEditor: (props?: Record<string, unknown>) => string;

before(async () => {
  server = await createServer({
    root: fileURLToPath(new URL('..', import.meta.url)),
    server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: 'custom'
  });
  const { default: Editor } = await server.ssrLoadModule('/src/components/ConnectionEditor.svelte');
  const { render } = await server.ssrLoadModule('svelte/server');
  renderEditor = (props = {}) => render(Editor, { props: {
    connection: { ...connection }, appearance: { color: 'blue', environment: 'LOCAL' },
    onClose: () => {}, onSave: () => {}, onTest: () => {}, onImport: () => {}, onConnectionInput: () => {}, ...props
  } }).body;
});
after(async () => { await server?.close(); });

test('connection editor exposes linked tabs with a single keyboard tab stop', () => {
  const html = renderEditor();
  assert.match(html, /role="tablist" aria-label="Connection settings"/);
  assert.match(html, /id="connection-editor-connection-tab"[^>]*aria-controls="connection-editor-connection-panel"[^>]*aria-selected="true" tabindex="0"/);
  assert.match(html, /id="connection-editor-appearance-tab"[^>]*aria-controls="connection-editor-appearance-panel"[^>]*aria-selected="false" tabindex="-1"/);
  assert.match(html, /id="connection-editor-appearance-panel"[^>]*hidden/);
});

test('appearance panel retains connection inputs but hides them and exposes saved preferences', () => {
  const html = renderEditor({ section: 'appearance' });
  assert.match(html, /id="connection-editor-connection-panel"[^>]*hidden/);
  assert.doesNotMatch(html, /id="connection-editor-appearance-panel"[^>]*hidden/);
  assert.match(html, /type="password"/);
  assert.match(html, /aria-label="blue"[^>]*aria-pressed="true"/);
  assert.match(html, /maxlength="16"[^>]*value="LOCAL"/);
  assert.match(html, /type="submit"[^>]*>Save</);
  assert.doesNotMatch(html, />Test</);
  // Validation is handled after revealing hidden fields, not by the browser's
  // automatic submit validation while the required fields are inaccessible.
  assert.match(html, /<form[^>]*novalidate/);
});

test('appearance drafts target the edited connection and cannot mutate saved preferences', () => {
  const saved = { docker: { color: 'blue' as const, environment: 'LOCAL' }, other: { color: 'red' as const, environment: 'PROD' } };
  const draft = connectionAppearanceDraft(saved, 'docker');
  draft.color = 'green';
  draft.environment = 'discard me';
  assert.deepEqual(saved.docker, { color: 'blue', environment: 'LOCAL' });
  assert.deepEqual(saved.other, { color: 'red', environment: 'PROD' });
  assert.deepEqual(connectionAppearanceDraft(saved, 'docker'), saved.docker);
  assert.deepEqual(connectionAppearanceDraft(saved), { color: 'none', environment: '' });
  assert.deepEqual(connectionAppearanceDraft(saved, 'missing'), { color: 'none', environment: '' });
});

test('the first color option is an empty no-color swatch, not an orange override', () => {
  const html = renderEditor({ section: 'appearance', appearance: { color: 'none', environment: '' } });
  const swatches = [...html.matchAll(/<button[^>]*class="color-option"[^>]*>[\s\S]*?<\/button>/g)].map(match => match[0]);
  assert.equal(swatches.length, 6);
  assert.match(swatches[0], /data-conn="none"/);
  assert.match(swatches[0], /aria-label="No color"/);
  assert.match(swatches[0], /aria-pressed="true"/);
  assert.match(swatches[0], /><\/button>$/);
  assert.doesNotMatch(swatches[0], /style=|Default ginger/);
  assert.match(swatches[1], /data-conn="red"/);
});

test('unchanged connection settings permit a local appearance-only save', () => {
  assert.equal(sameConnectionSettings(connection, { ...connection }), true);
  assert.equal(sameConnectionSettings(connection, { ...connection, password: '' }), true);
  assert.equal(sameConnectionSettings(connection, { ...connection, password: null }), true);
  for (const change of [
    { id: 'other' }, { name: 'Renamed' }, { host: 'elsewhere' }, { port: 5433 },
    { database: 'other' }, { username: 'other' }, { password: 'changed' }, { ssl: true }
  ]) assert.equal(sameConnectionSettings(connection, { ...connection, ...change }), false);
});
