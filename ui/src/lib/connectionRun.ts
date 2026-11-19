import type { ConnectionRecord } from './types';

export const LAST_CONNECTION_KEY = 'catabase.lastConnectionId';

export function restoreLastConnectionId(value: string | null): string | null {
  return value && value.trim() === value && value.length <= 128 ? value : null;
}

/** A tab's saved association takes priority; list order never chooses a database. */
export function resumeConnection(connections: ConnectionRecord[], tabConnectionId: string | null, lastConnectionId: string | null, locked = false): ConnectionRecord | null {
  return connections.find(connection => connection.id === tabConnectionId)
    ?? (!locked ? connections.find(connection => connection.id === lastConnectionId) : null)
    ?? null;
}
