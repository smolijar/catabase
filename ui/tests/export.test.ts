import assert from 'node:assert/strict';
import test from 'node:test';
import { serializeRows } from '../src/lib/export.ts';
import type { ColumnMeta, QueryResult } from '../src/lib/types.ts';

function result(types: Record<string, string>, rows: Record<string, unknown>[]): QueryResult {
  const columns: ColumnMeta[] = Object.entries(types).map(([name, typeName]) => ({ name, typeName, typeOid: 0, generated: false, primaryKey: false }));
  return { columns, rows, rowCount: rows.length, durationMs: 0, autoLimited: false, hasMore: false, jsonValuesAsText: true };
}

test('JSON columns export nested values with their original exact numeric literals', () => {
  const raw = '{"big":9223372036854775807,"decimal":0.123456789012345678901,"nested":[true,{"x":"a  b"}]}';
  const serialized = serializeRows(result({ payload: 'jsonb', text: 'text', amount: 'numeric' }, [{ payload: raw, text: raw, amount: '0.123456789012345678901' }]), 'json');
  assert.match(serialized, /"payload": \{"big":9223372036854775807,"decimal":0\.123456789012345678901/);
  const parsed = JSON.parse(serialized);
  assert.deepEqual(parsed[0].payload.nested, [true, { x: 'a  b' }]);
  assert.equal(parsed[0].text, raw);
  assert.equal(parsed[0].amount, '0.123456789012345678901');
});

test('JSON literal null is not a string, while text and JSON string scalars remain strings', () => {
  const serialized = serializeRows(result({ jsonNull: 'json', sqlNull: 'jsonb', jsonString: 'json', text: 'text', bool: 'jsonb' }, [
    { jsonNull: 'null', sqlNull: null, jsonString: '"null"', text: 'null', bool: 'false' }
  ]), 'json');
  // Standard row JSON represents both SQL NULL and JSON literal null as null.
  assert.deepEqual(JSON.parse(serialized), [{ jsonNull: null, sqlNull: null, jsonString: 'null', text: 'null', bool: false }]);
});

test('CSV/TSV preserve raw JSON and distinguish JSON literal null from SQL NULL', () => {
  const data = result({ payload: 'jsonb', sqlNull: 'jsonb', jsonNull: 'jsonb', text: 'text' }, [
    { payload: '{"n":9223372036854775807}', sqlNull: null, jsonNull: 'null', text: 'a\rb' }
  ]);
  assert.equal(serializeRows(data, 'csv'), 'payload,sqlNull,jsonNull,text\n"{""n"":9223372036854775807}",,null,"a\rb"');
  assert.equal(serializeRows(data, 'tsv'), 'payload\tsqlNull\tjsonNull\ttext\n"{""n"":9223372036854775807}"\t\tnull\t"a\rb"');
});

test('legacy decoded JSON and invalid raw strings remain valid exported JSON', () => {
  const data = result({ old: 'jsonb', string: 'json', escaped: 'text' }, [
    { old: { nested: [1, 2] }, string: 'plain legacy scalar', escaped: 'line\n"quoted"' },
    { old: 'null, "injected": true', string: '\n [1, 2] \n', escaped: '' }
  ]);
  assert.deepEqual(JSON.parse(serializeRows(data, 'json')), [
    { old: { nested: [1, 2] }, string: 'plain legacy scalar', escaped: 'line\n"quoted"' },
    { old: 'null, "injected": true', string: [1, 2], escaped: '' }
  ]);
  assert.equal(serializeRows(result({ value: 'json' }, []), 'json'), '[]');
});

test('legacy history JSON string scalars remain strings even when they look like raw JSON', () => {
  const data = result({ jsonNullString: 'jsonb', arrayString: 'json', numberString: 'jsonb', object: 'jsonb' }, [
    { jsonNullString: 'null', arrayString: '[1,2]', numberString: '123', object: { nested: [true] } }
  ]);
  delete data.jsonValuesAsText;
  assert.deepEqual(JSON.parse(serializeRows(data, 'json')), data.rows);
  data.jsonValuesAsText = false;
  assert.deepEqual(JSON.parse(serializeRows(data, 'json')), data.rows);
});
