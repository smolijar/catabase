import type { QueryResult } from './types';

export interface ClientQueryTimings {
  targetMs: number;
  invokeMs: number;
  uiMs: number;
  totalMs: number;
}

export interface QueryTimingDetails {
  result: QueryResult;
  client: ClientQueryTimings;
  connectionName: string;
  executedAt: string;
  /** Paging measures the new batch, not previously loaded rows. */
  batch: boolean;
}

export interface TimingRow {
  label: string;
  ms: number;
  note: string;
  estimated?: boolean;
}

export function timingRows(result: QueryResult, client: ClientQueryTimings): TimingRow[] {
  const rows: TimingRow[] = [
    { label: 'Choose SQL', ms: client.targetMs, note: 'Resolve the selection or statement at the cursor.' }
  ];
  const timing = result.timings;
  if (!timing) {
    rows.push({ label: 'Query', ms: result.durationMs, note: 'Detailed backend timings are unavailable for this result.' });
    rows.push({ label: 'Other request work', ms: Math.max(0, client.invokeMs - result.durationMs), note: 'Connection, history, serialization and IPC combined.', estimated: true });
  } else {
    const command = timing.command;
    if (command) rows.push({ label: 'Connection & setup', ms: command.setupMs, note: 'Connection lookup, pool checkout/recycling, transaction routing and cancellation bookkeeping.' });
    rows.push({ label: 'Prepare SQL', ms: timing.prepareMs, note: 'Prepare the statement and obtain result-column types; includes a database round trip.' });
    const metadataNote = timing.metadataSource === 'refreshed'
      ? `Reflection refreshed · ${timing.metadataQueries} metadata queries. Cache entries expire after 60 seconds or are invalidated by schema refreshes, non-read-only SQL, or transaction completion.`
      : timing.metadataSource === 'cache'
        ? `Reflection cache hit · ${timing.metadataQueries} metadata queries. Includes any wait for a concurrent cache refresh.`
        : 'No source-table metadata needed for this result.';
    rows.push({ label: 'Result metadata', ms: timing.metadataMs, note: metadataNote });
    rows.push({ label: 'Execute & fetch', ms: timing.executeFetchMs, note: 'PostgreSQL execution/output serialization plus network transfer of all fetched rows. These cannot be separated by client timestamps.' });
    rows.push({ label: 'Decode rows', ms: timing.decodeMs, note: 'Convert PostgreSQL values into application result rows.' });
    if (command) {
      rows.push({ label: 'Save history', ms: command.historyMs, note: 'Serialize the result snapshot and write query history.' });
      rows.push({ label: 'Response & IPC', ms: Math.max(0, client.invokeMs - command.totalMs), note: 'Estimated residual: response serialization, bridge transfer/deserialization and scheduling.', estimated: true });
    } else {
      rows.push({ label: 'Other request work', ms: Math.max(0, client.invokeMs - timing.totalMs), note: 'Connection, history, serialization and IPC combined.', estimated: true });
    }
  }
  rows.push({ label: 'Result UI update', ms: client.uiMs, note: 'Update result state and flush DOM changes. Browser paint is not included.' });
  return rows;
}

export function formatTiming(ms: number): string {
  return `${Math.max(0, Number.isFinite(ms) ? ms : 0).toFixed(1)} ms`;
}
