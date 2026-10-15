import assert from 'node:assert/strict';
import test from 'node:test';
import { schemaGroundedSql, schemaInlineIdentifierEnd, supermavenSchemaContext } from '../src/lib/sqlCompletion.ts';
import type { SchemaItem } from '../src/lib/types.ts';

const landlords: SchemaItem = {
  schema: 'public', name: 'landlords', kind: 'table',
  columns: ['id', 'name'].map((name) => ({ name, typeName: 'text', nullable: false, generated: false, primaryKey: name === 'id' })),
};
const properties: SchemaItem = {
  schema: 'public', name: 'properties', kind: 'table',
  columns: ['id', 'landlord_id', 'address'].map((name) => ({ name, typeName: 'text', nullable: true, generated: false, primaryKey: name === 'id' })),
  rowKeys: [{ schema: 'public', table: 'properties', columns: ['id'], primaryKey: true, nullsNotDistinct: false }],
  foreignKeys: [{ name: 'fk', columns: ['landlord_id'], referencedSchema: 'public', referencedTable: 'landlords', referencedColumns: ['id'] }],
};
const items = [landlords, properties];
function accepts(prefix: string, suggestion: string, suffix = '') {
  return schemaGroundedSql(prefix + suggestion + suffix, prefix.length, prefix.length + suggestion.length, items);
}

test('schema context includes exact columns, primary/unique keys and FK targets, not object payloads', () => {
  const schema = supermavenSchemaContext('select * from properties', 25, items);
  assert.match(schema, /CREATE TABLE "public"\."properties"/);
  assert.match(schema, /"landlord_id" text/);
  assert.match(schema, /PRIMARY KEY \("id"\)/);
  assert.match(schema, /FOREIGN KEY \("landlord_id"\) REFERENCES "public"\."landlords" \("id"\)/);
  assert.doesNotMatch(schema, /tableOid|rowKeys|password|fk"/);
});

test('schema context is UTF-8 bounded, prioritizes mentioned/prefix-matched tables, and never truncates definitions', () => {
  const many = Array.from({ length: 100 }, (_, i) => ({ ...landlords, name: `aaa_${i}` }));
  const schema = supermavenSchemaContext('select * from prop', 18, [...many, properties], 500);
  assert.ok(new TextEncoder().encode(schema).length <= 500);
  assert.ok(schema.startsWith('CREATE TABLE "public"."properties"'));
  assert.ok(schema.endsWith(');\n'));
  assert.equal(supermavenSchemaContext('', 0, items, 5), '');
});

test('both reported hallucinations and unknown/schema-mismatched tables are rejected', () => {
  assert.equal(accepts('select ', '* from "0f118d67-7c32-4745-a06c-411b66fdd8e6";'), false);
  assert.equal(accepts('select * from my', 'table where id = 1;'), false);
  assert.equal(accepts('select * from ', 'other.properties'), false);
  assert.equal(accepts('select * from ', 'landlords l join imaginary p on p.id = l.id'), false);
  assert.equal(accepts('select * from ', 'properties p, imaginary x'), false);
  assert.equal(accepts('select * from pro', 'perties where id is not null;'), true);
  assert.equal(accepts('select ', '* from public.properties;'), true);
});

test('invented qualified and bare columns are rejected, including a partial name and nested queries', () => {
  assert.equal(accepts('select p.', 'not_a_column', ' from properties p'), false);
  assert.equal(accepts('select p.', 'address', ' from properties p'), true);
  assert.equal(accepts('select * from properties p where add', 'ress is not null'), true);
  assert.equal(accepts('select * from properties p where ', 'imaginary = 1'), false);
  assert.equal(accepts('select * from properties p where ', 'x.address = 1'), false);
  assert.equal(accepts('select * from properties p where exists (', 'select 1 from imaginary x)'), false);
});

test('grounding preserves SQL literals/comments, casts, functions, aliases, CTEs and UNION scopes', () => {
  for (const sql of [
    "select id, 'from imaginary', $$join fake$$ from landlords -- from imaginary",
    'select count(*) as total from properties order by total',
    'select p.id::text from properties p where lower(p.address) is not null',
    'select extract(year from current_date) from properties',
    'with recent as (select id from properties) select r.id from recent r',
    'with recent as (select id from properties) select * from properties where exists (select 1 from recent)',
    'select p.address from properties p union all select p.name from landlords p',
    'select * from generate_series(1, 10) s',
  ]) assert.equal(accepts('', sql), true, sql);
  assert.equal(accepts('', 'select id from properties union all select imaginary from landlords'), false);
});

test('quoted catalog identifiers retain case and escaping in context and validation', () => {
  const quoted: SchemaItem = { ...landlords, schema: 'Mixed.Schema', name: 'Property"List', columns: [{ ...landlords.columns[0], name: 'DisplayName' }] };
  const schema = supermavenSchemaContext('', 0, [quoted]);
  assert.match(schema, /"Mixed.Schema"\."Property""List"/);
  const sql = 'select p."DisplayName" from "Mixed.Schema"."Property""List" p';
  assert.equal(schemaGroundedSql(sql, 0, sql.length, [quoted]), true);
  assert.equal(schemaGroundedSql(sql.replace('DisplayName', 'displayname'), 0, sql.length, [quoted]), false);
});

test('a connection change replaces schema knowledge rather than combining catalogs', () => {
  assert.equal(accepts('select * from ', 'properties'), true);
  const sql = 'select * from properties';
  assert.equal(schemaGroundedSql(sql, 0, sql.length, [landlords]), false);
  assert.doesNotMatch(supermavenSchemaContext(sql, sql.length, [landlords]), /CREATE TABLE "public"\."properties"/);
});

test('identifier suggestions stop at the real name instead of adding made-up filters and values', () => {
  for (const [prefix, suggestion, expected] of [
    ['select * from prop', "erties where status = 'active' and updated_at > '2023-01-01'", 'erties'],
    ['select * from ', "properties where id = 'made-up-id'", 'properties'],
    ['select ', "* from landlords where name = 'invented'", '* from landlords'],
    ['select * from properties p where p.', "address = 'invented'", 'address'],
    ['select * from properties p where add', "ress = 'invented'", 'ress'],
    ['select * from properties', " where id = 'made-up-id'", ''],
    ['select * from properties p where p.address', " = 'invented'", ''],
  ]) {
    const sql = prefix + suggestion;
    const end = schemaInlineIdentifierEnd(sql, prefix.length, items);
    assert.equal(sql.slice(prefix.length, end), expected, sql);
  }
  assert.equal(schemaInlineIdentifierEnd('select * from mytable', 16, items), undefined);
  assert.equal(schemaInlineIdentifierEnd('select 1 + 2', 10, items), undefined);
});
