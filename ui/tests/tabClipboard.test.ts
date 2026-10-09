import assert from 'node:assert/strict';
import test from 'node:test';
import { parseTabReference, serializeTabReference } from '../src/lib/tabClipboard.ts';
import { serializeConnectionReference } from '../src/lib/connectionClipboard.ts';

test('tab copies include only the reference, never SQL, results, connection choice or staged changes', () => {
  const tab = { id: 'source', title: 'Private draft', sql: "select 'secret'", connectionId: 'private-db', session: { data: { rows: [{ password: 'secret' }] } }, pendingChanges: [{ value: 'changed' }] };
  const text = serializeTabReference(tab);
  assert.deepEqual(JSON.parse(text), { kind: 'catabase.tab', version: 1, id: 'source' });
  assert.equal(parseTabReference(text), tab.id);
  assert.doesNotMatch(text, /secret|Private|password|connection|session|changed|sql/);
});

test('ordinary text, SQL, connection references and invalid or oversized values cannot duplicate tabs', () => {
  for (const text of ['', 'select 1', 'null', '[]', '{}', serializeConnectionReference({ id: 'source' }),
    JSON.stringify({ kind: 'catabase.tab', version: 2, id: 'source' }),
    JSON.stringify({ kind: 'catabase.tab', version: 1, id: 42 }),
    JSON.stringify({ kind: 'catabase.tab', version: 1, id: ' ' }),
    JSON.stringify({ kind: 'catabase.tab', version: 1, id: 'x'.repeat(129) }), 'x'.repeat(4097)]) {
    assert.equal(parseTabReference(text), null);
  }
});

test('reference IDs work for query UUIDs and read-only DDL/history tabs', () => {
  for (const id of ['0523bf48-4937-4580-94ab-cfa6abb8ef02', 'ddl:connection:public.properties', 'history:123']) {
    assert.equal(parseTabReference(serializeTabReference({ id })), id);
  }
});
