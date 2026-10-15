import type { SchemaForeignKey, SchemaItem } from './types';

type Token = {
  kind: 'identifier' | 'symbol' | 'opaque';
  value: string;
  quoted: boolean;
  start: number;
  end: number;
  scope: number[];
};

export type SqlCompletionContext =
  | { kind: 'none' | 'unqualified' }
  | { kind: 'expression'; references: SqlReferenceSuggestion[] }
  | { kind: 'qualified'; tables: SchemaItem[]; columns: SchemaItem[] };

export interface SqlReferenceSuggestion {
  kind: 'relation' | 'column';
  label: string;
  insertText: string;
  detail: string;
}

export interface SqlJoinSuggestion {
  label: string;
  insertText: string;
  detail: string;
  replaceStart: number;
  replaceEnd: number;
}

type Relation = {
  alias: string;
  qualifier: string;
  items: SchemaItem[];
  introducer: Token;
  end: number;
  joinable: boolean;
  aliased: boolean;
  virtual: boolean;
  source?: { name: string[]; start: number; end: number };
};

const RESERVED = new Set(`select from where join left right full inner outer cross natural on using
  group order by having limit offset fetch for union intersect except all distinct as with recursive
  insert into update delete set values returning window over partition only lateral tablesample
  and or not null is in exists case when then else end asc desc nulls first last`.split(/\s+/));

const RESERVED_IDENTIFIERS = new Set([...RESERVED, ...`all analyse analyze array asymmetric authorization
  binary both cast check collate collation column concurrently constraint current_catalog current_date
  current_role current_schema current_time current_timestamp current_user default deferrable do false
  foreign freeze grant isnull leading localtime localtimestamp notnull overlaps placing primary
  references session_user some symmetric table trailing true unique user variadic verbose`.split(/\s+/)]);

function sqlIdentifier(name: string, quoted = false): string {
  return !quoted && /^[a-z_][a-z0-9_$]*$/.test(name) && !RESERVED_IDENTIFIERS.has(name)
    ? name : `"${name.replaceAll('"', '""')}"`;
}

/** Compact definitions only, never row data/default expressions/connection details. */
export function supermavenSchemaContext(sql: string, offset: number, items: SchemaItem[], budget = 96 * 1024): string {
  const { tokens } = tokenize(sql, offset);
  const names = new Set(tokens.filter((token) => token.kind === 'identifier').map((token) => token.value));
  const word = tokens.find((token) => token.kind === 'identifier' && token.start < offset && token.end >= offset);
  const partial = word ? sql.slice(word.start, offset).replace(/^"/, '') : '';
  const priority = (item: SchemaItem) => (names.has(item.name) ? 4 : 0) + (partial && item.name.startsWith(partial) ? 2 : 0);
  const ordered = [...items].sort((a, b) => priority(b) - priority(a) || `${a.schema}.${a.name}`.localeCompare(`${b.schema}.${b.name}`));
  const encoder = new TextEncoder();
  const definitions: string[] = [];
  let size = 0;
  for (const item of ordered) {
    const columns = item.columns.map((column) => `  ${sqlIdentifier(column.name, true)} ${column.typeName}${column.nullable ? '' : ' NOT NULL'}`);
    for (const key of item.rowKeys ?? []) {
      columns.push(`  ${key.primaryKey ? 'PRIMARY KEY' : 'UNIQUE'} (${key.columns.map((name) => sqlIdentifier(name, true)).join(', ')})`);
    }
    if (!item.rowKeys?.length) {
      const primary = item.columns.filter((column) => column.primaryKey);
      if (primary.length) columns.push(`  PRIMARY KEY (${primary.map((column) => sqlIdentifier(column.name, true)).join(', ')})`);
    }
    for (const fk of item.foreignKeys ?? []) columns.push(`  FOREIGN KEY (${fk.columns.map((name) => sqlIdentifier(name, true)).join(', ')}) REFERENCES ${sqlIdentifier(fk.referencedSchema, true)}.${sqlIdentifier(fk.referencedTable, true)} (${fk.referencedColumns.map((name) => sqlIdentifier(name, true)).join(', ')})`);
    const definition = `CREATE TABLE ${sqlIdentifier(item.schema, true)}.${sqlIdentifier(item.name, true)} (\n${columns.join(',\n')}\n);\n`;
    const bytes = encoder.encode(definition).length;
    if (size + bytes > budget) continue; // Never cut a name/definition midway.
    definitions.push(definition);
    size += bytes;
  }
  return definitions.join('');
}

/** A grounding check, not a SQL parser: reject provably unknown new references. */
export function schemaGroundedSql(sql: string, start: number, end: number, items: SchemaItem[]): boolean {
  const { tokens } = tokenize(sql, start);
  const added = tokens.filter((token) => token.kind === 'identifier' && token.end > start && token.start < end);
  if (added.length > 256 || tokens.length > 20000) return false;
  for (const token of added) {
    const statement = currentStatement(tokens, sql, token.start);
    const visible = statement.filter((candidate) => candidate.kind !== 'opaque' && sameScope(candidate.scope, token.scope));
    const relations = visible.some((candidate) => ['select', 'insert', 'update', 'delete'].some((word) => isWord(candidate, word)))
      ? relationDefinitions(visible, items, token.start) : [];
    const ctes = new Set<string>();
    for (let depth = 0; depth <= token.scope.length; depth++) {
      for (const name of cteNames(statement.filter((candidate) => candidate.kind !== 'opaque' && sameScope(candidate.scope, token.scope.slice(0, depth))))) ctes.add(name);
    }
    for (const relation of relations) {
      const source = relation.source;
      if (source && source.end > start && source.start < end && !relation.virtual && !relation.items.length
        && !(source.name.length === 1 && ctes.has(source.name[0]))) return false;
    }
    // Source names and aliases are declarations, not column references.
    if (relations.some((relation) => token.start > relation.introducer.start && token.end <= relation.end)) continue;
    const index = visible.indexOf(token);
    const previous = visible[index - 1];
    const next = visible[index + 1];
    if (isSymbol(next, '.') || isWord(previous, 'as') || isSymbol(next, '(')
      || (!token.quoted && RESERVED_IDENTIFIERS.has(token.value))) continue;
    const context = completionContext(sql, token.end, items, tokens, token.scope);
    if (context.kind === 'qualified') {
      // CTE/derived-table output is not fully reflected, so don't guess its columns.
      const sources = visibleRelations(statement, items, token.end, token.scope, tokens);
      const qualifier = visible[index - 2];
      if (sources.some((source) => source.alias === qualifier?.value && !source.items.length)) continue;
      if (!context.tables.some((table) => table.name === token.value)
        && !context.columns.some((table) => table.columns.some((column) => column.name === token.value))) return false;
    } else if (context.kind === 'expression') {
      const sources = visibleRelations(statement, items, token.end, token.scope, tokens);
      if (!sources.length || sources.some((source) => source.virtual || !source.items.length)) continue;
      // Function/type names and SELECT output aliases need not be catalog columns.
      if (isSymbol(previous, ':') || /\d/.test(sql[token.start - 1] ?? '')
        || (next?.kind === 'opaque' && next.start === token.end)
        || ['date', 'interval', 'timestamp', 'timestamptz', 'year', 'month', 'day', 'hour', 'minute', 'second', 'epoch'].includes(token.value)
        || visible.some((candidate, i) => isWord(visible[i - 1], 'as') && candidate.value === token.value)) continue;
      const identifier = sqlIdentifier(token.value, token.quoted);
      if (!context.references.some((reference) => reference.insertText === identifier)) return false;
    }
  }
  return true;
}

/** Name completion should finish the known identifier, not invent filters/values. */
export function schemaInlineIdentifierEnd(sql: string, cursor: number, items: SchemaItem[]): number | undefined {
  const { tokens, scope } = tokenize(sql, cursor);
  const statement = currentStatement(tokens, sql, cursor);
  const visible = statement.filter((token) => token.kind !== 'opaque' && sameScope(token.scope, scope));
  const bareSelect = isWord(visible.filter((token) => token.end <= cursor).at(-1), 'select');
  for (const relation of relationDefinitions(visible, items, cursor)) {
    const source = relation.source;
    if (source && relation.items.length && source.end >= cursor && (source.start <= cursor || bareSelect)) return source.end;
  }
  const token = visible.find((token) => token.kind === 'identifier' && token.start <= cursor && token.end >= cursor);
  if (!token) return;
  const context = completionContext(sql, token.end, items, tokens, scope);
  if (context.kind === 'qualified' && context.columns.some((table) => table.columns.some((column) => column.name === token.value))) return token.end;
  if (context.kind === 'expression' && context.references.some((reference) => reference.kind === 'column' && reference.insertText === sqlIdentifier(token.value, token.quoted))) return token.end;
}

/** Offer complete predicates only for catalog-declared foreign keys, never naming heuristics. */
export function sqlJoinSuggestions(sql: string, offset: number, items: SchemaItem[]): SqlJoinSuggestion[] {
  const cursor = Math.max(0, Math.min(offset, sql.length));
  const { tokens, scope } = tokenize(sql, cursor);
  if (tokens.some((token) => token.kind === 'opaque' && token.start < cursor && cursor <= token.end)) return [];
  const visible = currentStatement(tokens, sql, cursor)
    .filter((token) => token.kind !== 'opaque' && sameScope(token.scope, scope));
  const relations = relationDefinitions(visible, items, cursor);
  const target = relations.filter((relation) => relation.introducer.start < cursor).at(-1);
  if (!target?.joinable || target.items.length !== 1 || target.end >= cursor) return [];
  const uniqueAlias = (relation: Relation) => relations.filter((other) => other.alias === relation.alias).length === 1;
  if (!uniqueAlias(target)) return [];

  const tail = visible.filter((token) => token.start >= target.end && token.start < cursor);
  let includeOn = true;
  let replaceStart = cursor;
  let replaceEnd = cursor;
  if (!tail.length) {
    if (!/^\s+$/.test(sql.slice(target.end, cursor))) return [];
    // Do not add a second predicate ahead of an existing ON/USING clause.
    const following = visible.find((token) => token.start >= cursor);
    if (isWord(following, 'on') || isWord(following, 'using')) return [];
  } else if (tail.length === 1 && tail[0].kind === 'identifier' && !tail[0].quoted) {
    const word = sql.slice(tail[0].start, Math.min(cursor, tail[0].end)).toLowerCase();
    if (word !== 'o' && word !== 'on') return [];
    if (!/^\s+$/.test(sql.slice(target.end, tail[0].start))) return [];
    if (cursor > tail[0].end) {
      if (word !== 'on' || !/^\s+$/.test(sql.slice(tail[0].end, cursor))) return [];
      includeOn = false;
    } else {
      replaceStart = tail[0].start;
      replaceEnd = tail[0].end;
    }
    const following = visible.find((token) => token.start >= Math.max(cursor, tail[0].end));
    if (following && !['where', 'group', 'order', 'limit', 'join', 'left', 'right', 'inner', 'full'].some((word) => isWord(following, word))) return [];
  } else return [];

  const joined = target.items[0];
  const suggestions: SqlJoinSuggestion[] = [];
  const seen = new Set<string>();
  function add(owner: SchemaItem, fk: SchemaForeignKey, previous: Relation, reverse: boolean) {
    const prior = previous.items[0];
    const referenced = reverse ? joined : prior;
    if (fk.referencedSchema !== referenced.schema || fk.referencedTable !== referenced.name
      || !fk.columns.length || fk.columns.length !== fk.referencedColumns.length
      || !fk.columns.every((name) => owner.columns.some((column) => column.name === name))
      || !fk.referencedColumns.every((name) => referenced.columns.some((column) => column.name === name))) return;
    const joinedColumns = reverse ? fk.referencedColumns : fk.columns;
    const previousColumns = reverse ? fk.columns : fk.referencedColumns;
    const condition = joinedColumns.map((column, index) =>
      `${target!.qualifier}.${sqlIdentifier(column)} = ${previous.qualifier}.${sqlIdentifier(previousColumns[index])}`
    ).join(' and ');
    if (seen.has(condition)) return;
    seen.add(condition);
    suggestions.push({
      label: includeOn ? `on ${condition}` : condition,
      insertText: `${includeOn ? 'on ' : ''}${condition}`,
      detail: `Foreign key ${owner.schema}.${owner.name} · ${fk.name}`,
      replaceStart, replaceEnd
    });
  }
  for (const previous of relations.slice(0, relations.indexOf(target))) {
    if (previous.items.length !== 1 || !uniqueAlias(previous)) continue;
    for (const fk of joined.foreignKeys ?? []) add(joined, fk, previous, false);
    for (const fk of previous.items[0].foreignKeys ?? []) add(previous.items[0], fk, previous, true);
  }
  return suggestions;
}

/** Resolve the qualifier at the cursor without treating SQL literals or sibling queries as aliases. */
export function sqlCompletionContext(sql: string, offset: number, items: SchemaItem[]): SqlCompletionContext {
  const cursor = Math.max(0, Math.min(offset, sql.length));
  const { tokens, scope } = tokenize(sql, cursor);
  return completionContext(sql, cursor, items, tokens, scope);
}

function completionContext(sql: string, cursor: number, items: SchemaItem[], tokens: Token[], scope: number[]): SqlCompletionContext {
  if (tokens.some((token) => token.kind === 'opaque' && token.start < cursor && cursor <= token.end)) {
    return { kind: 'none' };
  }
  const before = tokens.filter((token) => token.start < cursor && token.kind !== 'opaque');
  const chain: string[] = [];
  let index = before.length - 1;
  if (isSymbol(before[index], '*')) index--;
  if (isSymbol(before[index], '.')) chain.unshift('');
  else if (before[index]?.kind === 'identifier') chain.unshift(before[index--].value);
  while (isSymbol(before[index], '.') && before[index - 1]?.kind === 'identifier') {
    chain.unshift(before[index - 1].value);
    index -= 2;
  }
  const statement = currentStatement(tokens, sql, cursor);
  const relations = visibleRelations(statement, items, cursor, scope, tokens);
  const expression = expressionPosition(statement, cursor, scope, relations.length > 0);
  if (chain.length < 2) return expression
    ? { kind: 'expression', references: expressionReferences(relations) }
    : { kind: 'unqualified' };

  if (chain.length >= 3) {
    const available = expression
      ? relations.filter((relation) => !relation.aliased && relation.items.length === 1).flatMap((relation) => relation.items) : items;
    return {
      kind: 'qualified', tables: [],
      columns: available.filter((item) => item.schema === chain.at(-3) && item.name === chain.at(-2))
    };
  }

  const qualifier = chain[0];
  if (expression) return {
    kind: 'qualified', tables: [],
    columns: relations.find((relation) => relation.alias === qualifier && relation.items.length === 1)?.items ?? []
  };
  // Inner query bindings shadow outer bindings, including unsupported derived tables/CTEs.
  for (let depth = scope.length; depth >= 0; depth--) {
    const visible = statement.filter((token) => token.kind !== 'opaque' && sameScope(token.scope, scope.slice(0, depth)));
    const bindings = relationBindings(visible, items, cursor);
    if (bindings.has(qualifier)) return { kind: 'qualified', tables: [], columns: bindings.get(qualifier)! };
  }
  return {
    kind: 'qualified',
    tables: items.filter((item) => item.schema === qualifier),
    columns: items.filter((item) => item.name === qualifier)
  };
}

function expressionReferences(relations: Relation[]): SqlReferenceSuggestion[] {
  const references: SqlReferenceSuggestion[] = relations.map((relation) => ({
    kind: 'relation', label: relation.qualifier, insertText: relation.qualifier,
    detail: relation.items.length === 1
      ? `${relation.aliased ? 'alias · ' : ''}${relation.items[0].schema}.${relation.items[0].name}` : 'query alias'
  }));
  const sources = relations.filter((relation) => relation.items.length === 1);
  const occurrences = new Map<string, number>();
  for (const relation of sources) {
    for (const column of relation.items[0].columns) {
      occurrences.set(column.name, (occurrences.get(column.name) ?? 0) + 1);
    }
  }
  for (const relation of sources) {
    const item = relation.items[0];
    for (const column of item.columns) {
      const name = sqlIdentifier(column.name);
      const detail = `${item.schema}.${item.name} column (${column.typeName})`;
      references.push({ kind: 'column', label: `${relation.qualifier}.${name}`, insertText: `${relation.qualifier}.${name}`, detail });
      // A bare name must resolve to one source, not cause an ambiguous-column error.
      if (occurrences.get(column.name) === 1) references.push({ kind: 'column', label: name, insertText: name, detail });
    }
  }
  return references;
}

function visibleRelations(statement: Token[], items: SchemaItem[], cursor: number, scope: number[], allTokens: Token[]): Relation[] {
  const available: Relation[] = [];
  const seen = new Set<string>();
  const openings = allTokens.filter((token) => isSymbol(token, '('));
  let parentCutoff: number | undefined;
  for (let depth = scope.length; depth >= 0; depth--) {
    const level = scope.slice(0, depth);
    const visible = statement.filter((token) => token.kind !== 'opaque' && sameScope(token.scope, level));
    const inOn = nearestClause(visible.filter((token) => token.end < cursor)) === 'on';
    const relations = relationDefinitions(visible, items, cursor)
      .filter((relation) => (!inOn || relation.introducer.start < cursor)
        && (parentCutoff === undefined || relation.end < parentCutoff));
    parentCutoff = undefined;
    for (const relation of relations) {
      if (seen.has(relation.alias)) continue;
      seen.add(relation.alias);
      if (relations.filter((other) => other.alias === relation.alias).length === 1) available.push(relation);
    }
    if (!depth) break;
    const opening = openings[scope[depth - 1] - 1];
    const parent = statement.filter((token) => token.kind !== 'opaque' && sameScope(token.scope, scope.slice(0, depth - 1)));
    const prefix = parent.filter((token) => token.start < opening.start);
    const last = prefix.at(-1);
    // CTE bodies and non-LATERAL derived tables cannot reference the outer query's aliases.
    if (isWord(prefix[0], 'with') && !prefix.some((token) => isWord(token, 'select'))
      && (isWord(last, 'as') || isWord(last, 'materialized'))) break;
    if (isWord(last, 'from') || isWord(last, 'join')
      || (isSymbol(last, ',') && nearestClause(prefix) === 'from')) break;
    if (isWord(last, 'lateral')) parentCutoff = opening.start;
  }
  return available;
}

function nearestClause(tokens: Token[]): string | undefined {
  let clause: string | undefined;
  for (const token of tokens) {
    if (token.kind === 'identifier' && !token.quoted
      && ['select', 'from', 'join', 'where', 'on', 'having', 'group', 'order', 'returning', 'set', 'values', 'limit', 'offset', 'into', 'update'].includes(token.value)) {
      clause = token.value;
    }
  }
  return clause;
}

function expressionPosition(statement: Token[], cursor: number, scope: number[], hasRelations: boolean): boolean {
  for (let depth = scope.length; depth >= 0; depth--) {
    const prefix = statement.filter((token) => token.kind !== 'opaque' && token.end < cursor
      && sameScope(token.scope, scope.slice(0, depth)));
    const clause = nearestClause(prefix);
    if (!clause) continue;
    if (clause === 'select') return hasRelations;
    return ['where', 'on', 'having', 'group', 'order', 'returning', 'set'].includes(clause);
  }
  return false;
}

function sameScope(left: number[], right: number[]): boolean {
  return left.length === right.length && left.every((id, index) => id === right[index]);
}

function isWord(token: Token | undefined, word: string): boolean {
  return token?.kind === 'identifier' && !token.quoted && token.value === word;
}

function isSymbol(token: Token | undefined, symbol: string): boolean {
  return token?.kind === 'symbol' && token.value === symbol;
}

function currentStatement(tokens: Token[], sql: string, cursor: number): Token[] {
  const boundaries = [0, sql.length + 1];
  let seenSelect = false;
  let previous: Token | undefined;
  for (const token of tokens.filter((token) => !token.scope.length && token.kind !== 'opaque')) {
    if (isSymbol(token, ';')) {
      boundaries.push(token.end);
      seenSelect = false;
    } else if ((isWord(token, 'select') || isWord(token, 'with')) && seenSelect
      && previous && /\n\s*\n/.test(sql.slice(previous.end, token.start))
      && !['union', 'intersect', 'except', 'all', 'distinct'].some((word) => isWord(previous, word))) {
      boundaries.push(token.start);
      seenSelect = false;
    }
    if (isWord(token, 'select')) seenSelect = true;
    previous = token;
  }
  const start = Math.max(...boundaries.filter((boundary) => boundary <= cursor));
  const end = Math.min(...boundaries.filter((boundary) => boundary > cursor));
  return tokens.filter((token) => token.start >= start && token.start < end);
}

function relationBindings(tokens: Token[], items: SchemaItem[], cursor: number): Map<string, SchemaItem[]> {
  const bindings = new Map<string, SchemaItem[]>();
  for (const relation of relationDefinitions(tokens, items, cursor)) {
    bindings.set(relation.alias, bindings.has(relation.alias) ? [] : relation.items);
  }
  return bindings;
}

function cteNames(tokens: Token[]): Set<string> {
  const ctes = new Set<string>();
  if (isWord(tokens[0], 'with')) {
    for (let i = 1; i < tokens.length; i++) {
      if (['select', 'insert', 'update', 'delete'].some((word) => isWord(tokens[i], word))) break;
      const afterName = isSymbol(tokens[i + 1], '(') && isSymbol(tokens[i + 2], ')') ? i + 3 : i + 1;
      if (tokens[i].kind === 'identifier' && isWord(tokens[afterName], 'as')) {
        ctes.add(tokens[i].value);
      }
    }
  }
  return ctes;
}

function relationDefinitions(tokens: Token[], items: SchemaItem[], cursor: number): Relation[] {
  const relations: Relation[] = [];
  const ctes = cteNames(tokens);
  // Each arm of a compound query has its own FROM bindings.
  const compounds = tokens.filter((token) => ['union', 'intersect', 'except'].some((word) => isWord(token, word)));
  const start = compounds.filter((token) => token.end <= cursor).at(-1)?.end ?? 0;
  const end = compounds.find((token) => token.start >= cursor)?.start ?? Infinity;
  tokens = tokens.filter((token) => token.start >= start && token.start < end);
  let inFrom = false;
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    const beginsRelation = ['from', 'join', 'update', 'into', 'using'].some((word) => isWord(token, word));
    if (isWord(token, 'from')) inFrom = true;
    else if (['where', 'on', 'group', 'order', 'having', 'limit', 'offset', 'returning', 'union', 'set'].some((word) => isWord(token, word))) inFrom = false;
    if (!beginsRelation && !(inFrom && isSymbol(token, ','))) continue;

    let next = i + 1;
    while (isWord(tokens[next], 'only') || isWord(tokens[next], 'lateral')) next++;
    const name: string[] = [];
    const sourceStart = tokens[next]?.start ?? 0;
    let tableToken: Token | undefined;
    if (tokens[next]?.kind === 'identifier') {
      tableToken = tokens[next];
      name.push(tokens[next++].value);
      while (isSymbol(tokens[next], '.') && tokens[next + 1]?.kind === 'identifier') {
        tableToken = tokens[next + 1];
        name.push(tokens[next + 1].value);
        next += 2;
      }
    }
    // An unfinished schema qualifier (FROM public.) is not a table binding.
    if (isSymbol(tokens[next], '.')) continue;
    const derived = isSymbol(tokens[next], '(');
    if (derived) {
      // Child-scope tokens are excluded, leaving only the matching outer parentheses.
      next = tokens.findIndex((candidate, index) => index > next && isSymbol(candidate, ')')) + 1;
      if (!next) continue;
    } else if (!name.length) continue;

    const table = name.at(-1);
    const schema = name.at(-2);
    const matches = derived || (name.length === 1 && ctes.has(table!)) ? []
      : items.filter((item) => item.name === table && (!schema || item.schema === schema));
    const explicitAlias = isWord(tokens[next], 'as');
    if (explicitAlias) next++;
    const alias = tokens[next];
    const hasAlias = alias?.kind === 'identifier' && (alias.quoted || !RESERVED.has(alias.value));
    const aliasName = hasAlias ? alias.value : !derived && !explicitAlias ? table : undefined;
    if (aliasName) {
      const renamedColumns = hasAlias && isSymbol(tokens[next + 1], '(');
      relations.push({
        alias: aliasName,
        qualifier: sqlIdentifier(aliasName, hasAlias ? alias.quoted : tableToken?.quoted),
        items: renamedColumns ? [] : matches,
        introducer: token,
        aliased: !!hasAlias,
        virtual: derived || (name.length === 1 && ctes.has(table!)),
        source: name.length && tableToken ? { name, start: sourceStart, end: tableToken.end } : undefined,
        end: hasAlias ? alias.end : tokens[next - 1].end,
        joinable: isWord(token, 'join') && !tokens.slice(Math.max(0, i - 3), i)
          .some((modifier) => isWord(modifier, 'cross') || isWord(modifier, 'natural'))
      });
    }
  }
  return relations;
}

function tokenize(sql: string, cursor: number): { tokens: Token[]; scope: number[] } {
  const tokens: Token[] = [];
  const scope: number[] = [];
  let cursorScope: number[] | undefined;
  let nextScope = 0;
  let i = 0;
  while (i < sql.length) {
    if (cursorScope === undefined && i >= cursor) cursorScope = [...scope];
    if (/\s/.test(sql[i])) { i++; continue; }
    const start = i;
    let kind: Token['kind'] = 'symbol';
    let value = sql[i++];
    let quoted = false;
    if (sql.startsWith('--', start)) {
      kind = 'opaque';
      const end = sql.indexOf('\n', start);
      i = end < 0 ? sql.length : end;
    } else if (sql.startsWith('/*', start)) {
      kind = 'opaque';
      i = start + 2;
      let nesting = 1;
      while (i < sql.length && nesting) {
        if (sql.startsWith('/*', i)) { nesting++; i += 2; }
        else if (sql.startsWith('*/', i)) { nesting--; i += 2; }
        else i++;
      }
    } else if (value === '"' || value === "'") {
      quoted = value === '"';
      kind = quoted ? 'identifier' : 'opaque';
      const quote = value;
      value = '';
      while (i < sql.length) {
        if (sql[i] === quote) {
          if (sql[i + 1] === quote) { value += quote; i += 2; }
          else { i++; break; }
        } else if (!quoted && sql[i] === '\\') { i += Math.min(2, sql.length - i); }
        else value += sql[i++];
      }
    } else if (value === '$' && /^\$(?:[A-Za-z_][\w]*)?\$/.test(sql.slice(start))) {
      kind = 'opaque';
      const delimiter = sql.slice(start).match(/^\$(?:[A-Za-z_][\w]*)?\$/)![0];
      const end = sql.indexOf(delimiter, start + delimiter.length);
      i = end < 0 ? sql.length : end + delimiter.length;
    } else if (/[A-Za-z_\u0080-\uffff]/.test(value)) {
      kind = 'identifier';
      while (i < sql.length && /[\w$\u0080-\uffff]/.test(sql[i])) value += sql[i++];
      value = value.toLowerCase();
    }
    if (cursorScope === undefined && cursor > start && cursor < i) cursorScope = [...scope];
    if (value === ')' && kind === 'symbol') scope.pop();
    tokens.push({ kind, value, quoted, start, end: i, scope: [...scope] });
    if (value === '(' && kind === 'symbol') scope.push(++nextScope);
  }
  return { tokens, scope: cursorScope ?? [...scope] };
}
