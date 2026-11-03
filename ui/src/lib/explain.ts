export const DEFAULT_EXPLAIN_PREAMBLE = 'EXPLAIN (ANALYZE, COSTS, VERBOSE, BUFFERS, FORMAT JSON)';

export interface ExplainOptions {
  preamble: string;
  useSecondResult: boolean;
  title: string;
  comments: string;
  share: boolean;
}
export interface ExplainRequest extends ExplainOptions {
  query: string;
  connectionId: string;
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
export interface ExplainState {
  token: string | null;
  busy: boolean;
  cancelling: boolean;
  stage: ExplainProgress['stage'] | null;
  stageStartedAt: number | null;
  totalRuns: number;
  result: ExplainResult | null;
  error: string;
}
export const initialExplainState = (): ExplainState => ({ token: null, busy: false, cancelling: false, stage: null, stageStartedAt: null, totalRuns: 2, result: null, error: '' });
export const defaultExplainOptions = (): ExplainOptions => ({ preamble: DEFAULT_EXPLAIN_PREAMBLE, useSecondResult: true, title: '', comments: '', share: true });

export function explainStageLabel(state: ExplainState): string {
  if (state.cancelling) return state.stage === 'sharing' ? 'Cancelling sharing…' : 'Cancelling query…';
  if (state.stage === 'sharing') return 'Sharing on Dalibo…';
  return `Running ${state.stage === 'secondRun' ? 'second' : 'first'} EXPLAIN (${state.stage === 'secondRun' ? 2 : 1} of ${state.totalRuns})…`;
}

/** The native command owns both SQL executions and a single cancellation token.
 * Retrying a failed upload uses the captured plan; it never executes SQL again. */
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
  async function perform(connectionId: string, uploadOnly: boolean, work: (token: string) => Promise<ExplainResult>) {
    if (state.busy || disposed) return;
    const token = deps.begin(connectionId);
    set({ token, busy: true, cancelling: false, error: '', stage: uploadOnly ? 'sharing' : 'firstRun', stageStartedAt: now(), result: uploadOnly ? state.result : null });
    try {
      const result = await work(token);
      if (state.token === token) set({ result });
    } catch (error) {
      if (state.token !== token) return;
      const cancelled = state.cancelling && (error as { code?: string })?.code === '57014';
      const message = cancelled ? (uploadOnly ? 'Sharing cancelled. Dalibo may already have received the plan. The plan is still available below.' : 'Explain cancelled. No plan was shared.') : describe(error);
      if (uploadOnly && state.result) set({ result: { ...state.result, shareError: message } });
      else set({ error: message });
    } finally {
      if (state.token === token) set({ busy: false, cancelling: false, token: null, stage: null, stageStartedAt: null });
      deps.finish(token);
    }
  }
  return {
    reset() { if (!state.busy) { state = initialExplainState(); changed(state); } },
    run(request: Omit<ExplainRequest, 'queryToken'>) {
      if (state.busy || disposed) return Promise.resolve();
      set({ totalRuns: request.useSecondResult ? 2 : 1 });
      return perform(request.connectionId, false, token => deps.run({ ...request, queryToken: token }));
    },
    retryShare(connectionId: string, title: string, comments: string) {
      const result = state.result;
      if (!result || state.busy || disposed) return Promise.resolve();
      return perform(connectionId, true, async token => {
        const shared = await deps.share({ plan: result.plan, query: result.query, title, comments, queryToken: token });
        return { ...result, url: shared.url, shareError: null };
      });
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
