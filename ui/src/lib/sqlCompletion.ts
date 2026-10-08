import type { SchemaItem } from './types';

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
  | { kind: 'qualified'; tables: SchemaItem[]; columns: SchemaItem[] };

const RESERVED = new Set(`select from where join left right full inner outer cross natural on using
  group order by having limit offset fetch for union intersect except all distinct as with recursive
  insert into update delete set values returning window over partition only lateral tablesample
  and or not null is in exists case when then else end asc desc nulls first last`.split(/\s+/));

/** Resolve the qualifier at the cursor without treating SQL literals or sibling queries as aliases. */
export function sqlCompletionContext(sql: string, offset: number, items: SchemaItem[]): SqlCompletionContext {
  const cursor = Math.max(0, Math.min(offset, sql.length));
  const { tokens, scope } = tokenize(sql, cursor);
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
  if (chain.length < 2) return { kind: 'unqualified' };

  if (chain.length >= 3) {
    return {
      kind: 'qualified', tables: [],
      columns: items.filter((item) => item.schema === chain.at(-3) && item.name === chain.at(-2))
    };
  }

  const qualifier = chain[0];
  const statement = currentStatement(tokens, sql, cursor);
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
  // Each arm of a compound query has its own FROM bindings.
  const compounds = tokens.filter((token) => ['union', 'intersect', 'except'].some((word) => isWord(token, word)));
  const start = compounds.filter((token) => token.end <= cursor).at(-1)?.end ?? 0;
  const end = compounds.find((token) => token.start >= cursor)?.start ?? Infinity;
  tokens = tokens.filter((token) => token.start >= start && token.start < end);
  let inFrom = false;
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    const beginsRelation = ['from', 'join', 'update', 'into'].some((word) => isWord(token, word));
    if (isWord(token, 'from')) inFrom = true;
    else if (['where', 'on', 'group', 'order', 'having', 'limit', 'offset', 'returning', 'union', 'set'].some((word) => isWord(token, word))) inFrom = false;
    if (!beginsRelation && !(inFrom && isSymbol(token, ','))) continue;

    let next = i + 1;
    while (isWord(tokens[next], 'only') || isWord(tokens[next], 'lateral')) next++;
    const name: string[] = [];
    if (tokens[next]?.kind === 'identifier') {
      name.push(tokens[next++].value);
      while (isSymbol(tokens[next], '.') && tokens[next + 1]?.kind === 'identifier') {
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
    const aliasName = alias?.kind === 'identifier' && (alias.quoted || !RESERVED.has(alias.value))
      ? alias.value : !derived && !explicitAlias ? table : undefined;
    if (aliasName) {
      // An ambiguous alias must not offer columns from an arbitrary relation.
      const renamedColumns = alias?.kind === 'identifier' && isSymbol(tokens[next + 1], '(');
      bindings.set(aliasName, bindings.has(aliasName) || renamedColumns ? [] : matches);
    }
  }
  return bindings;
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
