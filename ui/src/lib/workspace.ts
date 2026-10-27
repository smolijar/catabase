import type { editor } from 'monaco-editor';
import type { AppError, ExecutionTarget, HistoryEntry, QueryResult } from './types';

export interface ResultSession {
  data: QueryResult;
  execution: ExecutionTarget;
  connectionId: string | null;
  connectionName: string;
  executedAt: string;
  snapshot: boolean;
}

export interface PendingChange {
  rowIndex: number;
  column: string;
  oldValue: unknown;
  newValue: unknown;
}

export interface WorkspaceTab {
  id: string;
  title: string;
  kind: 'query' | 'reference';
  sql: string;
  connectionId: string | null;
  viewState: editor.ICodeEditorViewState | null;
  session: ResultSession | null;
  error: AppError | null;
  historyContext?: Omit<ResultSession, 'data'> & { durationMs: number };
  originalRows: Record<string, unknown>[];
  pendingChanges: PendingChange[];
  editMode: boolean;
}

export type ArchivedTab = Pick<WorkspaceTab, 'id' | 'title' | 'sql' | 'connectionId' | 'viewState'> & { closedAt: string };

export function archiveTab(tab: WorkspaceTab, closedAt: string): ArchivedTab | null {
  if (tab.kind !== 'query') return null;
  const { id, title, sql, connectionId, viewState } = tab;
  return { id, title, sql, connectionId, viewState: viewState ? structuredClone(viewState) : null, closedAt };
}

export function reopenArchivedTab(tab: ArchivedTab): WorkspaceTab {
  return { ...createQueryTab(tab.id, tab.title, tab.sql, tab.connectionId), viewState: tab.viewState ? structuredClone(tab.viewState) : null };
}

export function createQueryTab(id: string, title: string, sql = '', connectionId: string | null = null): WorkspaceTab {
  return { id, title, kind: 'query', sql, connectionId, viewState: null, session: null, error: null, originalRows: [], pendingChanges: [], editMode: false };
}

/** Failed executions have a saved error, not a QueryResult. Keep their context
 * without inventing an empty successful result or re-running their SQL. */
export function createHistoryTab(entry: HistoryEntry, result: QueryResult | null, connectionName: string): WorkspaceTab {
  const tab = createQueryTab(`history:${entry.id}`, `History #${entry.id}`, entry.query, entry.connectionId ?? null);
  const context = {
    execution: { sql: entry.query, start: 0, end: entry.query.length },
    connectionId: entry.connectionId ?? null, connectionName,
    executedAt: entry.executedAt, snapshot: true
  };
  return {
    ...tab, kind: 'reference',
    historyContext: { ...context, durationMs: entry.durationMs },
    session: !entry.isError && result ? { ...context, data: result } : null,
    error: entry.isError ? { message: entry.errorMessage || 'This query failed, but no error message was saved.' } : null
  };
}

// Duplicate a draft, not its database results, errors or staged writes.
// References become editable queries, just like opening their SQL in a new tab.
export function duplicateQueryTab(source: WorkspaceTab, id: string, tabs: WorkspaceTab[]): WorkspaceTab {
  const base = `${source.title} copy`;
  const titles = new Set(tabs.map((tab) => tab.title));
  let title = base;
  for (let suffix = 2; titles.has(title); suffix++) title = `${base} ${suffix}`;
  const copy = createQueryTab(id, title, source.sql, source.connectionId);
  copy.viewState = source.viewState ? structuredClone(source.viewState) : null;
  return copy;
}

export function canUseResult(session: ResultSession | null, connectionId: string | null): boolean {
  return Boolean(session && !session.snapshot && connectionId && session.connectionId === connectionId);
}

// Persist drafts and editor positions, never cached database rows or staged edits.
export function serializeWorkspace(tabs: WorkspaceTab[], activeId: string, archivedTabs: ArchivedTab[] = []): string {
  const queries = tabs.filter((tab) => tab.kind === 'query');
  const openIds = new Set(tabs.map(tab => tab.id));
  return JSON.stringify({ version: 2, activeId,
    tabs: queries.map(({ id, title, sql, connectionId, viewState }) => ({ id, title, sql, connectionId, viewState })),
    archivedTabs: archivedTabs.filter(tab => !openIds.has(tab.id)).map(({ id, title, sql, connectionId, viewState, closedAt }) => ({ id, title, sql, connectionId, viewState, closedAt }))
  });
}

// IPC saves can finish in a different order from the snapshots they contain.
// Keep the latest save behind every earlier one, while letting a later retry
// recover from an earlier failure.
export function createDraftWriter(write: (content: string) => Promise<void>): (content: string) => Promise<void> {
  let pending = Promise.resolve();
  return (content) => {
    const next = pending.catch(() => {}).then(() => write(content));
    pending = next;
    return next;
  };
}

export function restoreWorkspace(serialized: string): { tabs: WorkspaceTab[]; archivedTabs: ArchivedTab[]; activeId: string } | null {
  try {
    const stored = JSON.parse(serialized);
    if (![1, 2].includes(stored?.version) || !Array.isArray(stored.tabs)) return null;
    const ids = new Set<string>();
    const tabs: WorkspaceTab[] = [];
    const readTab = (item: any): WorkspaceTab | null => {
      if (!item || typeof item.id !== 'string' || !item.id || ids.has(item.id) || typeof item.sql !== 'string' || typeof item.title !== 'string' || item.kind === 'reference') return null;
      const tab = createQueryTab(item.id, item.title, item.sql, typeof item.connectionId === 'string' ? item.connectionId : null);
      if (item.viewState && Array.isArray(item.viewState.cursorState) && item.viewState.viewState) tab.viewState = item.viewState;
      ids.add(tab.id);
      return tab;
    };
    for (const item of stored.tabs) {
      const tab = readTab(item);
      if (tab) tabs.push(tab);
    }
    const archivedTabs: ArchivedTab[] = [];
    for (const item of Array.isArray(stored.archivedTabs) ? stored.archivedTabs : []) {
      if (typeof item?.closedAt !== 'string' || !Number.isFinite(Date.parse(item.closedAt))) continue;
      const tab = readTab(item);
      if (tab) archivedTabs.push(archiveTab(tab, item.closedAt)!);
    }
    archivedTabs.sort((a, b) => Date.parse(b.closedAt) - Date.parse(a.closedAt));
    if (!tabs.length) {
      if (stored.tabs.length && !archivedTabs.length) return null;
      // A reference-only workspace must not resurrect the legacy Playground.
      // Avoid reusing an archived tab's identity for this new blank editor.
      let id = 'playground';
      while (ids.has(id)) id += ':blank';
      tabs.push(createQueryTab(id, 'Query 1'));
    }
    return { tabs, archivedTabs, activeId: tabs.some(tab => tab.id === stored.activeId) ? stored.activeId : tabs[0].id };
  } catch {
    return null;
  }
}
