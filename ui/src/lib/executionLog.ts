import type { ActivityEvent, ActivityStatus } from './activity';
import type { HistoryEntry } from './types';

export const activityStatusLabels: Record<ActivityStatus, string> = {
  running: 'Running',
  completed: 'Completed',
  failed: 'Failed',
  interrupted: 'Interrupted'
};

export const activityCategoryLabels = { sql: 'SQL', clipboard: 'Clipboard', connection: 'Connection', schema: 'Schema' };

export function logTimestamp(timestamp: string): string {
  const date = new Date(timestamp);
  return Number.isFinite(date.getTime()) ? date.toISOString().replace('T', ' ').replace('Z', ' UTC') : timestamp;
}

export function statementOutcome(entry: HistoryEntry): string {
  if (entry.isError) return entry.errorMessage ? `Failed · ${entry.errorMessage}` : 'Failed';
  if (entry.rowCount == null) return 'Completed';
  return `${entry.rowCount} ${entry.rowCount === 1 ? 'row' : 'rows'}`;
}

function transactionSummary(transaction: NonNullable<HistoryEntry['transaction']>): string {
  const status = {
    active: 'open · pending commit',
    finished: 'finished',
    rolledBack: 'rolled back',
    interrupted: 'interrupted · outcome unconfirmed'
  }[transaction.status];
  return `Transaction ${transaction.id} · ${status}`;
}

export function formatExecutionLog(entry: HistoryEntry, connection: string): string {
  const start = entry.startedAt ? `[${logTimestamp(entry.startedAt)}] SQL started · ${connection}` : `[${logTimestamp(entry.executedAt)}] SQL · ${connection}`;
  const outcome = `${statementOutcome(entry)} · ${entry.durationMs} ms`;
  const lines = [start, entry.query, entry.startedAt ? `[${logTimestamp(entry.executedAt)}] ${outcome}` : outcome];
  if (entry.transaction) lines.push(transactionSummary(entry.transaction));
  return lines.join('\n');
}

function summarize(parts: (string | undefined)[]): string {
  return parts.filter(Boolean).join(' · ').replace(/\r\n?|\n|\u2028|\u2029/g, '\\n');
}

export function executionSummary(entry: HistoryEntry, connection: string): string {
  return summarize([
    entry.query, statementOutcome(entry), `${entry.durationMs} ms`, connection,
    entry.transaction ? transactionSummary(entry.transaction) : undefined
  ]);
}

export function activitySummary(event: ActivityEvent): string {
  return summarize([
    event.title, event.subject, activityStatusLabels[event.status], event.detail,
    event.elapsedMs == null ? undefined : `${event.elapsedMs} ms`
  ]);
}

export function formatActivityLine(timestamp: string, category: keyof typeof activityCategoryLabels, summary: string): string {
  return `[${logTimestamp(timestamp)}] [${activityCategoryLabels[category]}] ${summary}`;
}
