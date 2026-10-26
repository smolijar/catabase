export type ThemePreference = 'tuxedo' | 'snow' | 'system';
export type ConnectionColor = 'none' | 'red' | 'amber' | 'green' | 'blue' | 'violet';
export type ConnectionAppearance = { color: ConnectionColor; environment: string };
export const CONNECTION_COLORS: ConnectionColor[] = ['none', 'red', 'amber', 'green', 'blue', 'violet'];
export const APPEARANCE_KEY = 'catabase.connectionAppearance';

export function restoreAppearance(raw: string | null): Record<string, ConnectionAppearance> {
  try {
    const parsed: unknown = JSON.parse(raw ?? '{}');
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
    return Object.fromEntries(Object.entries(parsed).flatMap(([id, value]) => {
      if (!value || typeof value !== 'object' || !CONNECTION_COLORS.includes(value.color)) return [];
      return [[id, { color: value.color, environment: typeof value.environment === 'string' ? value.environment.slice(0, 16) : '' }]];
    }));
  } catch { return {}; }
}

export function shortType(type: string): string {
  return type.replace(/^_(\w+)$/, '$1[]').replace(/character varying/gi, 'varchar').replace(/timestamp with time zone/gi, 'timestamptz')
    .replace(/timestamp without time zone/gi, 'timestamp').replace(/boolean/gi, 'bool').replace(/double precision/gi, 'float8');
}
