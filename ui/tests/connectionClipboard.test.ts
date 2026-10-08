import assert from 'node:assert/strict';
import test from 'node:test';
import { connectionClipboardAction, parseConnectionReference, serializeConnectionReference } from '../src/lib/connectionClipboard.ts';

test('connection clipboard references contain only an ID, never credentials or configuration', () => {
  const connection = { id: 'source-id', name: 'zenbase docker', password: 'test-secret', username: 'test-user', host: 'private-host' };
  const text = serializeConnectionReference(connection);
  assert.deepEqual(JSON.parse(text), { kind: 'catabase.connection', version: 1, id: 'source-id' });
  assert.doesNotMatch(text, /test-secret|test-user|private-host|password|username|host/);
  assert.equal(parseConnectionReference(text), connection.id);
});

test('ordinary clipboard text, URLs, malformed references and unknown versions cannot duplicate a connection', () => {
  for (const text of ['select 1', 'postgres://user:secret@host/db', '', 'null', '[]', '{}',
    '{"kind":"catabase.connection","version":2,"id":"source"}',
    '{"kind":"catabase.connection","version":1,"id":42}',
    '{"kind":"catabase.connection","version":1,"id":" "}',
    JSON.stringify({ kind: 'catabase.connection', version: 1, id: 'x'.repeat(129) }), 'x'.repeat(4097)]) {
    assert.equal(parseConnectionReference(text), null);
  }
});

const keyEvent = { key: 'c', metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, isComposing: false, repeat: false, defaultPrevented: false };

test('connection shortcuts support Command on Mac and Control elsewhere', () => {
  assert.equal(connectionClipboardAction({ ...keyEvent, metaKey: true }), 'copy');
  assert.equal(connectionClipboardAction({ ...keyEvent, ctrlKey: true }), 'copy');
  assert.equal(connectionClipboardAction({ ...keyEvent, metaKey: true, key: 'v' }), 'paste');
  assert.equal(connectionClipboardAction({ ...keyEvent, ctrlKey: true, key: 'V' }), 'paste');
});

test('unmodified typing, unrelated shortcuts and IME input are not connection shortcuts', () => {
  assert.equal(connectionClipboardAction(keyEvent), null);
  assert.equal(connectionClipboardAction({ ...keyEvent, metaKey: true, key: 'x' }), null);
  for (const flag of ['altKey', 'shiftKey', 'isComposing', 'defaultPrevented'] as const) {
    assert.equal(connectionClipboardAction({ ...keyEvent, metaKey: true, [flag]: true }), null);
  }
});

test('repeated clipboard shortcuts are recognized so the row can suppress native paste without duplicating again', () => {
  assert.equal(connectionClipboardAction({ ...keyEvent, metaKey: true, key: 'v', repeat: true }), 'paste');
});
