import assert from 'node:assert/strict';
import test from 'node:test';
import { sqlCompletionContext } from '../src/lib/sqlCompletion.ts';
import type { SchemaItem } from '../src/lib/types.ts';

function table(schema: string, name: string, columns: string[]): SchemaItem {
  return {
    schema, name, kind: 'table',
    columns: columns.map((name) => ({ name, typeName: 'text', nullable: true, generated: false, primaryKey: false }))
  };
}

const landlords = table('public', 'landlords', ['id', 'name', 'active']);
const properties = table('public', 'properties', ['id', 'name', 'landlord_id']);
const unrelated = table('warehouse', 'inventory', ['stock_id', 'leaky_column']);
const catalog = [landlords, properties, unrelated];

function complete(source: string, items = catalog) {
  const cursor = source.indexOf('|');
  assert.notEqual(cursor, -1);
  return sqlCompletionContext(source.replace('|', ''), cursor, items);
}

function references(source: string, items = catalog) {
  const context = complete(source, items);
  assert.equal(context.kind, 'expression');
  if (context.kind !== 'expression') throw new Error('expected scoped expression suggestions');
  return context.references;
}

function labels(source: string, items = catalog): string[] {
  return references(source, items).map((reference) => reference.label);
}

test('the reported WHERE prefix offers only the available alias, table and their columns', () => {
  const source = 'select * from landlords land\nleft join properties on properties.landlord_id = land.id\nwhere l|';
  const suggestions = references(source);
  assert.deepEqual(suggestions.filter((item) => item.kind === 'relation').map((item) => item.label), ['land', 'properties']);
  assert.deepEqual(suggestions.map((item) => item.label), [
    'land', 'properties', 'land.id', 'land.name', 'land.active', 'active',
    'properties.id', 'properties.name', 'properties.landlord_id', 'landlord_id'
  ]);
  assert.ok(suggestions.every((item) => !/inventory|warehouse|leaky|SQL snippet/.test(item.detail)));
});

test('ambiguous bare column names are omitted while both qualified choices remain available', () => {
  const suggestions = labels('select * from landlords l join properties p on true where |');
  assert.ok(!suggestions.includes('id'));
  assert.ok(!suggestions.includes('name'));
  for (const label of ['l.id', 'p.id', 'l.name', 'p.name', 'active', 'landlord_id']) assert.ok(suggestions.includes(label));
});

test('the full query supplies SELECT suggestions even when FROM is after the cursor', () => {
  const suggestions = labels('select l| from landlords land join properties p on p.landlord_id = land.id');
  assert.ok(suggestions.includes('land'));
  assert.ok(suggestions.includes('land.id'));
  assert.ok(suggestions.includes('p.landlord_id'));
  assert.ok(!suggestions.includes('inventory'));
});

test('ON, HAVING, GROUP BY, ORDER BY, SET and RETURNING use expression scope', () => {
  for (const source of [
    'select * from landlords l join properties p on |',
    'select * from landlords l having |',
    'select * from landlords l group by |',
    'select * from landlords l order by |',
    'update landlords l set |',
    'update landlords l set active = true returning |',
    'select * from landlords l where (|)',
    'select * from landlords l where exists (select |)'
  ]) assert.ok(labels(source).includes('l'), source);
});

test('FROM and JOIN keep catalog-wide lookup instead of restricting new relations', () => {
  assert.deepEqual(complete('select * from |'), { kind: 'unqualified' });
  assert.deepEqual(complete('select * from landlords l join |'), { kind: 'unqualified' });
  assert.deepEqual(complete('select * from warehouse.|'), { kind: 'qualified', tables: [unrelated], columns: [] });
  assert.deepEqual(complete('select |'), { kind: 'unqualified' });
});

test('qualified expression suggestions cannot expose tables outside the query or hidden by an alias', () => {
  assert.deepEqual(complete('select * from landlords land where land.|'), { kind: 'qualified', tables: [], columns: [landlords] });
  for (const source of [
    'select * from landlords land where landlords.|',
    'select * from landlords land where inventory.|',
    'select * from landlords land where warehouse.inventory.|',
    'select * from landlords land where public.landlords.|'
  ]) assert.deepEqual(complete(source), { kind: 'qualified', tables: [], columns: [] });
  assert.deepEqual(complete('select * from public.properties where public.properties.|'), {
    kind: 'qualified', tables: [], columns: [properties]
  });
});

test('inner aliases shadow outer sources, while correlated outer references remain available', () => {
  const shadowed = labels('select * from landlords p where exists (select * from properties p where |)');
  assert.ok(shadowed.includes('p.landlord_id'));
  assert.ok(!shadowed.includes('p.active'));
  const correlated = labels('select * from landlords land where exists (select * from properties p where |)');
  assert.ok(correlated.includes('land.active'));
  assert.ok(correlated.includes('p.landlord_id'));
  const outside = labels('select * from landlords land where exists (select * from properties p) and |');
  assert.ok(outside.includes('land.active'));
  assert.ok(!outside.includes('p'));
});

test('CTEs and non-LATERAL derived tables cannot leak the outer query aliases', () => {
  const cte = labels('with c as (select * from properties p where |) select * from landlords land');
  assert.ok(cte.includes('p'));
  assert.ok(!cte.includes('land'));
  const derived = labels('select * from landlords land join (select * from properties p where |) q on true');
  assert.ok(derived.includes('p'));
  assert.ok(!derived.includes('land'));
  const lateral = labels('select * from landlords land join lateral (select * from properties p where |) q on true');
  assert.ok(lateral.includes('land'));
  assert.ok(lateral.includes('p'));
  assert.ok(!lateral.includes('q'));
});

test('ON cannot suggest relations from later joins, and DELETE USING exposes its source', () => {
  const on = labels('select * from landlords l join properties p on | join inventory i on true');
  assert.ok(on.includes('l'));
  assert.ok(on.includes('p'));
  assert.ok(!on.includes('i'));
  const lateral = labels('select * from landlords l join lateral (select 1 where |) q on true join properties p on true');
  assert.ok(lateral.includes('l'));
  assert.ok(!lateral.includes('p'));
  const using = labels('delete from landlords l using properties p where |');
  assert.ok(using.includes('l'));
  assert.ok(using.includes('p'));
});

test('statement and UNION boundaries keep references in their own query', () => {
  for (const source of [
    'select * from landlords land; select * from properties p where |',
    'select * from landlords land\n\nselect * from properties p where |',
    'select * from landlords land union all select * from properties p where |'
  ]) {
    const suggestions = labels(source);
    assert.ok(suggestions.includes('p'));
    assert.ok(!suggestions.includes('land'));
  }
});

test('quoted aliases and columns keep valid SQL escaping', () => {
  const quoted = table('Mixed.Schema', 'Property.List', ['DisplayName', 'order', 'column"name']);
  const suggestions = references('select * from "Mixed.Schema"."Property.List" as "P.Name" where |', [quoted]);
  for (const name of ['"P.Name"', '"P.Name"."DisplayName"', '"P.Name"."order"', '"P.Name"."column""name"']) {
    assert.ok(suggestions.some((item) => item.label === name && item.insertText === name));
  }
});

test('unknown sources expose only their query alias without inventing reflected columns', () => {
  for (const source of [
    'with properties as (select 1) select * from properties p where |',
    'select * from (select 1) p where |',
    'select * from properties p(renamed_id) where |'
  ]) assert.deepEqual(labels(source), ['p']);
  assert.deepEqual(labels('select * from properties p where |', [properties, { ...properties, schema: 'other' }]), ['p']);
  assert.deepEqual(complete('select * from properties p where p.|', [properties, { ...properties, schema: 'other' }]), {
    kind: 'qualified', tables: [], columns: []
  });
  assert.deepEqual(labels('select 1 where |'), []);
  assert.deepEqual(labels('select * from landlords p join properties p on true where |'), []);
});
