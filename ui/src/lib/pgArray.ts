// PostgreSQL literals retain information JSON arrays cannot: lower bounds,
// exact numeric/JSON elements, SQL NULL versus the text "NULL", and dimensions.
type ArrayNode = Array<string | null | ArrayNode>;

export function arrayElementType(typeName: string): string | null {
  const type = typeName.toLowerCase().replace(/^pg_catalog\./, '').trim();
  if (type.startsWith('_')) return type.slice(1);
  return /\[\]$/.test(type) ? type.replace(/(?:\[\])+$/, '') : null;
}

export function parsePgArray(input: string) {
  let position = 0;
  const bounds: Array<{ lower: number; length: number }> = [];
  const invalid = (detail: string): never => { throw new Error(`Invalid PostgreSQL array: ${detail}`); };
  const whitespace = () => { while (/\s/.test(input[position] ?? '') && position < input.length) position += 1; };
  whitespace();
  while (input[position] === '[') {
    const match = /^\[\s*([+-]?\d+)\s*:\s*([+-]?\d+)\s*\]/.exec(input.slice(position));
    if (!match) invalid('use [lower:upper] for dimension bounds.');
    const lower = Number(match![1]), upper = Number(match![2]);
    if (![lower, upper].every((value) => Number.isInteger(value) && value >= -2147483648 && value <= 2147483647) || upper < lower) invalid('dimension bounds are out of range.');
    bounds.push({ lower, length: upper - lower + 1 });
    if (bounds.length > 6) invalid('at most six dimensions are supported.');
    position += match![0].length;
    whitespace();
  }
  if (bounds.length) {
    if (input[position++] !== '=') invalid('dimension bounds must be followed by =.');
    whitespace();
  }

  function element(): string | null {
    whitespace();
    let value = '';
    let escaped = false;
    if (input[position] === '"') {
      position += 1;
      while (position < input.length) {
        const character = input[position++];
        if (character === '"') return value;
        if (character === '\\') {
          if (position >= input.length) invalid('unfinished escape.');
          value += input[position++];
        } else value += character;
      }
      return invalid('unclosed quoted element.');
    }
    let trailingWhitespace = '';
    while (position < input.length && input[position] !== ',' && input[position] !== '}') {
      const character = input[position++];
      if (character === '\\') {
        if (position >= input.length) invalid('unfinished escape.');
        value += trailingWhitespace + input[position++];
        trailingWhitespace = '';
        escaped = true;
      } else if (character === '"' || character === '{') invalid('quote or escape special characters inside an element.');
      else if (/\s/.test(character)) trailingWhitespace += character;
      else { value += trailingWhitespace + character; trailingWhitespace = ''; }
    }
    if (!value) invalid('empty strings must be written as "".');
    return !escaped && /^null$/i.test(value) ? null : value;
  }

  function array(depth: number): ArrayNode {
    if (depth > 6) invalid('at most six dimensions are supported.');
    if (input[position++] !== '{') invalid('use braces, for example {"one","two",NULL}.');
    const values: ArrayNode = [];
    whitespace();
    if (input[position] === '}') { position += 1; return values; }
    while (position < input.length) {
      whitespace();
      values.push(input[position] === '{' ? array(depth + 1) : element());
      whitespace();
      const separator = input[position++];
      if (separator === '}') return values;
      if (separator !== ',') invalid('separate elements with commas and close each brace.');
    }
    return invalid('unclosed array.');
  }

  const values = array(1);
  whitespace();
  if (position !== input.length) invalid('unexpected text after the array.');
  function shape(values: ArrayNode): number[] {
    const nested = values.filter(Array.isArray) as ArrayNode[];
    if (!nested.length) return [values.length];
    const dimensions = shape(nested[0]);
    if (nested.length !== values.length || dimensions.includes(0) || nested.some((value) => JSON.stringify(shape(value)) !== JSON.stringify(dimensions))) invalid('multidimensional arrays must have matching dimensions.');
    return [values.length, ...dimensions];
  }
  const dimensions = shape(values);
  if (bounds.length && (bounds.length !== dimensions.length || bounds.some((bound, index) => bound.length !== dimensions[index]))) invalid('dimension bounds do not match the elements.');
  const prefix = bounds.some((bound) => bound.lower !== 1) ? `${bounds.map((bound) => `[${bound.lower}:${bound.lower + bound.length - 1}]`).join('')}=` : '';
  function format(values: ArrayNode, pretty: boolean, depth = 0): string {
    const parts = values.map((value) => Array.isArray(value) ? format(value, pretty, depth + 1) : value === null ? 'NULL' : `"${value.replaceAll('\\', '\\\\').replaceAll('"', '\\"')}"`);
    if (!pretty || !parts.length) return `{${parts.join(',')}}`;
    return `{\n${'  '.repeat(depth + 1)}${parts.join(`,\n${'  '.repeat(depth + 1)}`)}\n${'  '.repeat(depth)}}`;
  }
  return { literal: prefix + format(values, false), pretty: prefix + format(values, true), dimensions };
}
