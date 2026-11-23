import assert from 'node:assert/strict';
import { test } from 'node:test';
import { activitySummary, executionSummary, formatActivityLine, formatExecutionLog } from '../src/lib/executionLog.ts';
import type { HistoryEntry } from '../src/lib/types.ts';

const entry: HistoryEntry = {
  id: 3, query: 'SELECT * FROM accounts LIMIT 201', durationMs: 578,
  rowCount: 1, isError: false, hasResult: true, executedAt: '2026-11-23T16:17:30.824Z',
  queryHash: 'query', querySize: 31, executionSize: 58, queryReferences: 1, resultReferences: 1
};

test('execution log uses only saved timestamps and includes the executed SQL', () => {
  const complete = formatExecutionLog({ ...entry, startedAt: '2026-11-23T16:17:30.246Z' }, 'Bookshop · LOCAL');
  assert.equal(complete, [
    '[2026-11-23 16:17:30.246 UTC] SQL started · Bookshop · LOCAL',
    'SELECT * FROM accounts LIMIT 201',
    '[2026-11-23 16:17:30.824 UTC] 1 row · 578 ms'
  ].join('\n'));
  assert.doesNotMatch(formatExecutionLog(entry, 'Bookshop'), /started/);
});

test('errors and transaction state appear in the copied log', () => {
  const failed = formatExecutionLog({
    ...entry, isError: true, errorMessage: 'syntax error near LIMIT',
    transaction: { id: 'example-transaction', status: 'rolledBack', kind: 'manual', statementCount: 3 }
  }, 'Bookshop');
  assert.match(failed, /Failed · syntax error near LIMIT/);
  assert.match(failed, /Transaction example-transaction · rolled back/);
});

test('an interrupted clipboard operation includes its known start and outcome', () => {
  const startedAt = '2026-11-23T16:17:30.824Z';
  const summary = activitySummary({
    id: 'operation', category: 'clipboard', title: 'Copy results as TSV',
    subject: '225 rows', startedAt, status: 'interrupted'
  });
  assert.equal(formatActivityLine(startedAt, 'clipboard', summary),
    '[2026-11-23 16:17:30.824 UTC] [Clipboard] Copy results as TSV · 225 rows · Interrupted');
});

test('SQL activity takes one line even when its SQL and error span several lines', () => {
  const failed = { ...entry, query: 'SELECT\n  bad()', isError: true, errorMessage: 'missing function\r\nCheck argument types' };
  const summary = executionSummary(failed, 'Bookshop');
  const line = formatActivityLine(failed.executedAt, 'sql', summary);
  assert.equal(line,
    '[2026-11-23 16:17:30.824 UTC] [SQL] SELECT\\n  bad() · Failed · missing function\\nCheck argument types · 578 ms · Bookshop');
  assert.match(formatExecutionLog(failed, 'Bookshop'), /SELECT\n  bad\(\)/);
});
