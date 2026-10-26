import type { ConnectionInput } from './types';
import { connectionOptions } from './connectionOptions.ts';

/** Display-only changes must not reconnect or invalidate results/staged edits. */
export function sameConnectionSettings(saved: ConnectionInput, draft: ConnectionInput): boolean {
  return saved.id === draft.id && saved.name === draft.name && saved.host === draft.host
    && saved.port === draft.port && saved.database === draft.database
    && saved.username === draft.username && (saved.password ?? '') === (draft.password ?? '')
    && saved.ssl === draft.ssl
    && Object.entries(connectionOptions(saved.options)).every(([key, value]) => value === connectionOptions(draft.options)[key as keyof ReturnType<typeof connectionOptions>]);
}
