import type { HistoryEntry, HistoryPage, HistoryTransaction } from './types';

export interface HistoryGroup {
  key: string;
  transaction: HistoryTransaction | null;
  entries: HistoryEntry[];
}

/** Preserve newest-first group order even when another connection's statements
 * interleave. Groups spanning history pages are still represented only once. */
export function groupHistory(entries: HistoryEntry[]): HistoryGroup[] {
  const groups = new Map<string, HistoryGroup>();
  for (const entry of entries) {
    const key = entry.transaction ? `transaction:${entry.transaction.id}` : `entry:${entry.id}`;
    let group = groups.get(key);
    if (!group) {
      group = { key, transaction: entry.transaction ?? null, entries: [] };
      groups.set(key, group);
    }
    if (!group.entries.some(item => item.id === entry.id)) group.entries.push(entry);
  }
  return [...groups.values()];
}

export function transactionLabel(transaction: HistoryTransaction): string {
  return transaction.kind === 'review' ? 'Reviewed edits' : transaction.kind === 'batch' ? 'SQL batch' : 'Transaction';
}

export function transactionStatus(transaction: HistoryTransaction): string {
  return ({ active: 'Active', finished: 'Finished', rolledBack: 'Rolled back', interrupted: 'Interrupted · outcome unconfirmed' })[transaction.status];
}

/** Decimal storage units, matching the KB/MB/GB labels rather than KiB/MiB. */
export function formatBytes(bytes: number): string {
  const size = Number.isFinite(bytes) ? Math.max(0, bytes) : 0;
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const unit = size ? Math.max(0, Math.min(units.length - 1, Math.floor(Math.log(size) / Math.log(1000)))) : 0;
  return `${unit ? (size / 1000 ** unit).toFixed(1) : Math.round(size)} ${units[unit]}`;
}

export function historyEntryBytes(entry: HistoryEntry): number {
  return entry.querySize + (entry.resultSize ?? 0) + entry.executionSize;
}

export interface HistorySearchState extends HistoryPage {
  loading: boolean;
  error: string;
}

/** Search the stored timeline, not the sidebar's first page. Invalidate old
 * matches immediately and reject responses arriving after another search/close. */
export function createHistorySearch(
  fetch: (search: string) => Promise<HistoryPage>,
  changed: (state: HistorySearchState) => void,
  describeError: (error: unknown) => string,
  delay = 200
) {
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  function cancel() {
    generation++;
    clearTimeout(timer);
  }
  function search(text: string) {
    cancel();
    const request = generation;
    changed({ entries: [], total: 0, loading: true, error: '' });
    timer = setTimeout(async () => {
      try {
        const page = await fetch(text.trim());
        if (request === generation) changed({ ...page, loading: false, error: '' });
      } catch (error) {
        if (request === generation) changed({ entries: [], total: 0, loading: false, error: describeError(error) });
      }
    }, delay);
  }
  return { search, cancel };
}
