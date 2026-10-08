import assert from 'node:assert/strict';
import test from 'node:test';
import { sqlJoinSuggestions } from '../src/lib/sqlCompletion.ts';
import type { SchemaForeignKey, SchemaItem } from '../src/lib/types.ts';

function table(schema: string, name: string, columns: string[], foreignKeys: SchemaForeignKey[] = []): SchemaItem {
  return {
    schema, name, kind: 'table', foreignKeys,
    columns: columns.map((name) => ({ name, typeName: 'text', nullable: true, generated: false, primaryKey: false }))
  };
}

const landlordKey: SchemaForeignKey = {
  name: 'properties_landlord_id_fkey', columns: ['landlord_id'],
  referencedSchema: 'public', referencedTable: 'landlords', referencedColumns: ['id']
};
const landlords = table('public', 'landlords', ['id', 'name']);
const properties = table('public', 'properties', ['id', 'landlord_id', 'manager_id'], [landlordKey]);
const catalog = [landlords, properties];

function complete(source: string, items = catalog) {
  const cursor = source.indexOf('|');
  assert.notEqual(cursor, -1);
  return sqlJoinSuggestions(source.replace('|', ''), cursor, items);
}

function inserted(source: string, items = catalog): string[] {
  const sql = source.replace('|', '');
  return complete(source, items).map((suggestion) =>
    sql.slice(0, suggestion.replaceStart) + suggestion.insertText + sql.slice(suggestion.replaceEnd)
  );
}

test('the reported LEFT JOIN suggests ON from the real foreign key and preserves both aliases', () => {
  const source = 'select * from landlords l\nleft join properties p |';
  const suggestions = complete(source);
  assert.equal(suggestions.length, 1);
  assert.equal(suggestions[0].label, 'on p.landlord_id = l.id');
  assert.match(suggestions[0].detail, /Foreign key public.properties.*properties_landlord_id_fkey/);
  assert.deepEqual(inserted(source), ['select * from landlords l\nleft join properties p on p.landlord_id = l.id']);
});

test('reverse joins, explicit AS aliases and unaliased relations work', () => {
  assert.equal(complete('select * from properties p join landlords l |')[0].insertText, 'on l.id = p.landlord_id');
  assert.equal(complete('select * from public.landlords AS l join public.properties AS p |')[0].insertText, 'on p.landlord_id = l.id');
  assert.equal(complete('select * from landlords left join properties |')[0].insertText, 'on properties.landlord_id = landlords.id');
});

test('partial ON is replaced and a completed ON keyword is not duplicated', () => {
  for (const prefix of ['o', 'on', 'ON']) {
    assert.deepEqual(inserted(`select * from landlords l join properties p ${prefix}|`), [
      'select * from landlords l join properties p on p.landlord_id = l.id'
    ]);
  }
  assert.deepEqual(inserted('select * from landlords l join properties p ON\n  |'), [
    'select * from landlords l join properties p ON\n  p.landlord_id = l.id'
  ]);
});

test('composite foreign keys keep their ordered column pairs and include every comparison', () => {
  const parent = table('pms', 'parents', ['tenant', 'id']);
  const child = table('pms', 'children', ['parent_id', 'tenant_id'], [{
    name: 'composite_fk', columns: ['parent_id', 'tenant_id'],
    referencedSchema: 'pms', referencedTable: 'parents', referencedColumns: ['id', 'tenant']
  }]);
  assert.equal(complete('select * from pms.parents a join pms.children b |', [parent, child])[0].insertText,
    'on b.parent_id = a.id and b.tenant_id = a.tenant');
});

test('multiple foreign keys remain separate choices and duplicate predicates are deduplicated', () => {
  const multiple = { ...properties, foreignKeys: [landlordKey, { ...landlordKey }, {
    ...landlordKey, name: 'manager_fk', columns: ['manager_id']
  }] };
  assert.deepEqual(complete('select * from landlords l join properties p |', [landlords, multiple]).map((item) => item.insertText), [
    'on p.landlord_id = l.id', 'on p.manager_id = l.id'
  ]);
});

test('relationships to any earlier joined relation can be suggested', () => {
  const tenant = table('public', 'tenants', ['id']);
  const lease = table('public', 'leases', ['tenant_id'], [{
    name: 'lease_tenant_fk', columns: ['tenant_id'], referencedSchema: 'public',
    referencedTable: 'tenants', referencedColumns: ['id']
  }]);
  assert.equal(complete('select * from landlords l join tenants t on true join leases x |', [...catalog, tenant, lease])[0].insertText,
    'on x.tenant_id = t.id');
});

test('self joins expose both foreign-key directions as choices', () => {
  const employees = table('public', 'employees', ['id', 'manager_id'], [{
    name: 'manager_fk', columns: ['manager_id'], referencedSchema: 'public',
    referencedTable: 'employees', referencedColumns: ['id']
  }]);
  assert.deepEqual(complete('select * from employees e join employees m |', [employees]).map((item) => item.insertText), [
    'on m.manager_id = e.id', 'on m.id = e.manager_id'
  ]);
});

test('quoted aliases and mixed-case or reserved columns are escaped in generated SQL', () => {
  const parent = table('Mixed.Schema', 'Parents', ['order']);
  const child = table('Mixed.Schema', 'Children', ['Parent"Key'], [{
    name: 'quoted_fk', columns: ['Parent"Key'], referencedSchema: 'Mixed.Schema',
    referencedTable: 'Parents', referencedColumns: ['order']
  }]);
  assert.equal(complete('select * from "Mixed.Schema"."Parents" "L" join "Mixed.Schema"."Children" "p""q" |', [parent, child])[0].insertText,
    'on "p""q"."Parent""Key" = "L"."order"');
});

test('no foreign key means no inferred relationship, even when column names match', () => {
  assert.deepEqual(complete('select * from landlords l join properties p |', [landlords, { ...properties, foreignKeys: [] }]), []);
  assert.deepEqual(complete('select * from landlords l join properties p |', [landlords, { ...properties, foreignKeys: undefined }]), []);
  assert.deepEqual(complete('select * from landlords l join properties p |', [landlords, {
    ...properties, foreignKeys: [{ ...landlordKey, referencedSchema: 'other' }]
  }]), []);
});

test('ambiguous relations, aliases, derived tables and invalid key metadata fail closed', () => {
  const otherProperties = { ...properties, schema: 'other' };
  assert.deepEqual(complete('select * from landlords l join properties p |', [...catalog, otherProperties]), []);
  assert.deepEqual(complete('select * from landlords p join properties p |'), []);
  assert.deepEqual(complete('select * from (select * from landlords) l join properties p |'), []);
  for (const fk of [
    { ...landlordKey, columns: [] },
    { ...landlordKey, referencedColumns: [] },
    { ...landlordKey, columns: ['missing_column'] },
    { ...landlordKey, referencedColumns: ['missing_column'] }
  ]) assert.deepEqual(complete('select * from landlords l join properties p |', [landlords, { ...properties, foreignKeys: [fk] }]), []);
});

test('suggestions respect statement, subquery and UNION scopes', () => {
  assert.deepEqual(complete('select * from landlords l; select * from properties p join properties q |'), []);
  assert.deepEqual(complete('select * from landlords l\n\nselect * from properties p join properties q |'), []);
  assert.deepEqual(complete('select * from landlords l where exists (select * from properties p join properties q |)'), []);
  assert.equal(complete('select * from landlords l where exists (select * from landlords x join properties p |)')[0].insertText,
    'on p.landlord_id = x.id');
  assert.equal(complete('select * from landlords l join properties p | union all select * from properties p')[0].insertText,
    'on p.landlord_id = l.id');
});

test('ON is not suggested for CROSS/NATURAL/USING joins, existing predicates, comments or strings', () => {
  for (const source of [
    'select * from landlords l cross join properties p |',
    'select * from landlords l natural left outer join properties p |',
    'select * from landlords l join properties p using |',
    'select * from landlords l join properties p on p.|',
    'select * from landlords l join properties p on p.landlord_id = l.id |',
    'select * from landlords l join properties p | on p.landlord_id = l.id',
    'select * from landlords l join properties p on | p.landlord_id = l.id',
    'select * from landlords l join properties p -- |',
    "select 'from landlords l join properties p |'",
    'select $$from landlords l join properties p |$$',
    'select * from landlords l join properties p|'
  ]) assert.deepEqual(complete(source), [], source);
});
