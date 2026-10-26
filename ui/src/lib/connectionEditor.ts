import type { ConnectionInput } from './types';

/** Display-only changes must not reconnect or invalidate results/staged edits. */
export function sameConnectionSettings(saved: ConnectionInput, draft: ConnectionInput): boolean {
  return saved.id === draft.id && saved.name === draft.name && saved.host === draft.host
    && saved.port === draft.port && saved.database === draft.database
    && saved.username === draft.username && (saved.password ?? '') === (draft.password ?? '')
    && saved.ssl === draft.ssl;
}
