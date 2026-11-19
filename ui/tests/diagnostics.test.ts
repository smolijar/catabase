import assert from 'node:assert/strict';
import test from 'node:test';
import { copyableReport, diagnosticBody, issueDraft, ISSUE_URL } from '../src/lib/diagnostics.ts';
import type { DiagnosticReport } from '../src/lib/diagnostics.ts';

const report: DiagnosticReport = {
  id: 'crash', newCrash: true, title: 'Native crash', body: 'Technical details only', note: '',
  query: { sql: "SELECT 'sensitive ``` value'", phase: 'running', truncated: false, startedAt: '2026-11-19T11:00:00Z', fromCrash: true }
};

test('SQL is excluded by default from both GitHub and clipboard and can be removed again', () => {
  const excluded = diagnosticBody(report, false);
  assert.equal(excluded, report.body);
  assert.doesNotMatch(copyableReport(report.title, excluded), /sensitive/);
  assert.doesNotMatch(issueDraft(report.title, excluded).body, /sensitive/);
  assert.match(diagnosticBody(report, true), /sensitive/);
  assert.equal(diagnosticBody(report, false), report.body);
  assert.equal(diagnosticBody({ ...report, query: null }, true), report.body);
});

test('SQL with Markdown fences cannot escape its code block', () => {
  const body = diagnosticBody(report, true);
  assert.match(body, /````sql\nSELECT/);
  assert.match(body, /\n````\n$/);
  assert.match(diagnosticBody({ ...report, query: { ...report.query!, truncated: true } }, true), /SQL truncated/);
  assert.match(diagnosticBody({ ...report, query: { ...report.query!, fromCrash: false } }, true), /Latest SQL in this session/);
});

test('short reports are prefilled; long encoded reports use the full clipboard fallback', () => {
  assert.equal(new URL(ISSUE_URL).pathname, '/smolijar/catabase/issues/new');
  assert.deepEqual(issueDraft('Crash', report.body), { title: 'Crash', body: report.body, copyRequired: false });
  const huge = `${report.body}\n${'🐈'.repeat(1000)}`;
  const draft = issueDraft('Crash', huge);
  assert.equal(draft.copyRequired, true);
  assert.match(draft.body, /Paste it here/);
  assert.equal(copyableReport('Crash', huge), `# Crash\n\n${huge}`);
});
