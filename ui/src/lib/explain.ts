import { queryLostConnection } from './connectionTest.ts';

export const DEFAULT_EXPLAIN_PREAMBLE = 'EXPLAIN (ANALYZE, COSTS, VERBOSE, BUFFERS, FORMAT JSON)';

export interface ExplainOptions {
  preamble: string;
  useSecondResult: boolean;
  title: string;
  comments: string;
  share: boolean;
}
export interface ExplainRunOptions extends ExplainOptions {
  connectionId: string;
  transaction?: { id: string | null };
}
export interface ExplainRequest extends ExplainRunOptions {
  query: string;
  queryToken: string;
}
export interface ExplainResult {
  plan: string;
  query: string;
  explainSql: string;
  runs: Array<{ durationMs: number }>;
  usedRun: number;
  url: string | null;
  shareError: string | null;
}
export interface ShareExplainRequest {
  plan: string;
  query: string;
  title: string;
  comments: string;
  queryToken: string;
}
export interface ExplainProgress {
  queryToken: string;
  stage: 'firstRun' | 'secondRun' | 'sharing';
  run: number;
  totalRuns: number;
}
export interface ExplainStatement {
  index: number;
  query: string;
}
export interface ExplainOutcome extends ExplainStatement {
  status: 'queued' | 'running' | 'ready' | 'failed' | 'cancelled' | 'skipped';
  result: ExplainResult | null;
  error: string;
}
export interface ExplainState {
  token: string | null;
  busy: boolean;
  cancelling: boolean;
  stage: ExplainProgress['stage'] | null;
  stageStartedAt: number | null;
  totalRuns: number;
  activeIndex: number | null;
  outcomes: ExplainOutcome[];
  error: string;
}
export const initialExplainState = (): ExplainState => ({ token: null, busy: false, cancelling: false, stage: null, stageStartedAt: null, totalRuns: 2, activeIndex: null, outcomes: [], error: '' });
export const defaultExplainOptions = (): ExplainOptions => ({ preamble: DEFAULT_EXPLAIN_PREAMBLE, useSecondResult: true, title: '', comments: '', share: true });

export function explainStageLabel(state: ExplainState): string {
  if (state.cancelling) return state.stage === 'sharing' ? 'Cancelling sharing…' : 'Cancelling query…';
  if (state.stage === 'sharing') return 'Sharing on Dalibo…';
  return `Running ${state.stage === 'secondRun' ? 'second' : 'first'} EXPLAIN (${state.stage === 'secondRun' ? 2 : 1} of ${state.totalRuns})…`;
}

/** Run approved statements in order, each through the native single-query command.
 * Keep the workspace busy across the sequence and only cancel the active request. */
export function createExplainWorkflow(deps: {
  begin: (connectionId: string) => string;
  finish: (token: string) => void;
  run: (request: ExplainRequest) => Promise<ExplainResult>;
  share: (request: ShareExplainRequest) => Promise<{ url: string }>;
  cancel: (token: string) => Promise<unknown>;
}, changed: (state: ExplainState) => void, describe: (error: unknown) => string, now = Date.now) {
  let state = initialExplainState();
  let disposed = false;
  function set(patch: Partial<ExplainState>) {
    if (disposed) return;
    state = { ...state, ...patch };
    changed(state);
  }
  function updateOutcome(index: number, patch: Partial<ExplainOutcome>) {
    set({ outcomes: state.outcomes.map(outcome => outcome.index === index ? { ...outcome, ...patch } : outcome) });
  }
  function finish(token: string) {
    set({ busy: false, cancelling: false, token: null, stage: null, stageStartedAt: null, activeIndex: null });
    deps.finish(token);
  }
  return {
    reset() { if (!state.busy) { state = initialExplainState(); changed(state); } },
    async run(options: ExplainRunOptions, statements: ExplainStatement[]) {
      if (state.busy || disposed || !statements.length) return;
      const token = deps.begin(options.connectionId);
      set({ busy: true, error: '', totalRuns: options.useSecondResult ? 2 : 1,
        outcomes: statements.map(statement => ({ ...statement, status: 'queued', result: null, error: '' })) });
      try {
        for (const { index, query } of statements) {
          if (state.cancelling || disposed) break;
          const queryToken = statements.length > 1 ? `${token}:${index}` : token;
          set({ token: queryToken, activeIndex: index, stage: 'firstRun', stageStartedAt: now() });
          updateOutcome(index, { status: 'running' });
          try {
            const result = await deps.run({ ...options, query, queryToken });
            updateOutcome(index, { status: 'ready', result });
          } catch (error) {
            const code = (error as { code?: string })?.code;
            const cancelled = state.cancelling && code === '57014';
            updateOutcome(index, { status: cancelled ? 'cancelled' : 'failed', error: cancelled ? 'Explain cancelled. No plan was shared.' : describe(error) });
            if (code === 'CATABASE_EXPLAIN_CONTEXT' || queryLostConnection(error)) break;
          }
        }
      } finally {
        set({ outcomes: state.outcomes.map(outcome => outcome.status === 'queued' ? { ...outcome, status: 'skipped' } : outcome) });
        finish(token);
      }
    },
    async retryShare(connectionId: string, index: number, title: string, comments: string) {
      const result = state.outcomes.find(outcome => outcome.index === index)?.result;
      if (!result || state.busy || disposed) return;
      const token = deps.begin(connectionId);
      set({ token, busy: true, error: '', activeIndex: index, stage: 'sharing', stageStartedAt: now() });
      try {
        const shared = await deps.share({ plan: result.plan, query: result.query, title, comments, queryToken: token });
        updateOutcome(index, { result: { ...result, url: shared.url, shareError: null } });
      } catch (error) {
        const cancelled = state.cancelling && (error as { code?: string })?.code === '57014';
        const shareError = cancelled ? 'Sharing cancelled. Dalibo may already have received the plan. The plan is still available below.' : describe(error);
        updateOutcome(index, { result: { ...result, shareError } });
      } finally {
        finish(token);
      }
    },
    progress(progress: ExplainProgress) {
      if (!state.busy || progress.queryToken !== state.token) return;
      const changedStage = state.stage !== progress.stage;
      set({ stage: progress.stage, stageStartedAt: changedStage ? now() : state.stageStartedAt, totalRuns: progress.totalRuns });
    },
    async cancel() {
      const token = state.token;
      if (!state.busy || state.cancelling || !token) return;
      set({ cancelling: true });
      try { await deps.cancel(token); }
      catch (error) { if (state.token === token) set({ cancelling: false, error: `Cancellation failed: ${describe(error)}` }); }
    },
    dispose() { disposed = true; }
  };
}
