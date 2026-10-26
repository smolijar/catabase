import assert from 'node:assert/strict';
import test from 'node:test';
import { restoreAppearance, shortType } from '../src/lib/appearance.ts';

test('appearance restores only supported connection accents and bounded display labels', () => {
  assert.deepEqual(restoreAppearance(JSON.stringify({
    docker: { color: 'green', environment: 'local' },
    bad: { color: 'url(https://example.com)' },
    extra: { color: 'blue', environment: 'abcdefghijklmnopq', ignored: true },
    missingLabel: { color: 'none' },
    null: null
  })), {
    docker: { color: 'green', environment: 'local' },
    extra: { color: 'blue', environment: 'abcdefghijklmnop' },
    missingLabel: { color: 'none', environment: '' }
  });
  for (const value of [null, '', '{broken', 'null', '[]', 'true', '42']) {
    assert.deepEqual(restoreAppearance(value), {});
  }
});

test('compact type labels retain array identity and unfamiliar PostgreSQL types', () => {
  for (const [type, expected] of [
    ['_text', 'text[]'], ['_int4', 'int4[]'], ['text[][]', 'text[][]'],
    ['character varying(32)', 'varchar(32)'], ['timestamp with time zone', 'timestamptz'],
    ['timestamp without time zone', 'timestamp'], ['double precision', 'float8'],
    ['boolean', 'bool'], ['citext', 'citext']
  ]) assert.equal(shortType(type), expected);
});
