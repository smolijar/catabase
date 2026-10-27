import type { ConnectionInput } from './types';

export const PASSWORD_MASK = '••••••';

/** Never put the actual password into the editable URL or send it for parsing. */
export function displayConnectionUrl(connection: ConnectionInput): string {
  const host = connection.host.includes(':') && !connection.host.startsWith('[') ? `[${connection.host}]` : connection.host;
  return `postgresql://${encodeURIComponent(connection.username)}${connection.password ? `:${PASSWORD_MASK}` : ''}@${host}:${connection.port ?? ''}/${encodeURIComponent(connection.database)}?sslmode=${connection.ssl ? 'require' : 'disable'}`;
}

export function redactConnectionUrl(value: string, hasPassword: boolean): string {
  // Work on text, not URL(), so even malformed/pasted URLs are redacted before
  // validation. The last @ also covers unescaped @ characters in a password.
  return value.replace(/([a-z][a-z\d+.-]*:\/\/)([^/?#]*)/i, (_, scheme: string, authority: string) => {
    const at = authority.lastIndexOf('@');
    if (at < 0) return scheme + authority;
    const user = authority.slice(0, at).split(':', 1)[0];
    return `${scheme}${user}${hasPassword ? `:${PASSWORD_MASK}` : ''}${authority.slice(at)}`;
  }).replace(/([?&]password=)[^&#]*/gi, `$1${PASSWORD_MASK}`);
}

/** URL edits change address/SSL fields only, never identity, options or password. */
export function applyConnectionUrl(value: string, connection: ConnectionInput): ConnectionInput {
  let url: URL;
  try { url = new URL(redactConnectionUrl(value.trim(), false)); }
  catch { throw new Error('Enter a valid PostgreSQL URL.'); }
  if (!['postgres:', 'postgresql:'].includes(url.protocol)) throw new Error('Use postgres:// or postgresql://.');
  if (!url.hostname) throw new Error('The connection URL needs a host.');
  if (!url.username) throw new Error('The connection URL needs a username.');
  if (!url.pathname.replace(/^\/+/, '')) throw new Error('The connection URL needs a database.');
  const port = Number(url.port || 5432);
  if (port < 1 || port > 65535) throw new Error('The URL port must be between 1 and 65535.');
  let username: string, database: string;
  try { username = decodeURIComponent(url.username); database = decodeURIComponent(url.pathname.replace(/^\/+/, '')); }
  catch { throw new Error('The connection URL has invalid percent-encoding.'); }
  const sslmode = [...url.searchParams].find(([key]) => key.toLowerCase() === 'sslmode')?.[1];
  return { ...connection, host: url.hostname, port, username, database, ssl: ['require', 'verify-ca', 'verify-full'].includes(sslmode ?? '') };
}

/** Keep the masked credential segment read-only, while allowing whole-URL replacement. */
export function editsMaskedPassword(value: string, start: number, end: number, inputType: string): boolean {
  if (start === 0 && end === value.length) return false;
  const mask = value.indexOf(`:${PASSWORD_MASK}@`);
  if (mask < 0) return false;
  const limit = mask + PASSWORD_MASK.length + 2;
  if (start === end) {
    if (inputType.startsWith('delete') && inputType.endsWith('Backward')) start--;
    else if (inputType.startsWith('delete') && inputType.endsWith('Forward')) end++;
    else return start > mask && start < limit;
  }
  return start < limit && end > mask;
}
