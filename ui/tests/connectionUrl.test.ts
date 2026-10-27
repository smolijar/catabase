import assert from 'node:assert/strict';
import test from 'node:test';
import { applyConnectionUrl, displayConnectionUrl, editsMaskedPassword, PASSWORD_MASK, redactConnectionUrl } from '../src/lib/connectionUrl.ts';
import type { ConnectionInput } from '../src/lib/types';

const connection: ConnectionInput = {
  id: 'docker', name: 'My connection', host: 'localhost', port: 5432,
  username: 'cat user', database: 'cat/db', password: 'keep-this-secret', ssl: true,
  options: { connectTimeoutSeconds: 12, queryTimeoutSeconds: 30, lockTimeoutSeconds: 2, readOnly: true }
};

test('display URLs contain only a fixed password mask and round-trip address fields', () => {
  const url = displayConnectionUrl(connection);
  assert.equal(url, `postgresql://cat%20user:${PASSWORD_MASK}@localhost:5432/cat%2Fdb?sslmode=require`);
  assert.doesNotMatch(url, /keep-this-secret/);
  assert.deepEqual(applyConnectionUrl(url, connection), connection);
  assert.equal(displayConnectionUrl({ ...connection, password: null }).includes(PASSWORD_MASK), false);
  assert.match(displayConnectionUrl({ ...connection, host: '::1' }), /@\[::1\]:5432/);
});

test('live URL edits update connection fields without changing name, ID, password or options', () => {
  const updated = applyConnectionUrl('postgres://reader:replace-me@127.0.0.1:5433/another%20db?sslmode=disable', connection);
  assert.deepEqual(updated, { ...connection, host: '127.0.0.1', port: 5433, username: 'reader', database: 'another db', ssl: false });
  assert.equal(updated.options, connection.options);
  assert.equal(applyConnectionUrl('postgres://reader@localhost/db?SSLMODE=verify-full', connection).ssl, true);
  assert.equal(applyConnectionUrl('postgres://reader@localhost/db', connection).port, 5432);
  assert.equal(applyConnectionUrl('postgres://reader:new-secret@localhost/db', { ...connection, password: null }).password, null);
});

test('pasted passwords are redacted even with invalid URLs and unescaped credential characters', () => {
  for (const secret of ['plain', 'p%40ss', 'p@ss', 'p:a:ss']) {
    const pasted = `postgresql://reader:${secret}@localhost:99999/db?password=other-secret`;
    const redacted = redactConnectionUrl(pasted, true);
    assert.equal(redacted, `postgresql://reader:${PASSWORD_MASK}@localhost:99999/db?password=${PASSWORD_MASK}`);
    assert.equal(redactConnectionUrl(pasted, false), `postgresql://reader@localhost:99999/db?password=${PASSWORD_MASK}`);
  }
});

test('partial and invalid URLs fail without modifying the form or echoing credentials', () => {
  for (const value of ['', 'postgresql://', 'https://reader@host/db', 'postgres://reader@host',
    'postgres://host/db', 'postgres://reader:secret@host:99999/db', 'postgres://reader@host:0/db', 'postgres://reader@host/%xx']) {
    assert.throws(() => applyConnectionUrl(value, connection), error => error instanceof Error && !error.message.includes('secret'));
  }
  assert.equal(connection.password, 'keep-this-secret');
});

test('the password segment rejects typing, deletion, cut and partial paste, but permits whole-URL replacement', () => {
  const url = displayConnectionUrl(connection);
  const start = url.indexOf(`:${PASSWORD_MASK}`), end = start + PASSWORD_MASK.length + 2;
  assert.equal(editsMaskedPassword(url, start + 2, start + 2, 'insertText'), true);
  assert.equal(editsMaskedPassword(url, start, start, 'deleteContentForward'), true);
  assert.equal(editsMaskedPassword(url, end, end, 'deleteContentBackward'), true);
  for (const kind of ['deleteByCut', 'insertFromPaste', 'insertText']) {
    assert.equal(editsMaskedPassword(url, start + 1, end - 1, kind), true);
    assert.equal(editsMaskedPassword(url, 0, url.length, kind), false);
  }
  assert.equal(editsMaskedPassword(url, end, end + 3, 'insertText'), false);
  assert.equal(editsMaskedPassword(url, start, start, 'insertText'), false);
});
