import assert from 'node:assert/strict';
import test from 'node:test';
import { createExplainWorkflow, defaultExplainOptions, explainStageLabel, initialExplainState } from '../src/lib/explain.ts';
import type { ExplainRequest, ExplainResult, ShareExplainRequest } from '../src/lib/explain.ts';

const result: ExplainResult = { plan: '[{"Plan":{"Node Type":"Result"}}]', query: 'select 1', explainSql: 'EXPLAIN select 1', runs: [{ durationMs: 5 }, { durationMs: 3 }], usedRun: 2, url: 'https://explain.dalibo.com/plan/abc', shareError: null };
function harness() {
  let state = initialExplainState();
  let runResolve!: (result: ExplainResult) => void;
  let runReject!: (error: unknown) => void;
  let shareResolve!: (result: { url: string }) => void;
  let shareReject!: (error: unknown) => void;
  let now = 100;
  let serial = 0;
  const calls = { runs: [] as ExplainRequest[], shares: [] as ShareExplainRequest[], cancels: [] as string[], finishes: [] as string[] };
  const workflow = createExplainWorkflow({
    begin: () => `token-${++serial}`,
    finish: token => calls.finishes.push(token),
    run: request => { calls.runs.push(request); return new Promise((yes, no) => { runResolve = yes; runReject = no; }); },
    share: request => { calls.shares.push(request); return new Promise((yes, no) => { shareResolve = yes; shareReject = no; }); },
    cancel: async token => { calls.cancels.push(token); }
  }, next => state = next, error => (error as Error).message, () => now);
  return { workflow, calls, state: () => state, advance: () => now += 100, resolve: (value = result) => runResolve(value), reject: (error: unknown) => runReject(error), resolveShare: () => shareResolve({ url: result.url! }), rejectShare: (error: unknown) => shareReject(error) };
}
const request = () => ({ ...defaultExplainOptions(), query: 'select 1', connectionId: 'docker' });

test('Explain defaults request both runs and sharing with no metadata', () => {
  assert.deepEqual(defaultExplainOptions(), { preamble: 'EXPLAIN (ANALYZE, COSTS, VERBOSE, BUFFERS, FORMAT JSON)', useSecondResult: true, share: true, title: '', comments: '' });
});

test('one native request owns both runs, progress stays token-bound, cancellation spans stages', async () => {
  const h = harness();
  const pending = h.workflow.run(request());
  await h.workflow.run(request());
  assert.equal(h.calls.runs.length, 1);
  assert.equal(h.state().stage, 'firstRun');
  assert.match(explainStageLabel(h.state()), /first EXPLAIN \(1 of 2\)/);
  h.workflow.progress({ queryToken: 'old', stage: 'sharing', run: 2, totalRuns: 2 });
  assert.equal(h.state().stage, 'firstRun');
  h.advance();
  h.workflow.progress({ queryToken: 'token-1', stage: 'secondRun', run: 2, totalRuns: 2 });
  assert.equal(h.state().stageStartedAt, 200);
  await h.workflow.cancel();
  await h.workflow.cancel();
  assert.deepEqual(h.calls.cancels, ['token-1']);
  assert.match(explainStageLabel(h.state()), /Cancelling query/);
  h.workflow.progress({ queryToken: 'token-1', stage: 'sharing', run: 2, totalRuns: 2 });
  assert.equal(h.state().cancelling, true);
  h.resolve({ ...result, url: null, shareError: 'Upload cancelled' });
  await pending;
  assert.equal(h.state().busy, false);
  assert.equal(h.state().result?.plan, result.plan);
  assert.equal(h.state().result?.url, null);
  assert.deepEqual(h.calls.finishes, ['token-1']);
});

test('query cancellation leaves no upload or retryable plan', async () => {
  const h = harness();
  const pending = h.workflow.run(request());
  await h.workflow.cancel();
  h.reject({ message: 'Cancelled', code: '57014' });
  await pending;
  assert.equal(h.state().result, null);
  assert.match(h.state().error, /cancelled.*No plan was shared/);
  await h.workflow.retryShare('docker', '', '');
  assert.equal(h.calls.shares.length, 0);
});

test('server statement timeout retains its actual cause when the user did not cancel', async () => {
  const h = harness();
  const pending = h.workflow.run(request());
  h.reject({ code: '57014', severity: 'ERROR', message: 'canceling statement due to statement timeout' });
  await pending;
  assert.equal(h.state().error, 'canceling statement due to statement timeout');
  assert.equal(h.state().result, null);
});

test('upload-only retry preserves captured plan and never reruns SQL', async () => {
  const h = harness();
  const pending = h.workflow.run(request());
  h.resolve({ ...result, url: null, shareError: 'Offline' });
  await pending;
  const retry = h.workflow.retryShare('docker', 'Plan title', 'Comments');
  assert.deepEqual(h.calls.shares, [{ plan: result.plan, query: result.query, title: 'Plan title', comments: 'Comments', queryToken: 'token-2' }]);
  assert.equal(h.calls.runs.length, 1);
  h.rejectShare(new Error('Still offline'));
  await retry;
  assert.equal(h.state().result?.plan, result.plan);
  assert.equal(h.state().result?.shareError, 'Still offline');
  const finalRetry = h.workflow.retryShare('docker', '', '');
  h.resolveShare();
  await finalRetry;
  assert.equal(h.state().result?.url, result.url);
  assert.equal(h.state().result?.shareError, null);
  assert.equal(h.calls.runs.length, 1);
});

test('local-only single-run settings reach native execution unchanged', async () => {
  const h = harness();
  const pending = h.workflow.run({ ...request(), preamble: 'EXPLAIN (FORMAT TEXT)', useSecondResult: false, share: false });
  assert.equal(h.state().totalRuns, 1);
  assert.equal(h.calls.runs[0].share, false);
  assert.equal(h.calls.runs[0].preamble, 'EXPLAIN (FORMAT TEXT)');
  h.resolve({ ...result, runs: [{ durationMs: 4 }], usedRun: 1, url: null });
  await pending;
  assert.equal(h.state().result?.usedRun, 1);
  assert.equal(h.calls.shares.length, 0);
});

test('cancelling an upload retry retains the plan and acknowledges possible delivery', async () => {
  const h = harness();
  const pending = h.workflow.run(request());
  h.resolve({ ...result, url: null, shareError: 'Offline' });
  await pending;
  const retry = h.workflow.retryShare('docker', '', '');
  await h.workflow.cancel();
  h.rejectShare({ code: '57014', message: 'Cancelled' });
  await retry;
  assert.equal(h.state().result?.plan, result.plan);
  assert.match(h.state().result?.shareError ?? '', /Dalibo may already have received/);
  assert.equal(h.calls.runs.length, 1);
});

test('late progress and completion cannot update a disposed view', async () => {
  const h = harness();
  const pending = h.workflow.run(request());
  const previous = h.state();
  h.workflow.dispose();
  h.workflow.progress({ queryToken: 'token-1', stage: 'secondRun', run: 2, totalRuns: 2 });
  h.resolve();
  await pending;
  assert.equal(h.state(), previous);
  assert.deepEqual(h.calls.finishes, ['token-1']);
});
