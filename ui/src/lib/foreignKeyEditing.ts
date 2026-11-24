import type { ColumnMeta, ForeignKeyLookup, ForeignKeyValues, SchemaForeignKey, SchemaItem } from './types';

export interface ForeignKeyTarget {
  schema: string;
  table: string;
  key: SchemaForeignKey;
  mappings: { column: ColumnMeta; referencedColumn: string }[];
  unavailableReason: string;
}

export function foreignKeyTargets(column: ColumnMeta | undefined, columns: ColumnMeta[], schema: SchemaItem[], editable: string[]): ForeignKeyTarget[] {
  if (!column?.sourceName || !column.tableSchema || !column.tableName) return [];
  const source = schema.find((item) => item.schema === column.tableSchema && item.name === column.tableName);
  return (source?.foreignKeys ?? []).filter((key) => key.columns.includes(column.sourceName!)
    && key.columns.length > 0 && key.columns.length === key.referencedColumns.length
    && new Set(key.columns).size === key.columns.length && new Set(key.referencedColumns).size === key.referencedColumns.length)
    .map((key) => {
      const mappings: ForeignKeyTarget['mappings'] = [];
      const missing: string[] = [];
      key.columns.forEach((name, index) => {
        const matches = columns.filter((candidate) => candidate.tableSchema === column.tableSchema && candidate.tableName === column.tableName && candidate.sourceName === name);
        if (matches.length !== 1 || !editable.includes(matches[0].name)) missing.push(name);
        else mappings.push({ column: matches[0], referencedColumn: key.referencedColumns[index] });
      });
      return { schema: column.tableSchema!, table: column.tableName!, key, mappings,
        unavailableReason: missing.length ? `Select every editable component of this foreign key to use the picker. Missing: ${missing.join(', ')}.` : '' };
    });
}

export function foreignKeyValuesMatch(target: ForeignKeyTarget, values: ForeignKeyValues): boolean {
  const left = target.key;
  const right = values.foreignKey;
  return left.name === right.name && left.referencedSchema === right.referencedSchema && left.referencedTable === right.referencedTable
    && JSON.stringify(left.columns) === JSON.stringify(right.columns)
    && JSON.stringify(left.referencedColumns) === JSON.stringify(right.referencedColumns);
}

// Map all components to their projected names, including SELECT aliases.
// Keep exact server text until the normal typed cell parser validates the draft.
export function foreignKeyDraft(target: ForeignKeyTarget, option: Record<string, string | null>): Record<string, string> {
  if (target.unavailableReason || target.mappings.length !== target.key.columns.length) throw new Error(target.unavailableReason || 'The foreign key mapping is incomplete.');
  return Object.fromEntries(target.mappings.map(({ column, referencedColumn }) => {
    if (!Object.hasOwn(option, referencedColumn) || typeof option[referencedColumn] !== 'string') throw new Error('This referenced row has an incomplete key.');
    return [column.name, option[referencedColumn]!];
  }));
}

export interface ForeignKeyLookupState { loading: boolean; data: ForeignKeyValues | null; error: string }

export function createForeignKeyLookup(
  load: (request: ForeignKeyLookup, token: string) => Promise<ForeignKeyValues>,
  update: (state: ForeignKeyLookupState) => void,
  cancel: (token: string) => void,
  tokenFactory = () => crypto.randomUUID() as string
) {
  let revision = 0;
  let activeToken: string | null = null;
  function invalidate() {
    revision++;
    const token = activeToken;
    activeToken = null;
    if (token) cancel(token);
  }
  async function run(request: ForeignKeyLookup) {
    invalidate();
    const ownRevision = revision;
    const token = tokenFactory();
    activeToken = token;
    update({ loading: true, data: null, error: '' });
    try {
      const data = await load(request, token);
      if (revision === ownRevision) update({ loading: false, data, error: '' });
    } catch (error) {
      if (revision === ownRevision) update({ loading: false, data: null, error: error && typeof error === 'object' && 'message' in error ? String(error.message) : String(error) });
    } finally {
      if (revision === ownRevision) activeToken = null;
    }
  }
  return { run, invalidate };
}
