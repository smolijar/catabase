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

const landlords = table('public', 'landlords', ['landlord_id', 'name']);
const properties = table('public', 'properties', ['property_id', 'landlord_id', 'address']);
const inventory = table('warehouse', 'inventory', ['stock_id', 'quantity']);
const quoted = table('Mixed.Schema', 'Property.List', ['DisplayName', 'unit_count']);
const items = [landlords, properties, inventory, quoted];

function complete(source: string, catalog = items) {
  const cursor = source.indexOf('|');
  assert.notEqual(cursor, -1, 'test SQL must identify the cursor');
  return sqlCompletionContext(source.replace('|', ''), cursor, catalog);
}

function columns(source: string, expected: SchemaItem[]) {
  assert.deepEqual(complete(source), { kind: 'qualified', tables: [], columns: expected });
}

test('the reported JOIN alias offers only properties columns at the final dot', () => {
  columns('select * from landlords l\nleft join properties p on p.|', [properties]);
  columns('select * from landlords l\nleft join properties p on l.|', [landlords]);
  columns('select * from landlords l\nleft join properties p on p.ad|', [properties]);
});

test('AS, schema-qualified relations, uppercase SQL and comma joins resolve aliases', () => {
  columns('SELECT * FROM public.landlords AS L LEFT JOIN public.properties AS P ON P.|', [properties]);
  columns('select * from landlords l, warehouse.inventory i where i.|', [inventory]);
  columns('select * from only public.properties p where p.|', [properties]);
});

test('aliases are resolved from the whole current query, including FROM after the cursor', () => {
  columns('select p.| from properties p left join landlords l on p.landlord_id = l.landlord_id', [properties]);
  columns('select p.*| from properties p', [properties]);
});

test('quoted names preserve dots, escaped quotes, case and keyword aliases', () => {
  columns('select * from "Mixed.Schema"."Property.List" as "P.Name" where "P.Name".|', [quoted]);
  columns('select * from properties as "join" where "join".|', [properties]);
  columns('select * from properties as "P" where p.|', []);
  columns('select * from properties as "P" where "P".|', [properties]);
  columns('select * from properties as "." where ".".|', [properties]);
  columns('select * from properties as "*" where "*".|', [properties]);
  const escaped = table('public', 'property"names', ['id']);
  assert.deepEqual(complete('select * from "property""names" as "p""q" where "p""q".|', [escaped]), {
    kind: 'qualified', tables: [], columns: [escaped]
  });
});

test('aliases shadow schemas and duplicate aliases fail closed', () => {
  const aliasSchema = table('p', 'unrelated', ['wrong_column']);
  assert.deepEqual(complete('select * from properties p where p.|', [...items, aliasSchema]), {
    kind: 'qualified', tables: [], columns: [properties]
  });
  columns('select * from properties p join landlords p on p.|', []);
});

test('nested query aliases shadow their parents without leaking from sibling scopes', () => {
  columns('select * from properties p where exists (select 1 from landlords p where p.|)', [landlords]);
  columns('select * from properties p where exists (select 1 from landlords l where p.|)', [properties]);
  columns('select p.| from properties p where exists (select 1 from landlords p)', [properties]);
  columns('select * from properties p where exists (select 1 from landlords l) and l.|', []);
  columns('select * from properties p where (p.|)', [properties]);
});

test('aliases do not leak between semicolon- or blank-line-separated queries', () => {
  columns('select * from landlords p;\nselect p.| from properties p;', [properties]);
  columns('select * from landlords p;\nselect p.|', []);
  columns('select * from landlords p\n\nselect * from properties p where p.|', [properties]);
  columns('select * from landlords p\n\nselect p.|', []);
  columns('select p.| from properties p\nunion all\n\nselect * from landlords l', [properties]);
  columns('select p.| from properties p union all select p.name from landlords p', [properties]);
  columns('select p.address from properties p union all select p.| from landlords p', [landlords]);
});

test('comments and SQL strings cannot introduce fake aliases or suggest inside literals', () => {
  columns("select * from properties p -- join landlords p\nwhere p.|", [properties]);
  columns("select 'from landlords p', $$from landlords p$$ from properties p where p.|", [properties]);
  columns('select * from properties /* nested /* join landlords p */ comment */ p where p.|', [properties]);
  for (const source of [
    "select * from properties p where 'p.|'",
    'select * from properties p -- p.|',
    'select * from properties p /* p.| */',
    'select $body$ join properties p on p.| $body$'
  ]) assert.deepEqual(complete(source), { kind: 'none' });
});

test('derived tables, CTEs and table functions do not guess a reflected table', () => {
  columns('select * from properties p where exists (select * from (select 1) p where p.|)', []);
  columns('with properties as (select 1) select * from properties p where p.|', []);
  columns('with properties(id) as materialized (select 1) select * from properties p where p.|', []);
  columns('select * from properties(1) p where p.|', []);
  columns('select * from properties p(renamed_id) where p.|', []);
});

test('schema and direct table completion remain available without an alias', () => {
  assert.deepEqual(complete('select * from public.|'), { kind: 'qualified', tables: [landlords, properties], columns: [] });
  columns('select public.properties.|', [properties]);
  columns('select properties.|', [properties]);
  assert.deepEqual(complete('select |'), { kind: 'unqualified' });
});
