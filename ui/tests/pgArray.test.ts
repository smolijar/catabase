import assert from 'node:assert/strict';
import test from 'node:test';
import { arrayElementType, parsePgArray } from '../src/lib/pgArray.ts';

test('array types are recognized in wire and reflected spellings, never from cell text', () => {
  for (const type of ['_text', 'text[]', 'pg_catalog.text[]', 'text[][]']) assert.equal(arrayElementType(type), 'text');
  for (const type of ['text', 'jsonb', 'int8']) assert.equal(arrayElementType(type), null);
});

test('array literals distinguish SQL NULL elements, NULL text, empty strings and empty arrays', () => {
  assert.equal(parsePgArray('{}').literal, '{}');
  assert.equal(parsePgArray('{NULL,"NULL","",null}').literal, '{NULL,"NULL","",NULL}');
  assert.equal(parsePgArray('{a, b, " c "}').literal, '{"a","b"," c "}');
  assert.equal(parsePgArray('{\\NULL,\\ }').literal, '{"NULL"," "}');
});

test('escaping and multiline formatting round trip exactly, without parsing element numbers', () => {
  const literal = String.raw`{"a,b","{braces}","quote\"","slash\\","line
break","9007199254740993","0.12345678901234567890","žluťoučký"}`;
  const parsed = parsePgArray(literal);
  assert.equal(parsed.literal, literal);
  assert.match(parsed.pretty, /\n  /);
  assert.equal(parsePgArray(parsed.pretty).literal, literal);
});

test('rectangular multidimensional arrays retain explicit non-one lower bounds', () => {
  const parsed = parsePgArray('[-2:-1][3:4]={{1,NULL},{2,3}}');
  assert.deepEqual(parsed.dimensions, [2, 2]);
  assert.equal(parsed.literal, '[-2:-1][3:4]={{"1",NULL},{"2","3"}}');
  assert.equal(parsePgArray(parsed.pretty).literal, parsed.literal);
  assert.equal(parsePgArray('[1:2]={a,b}').literal, '{"a","b"}');
});

test('malformed, ragged, over-deep arrays and legacy placeholders cannot be staged', () => {
  for (const value of ['', '<_text>', 'NULL', '[]', '{a,}', '{,a}', '{"a}', '{a"b}', '{{1},{2,3}}', '{{}, {}}', '{a,{b}}', '[0:2]={a,b}', '[1:2147483648]={a}', '{a} drop table x', '{{{{{{{1}}}}}}}']) assert.throws(() => parsePgArray(value), /Invalid PostgreSQL array/, value);
});
