import type { WorkspaceTab } from './workspace';

export type ConnectionScope = 'tab' | 'global';
export const CONNECTION_SCOPE_KEY = 'catabase.connectionScope';
export const GLOBAL_CONNECTION_KEY = 'catabase.globalConnectionId';

export function restoreConnectionScope(raw: string | null): ConnectionScope {
  return raw === 'global' ? 'global' : 'tab';
}

/** A focused lock blocks selection; global changes also leave other locks alone. */
export function connectionTargets(tabs: WorkspaceTab[], focusedId: string, scope: ConnectionScope): WorkspaceTab[] {
  const focused = tabs.find(tab => tab.id === focusedId);
  if (!focused || focused.connectionLocked) return [];
  return scope === 'global' ? tabs.filter(tab => !tab.connectionLocked) : [focused];
}

export function newTabConnection(scope: ConnectionScope, globalId: string | null, focusedId: string | null, locked: boolean): string | null {
  return scope === 'tab' ? focusedId : globalId ?? (locked ? null : focusedId);
}

/** Keep the original result visible as a previous result, without staged values. */
export function switchTabConnection(tab: WorkspaceTab, connectionId: string): Partial<WorkspaceTab> {
  return {
    connectionId, pendingChanges: [], editMode: false,
    session: tab.session && tab.pendingChanges.length && !tab.session.snapshot
      ? { ...tab.session, data: { ...tab.session.data, rows: structuredClone(tab.originalRows) } } : tab.session
  };
}
