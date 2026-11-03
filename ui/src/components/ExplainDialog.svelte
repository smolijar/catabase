<script lang="ts">
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';
  import { defaultExplainOptions, explainStageLabel } from '../lib/explain';
  import type { ExplainOptions, ExplainState } from '../lib/explain';
  import { formatRunningDuration } from '../lib/status';
  export let query: string;
  export let connectionName: string;
  export let transactionActive: boolean;
  export let state: ExplainState;
  export let now: number;
  export let onStart: (options: ExplainOptions) => void;
  export let onShare: (title: string, comments: string) => void;
  export let onCancel: () => void;
  export let onClose: () => void;
  export let onOpenLink: (url: string) => Promise<unknown>;
  let options = defaultExplainOptions();
  let copyStatus = '';
  let linkError = '';
  $: elapsed = formatRunningDuration(state.stageStartedAt, now);
  async function copy(value: string, label: string) {
    try { await writeText(value).catch(() => navigator.clipboard.writeText(value)); copyStatus = `${label} copied`; }
    catch { copyStatus = `Could not copy ${label.toLowerCase()}`; }
  }
  async function openLink(url: string) {
    linkError = '';
    try { await onOpenLink(url); } catch { linkError = 'Could not open the link. Copy it to your browser instead.'; }
  }
</script>

<Modal label="Explain query" locked={state.busy} {onClose}>
  <div class="modal explain-dialog">
    <div class="panel-head"><div><h2>Explain query</h2><p class="connection">{connectionName}</p></div><button disabled={state.busy} on:click={onClose}>Close</button></div>
    <div class="explain-content">
    {#if !state.result}
      <section aria-label="Query preview"><label for="explain-query">Query</label><pre id="explain-query" class="query-preview">{query}</pre></section>
      <label for="explain-preamble">Explain preamble</label>
      <textarea id="explain-preamble" class="preamble" rows="2" spellcheck="false" bind:value={options.preamble} disabled={state.busy}></textarea>
      <label class="check-option"><input type="checkbox" bind:checked={options.useSecondResult} disabled={state.busy} />Use second result</label>
      <p class="help">{options.useSecondResult ? 'Run EXPLAIN twice and keep the second plan, after the first run has warmed the cache.' : 'Run EXPLAIN once and keep that plan.'}</p>
      <div class="effect-note">With ANALYZE, the query executes{options.useSecondResult ? ' twice' : ''}, including writes. Auto transaction is bypassed. {transactionActive ? 'This will use your open transaction.' : 'No transaction will be opened; writes can commit immediately.'}</div>
      <label class="check-option"><input type="checkbox" bind:checked={options.share} disabled={state.busy} />Share on Dalibo</label>
      <p class="help">{options.share ? 'Send the plan, SQL, title and comments to explain.dalibo.com. Anyone with the returned link can view them.' : 'Keep the plan in this dialog. Nothing is sent to Dalibo.'}</p>
    {/if}
    {#if !state.result || state.result.shareError}
      <div class="metadata-fields">
        <label for="explain-title">Title <span>(optional)</span></label><input id="explain-title" bind:value={options.title} disabled={state.busy} placeholder="" />
        <label for="explain-comments">Comments <span>(optional)</span></label><textarea id="explain-comments" rows="2" bind:value={options.comments} disabled={state.busy}></textarea>
      </div>
    {/if}
    {#if state.error}<p class="error-message" role="alert">{state.error}</p>{/if}
    {#if state.result}
      <section class="explain-complete">
        <div class="plan-summary"><strong>Plan ready</strong><span>Using run {state.result.usedRun}{#each state.result.runs as run, index} · Run {index + 1}: {run.durationMs} ms{/each}</span></div>
        {#if state.result.url}
          <div class="share-result"><span class="link-label">Dalibo link</span><a href={state.result.url} on:click|preventDefault={() => openLink(state.result!.url!)}>{state.result.url}</a><button on:click={() => copy(state.result!.url!, 'Link')}><Icon name="copy" size={14} />Copy link</button></div>
        {/if}
        {#if state.result.shareError}<p class="error-message" role="alert">The plan is ready, but sharing did not finish: {state.result.shareError}</p>{/if}
        {#if linkError}<p class="error-message" role="alert">{linkError}</p>{/if}
        <details open={!state.result.url}><summary>Plan</summary><pre class="plan-preview">{state.result.plan}</pre></details>
      </section>
    {/if}
    </div>
    {#if state.busy}
      <div class="explain-progress" role="status" aria-live="polite"><span class="spinner" aria-hidden="true"></span><strong>{explainStageLabel(state)}</strong><span class="elapsed">{elapsed}</span></div>
      <progress aria-label="Explain progress"></progress>
      {#if state.stage === 'sharing'}<p class="help">Cancelling stops waiting for the upload. Dalibo may already have received it.</p>{/if}
    {/if}
    <div class="row-actions">
      <span class="copy-status" role="status">{copyStatus}</span>
      {#if state.busy}
        <button class="danger-quiet" disabled={state.cancelling} on:click={onCancel}>{state.cancelling ? 'Cancelling…' : 'Cancel'}</button>
      {:else if state.result}
        <button on:click={() => copy(state.result!.plan, 'Plan')}><Icon name="copy" size={14} />Copy plan</button>
        {#if state.result.shareError}<button class="primary" on:click={() => onShare(options.title, options.comments)}>Retry sharing</button>{/if}
        <button on:click={onClose}>Done</button>
      {:else}
        <button on:click={onClose}>Cancel</button><button class="primary" disabled={!options.preamble.trim() || !query.trim()} on:click={() => onStart({ ...options })}><Icon name="explain" size={14} />Run explain</button>
      {/if}
    </div>
  </div>
</Modal>

<style>
  .explain-dialog { display: flex; flex-direction: column; gap: 10px; max-height: calc(100vh - 52px); }
  .explain-content { display: grid; gap: 10px; overflow: auto; min-height: 0; padding-right: 3px; }
  .panel-head, .row-actions, .explain-progress, progress { flex-shrink: 0; }
  h2 { margin: 0; }
  .connection { margin: 3px 0 0; font-size: 12px; color: var(--ink-muted); }
  label { display: block; font-size: 12px; color: var(--ink-muted); }
  label span { color: var(--ink-faint); }
  .query-preview, .plan-preview { margin: 5px 0 0; padding: 12px; background: var(--surface-1); border: 1px solid var(--line); max-height: 160px; overflow: auto; font: 12px/18px var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
  .plan-preview { max-height: 280px; }
  textarea { resize: vertical; min-height: 48px; width: 100%; }
  .preamble { font: 12px/18px var(--font-mono); }
  .check-option { display: flex; align-items: center; gap: 8px; color: var(--ink); font-size: 13px; }
  .check-option input { width: auto; margin: 0; }
  .help { margin: -5px 0 0; font-size: 11px; line-height: 16px; color: var(--ink-muted); }
  .effect-note { padding: 8px 10px; border-left: 2px solid var(--warn); background: var(--accent-soft); color: var(--ink-muted); font-size: 11px; line-height: 17px; }
  .metadata-fields { display: grid; gap: 6px; }
  .metadata-fields input { width: 100%; }
  .explain-progress { display: flex; gap: 8px; align-items: center; margin-top: 6px; font-size: 12px; }
  .elapsed { margin-left: auto; font-family: var(--font-mono); color: var(--ink-muted); }
  progress { width: 100%; height: 4px; accent-color: var(--accent); }
  .error-message { margin: 0; color: var(--danger); font-size: 12px; line-height: 18px; overflow-wrap: anywhere; }
  .explain-complete { display: grid; gap: 14px; }
  .plan-summary { display: grid; gap: 5px; }
  .plan-summary span { color: var(--ink-muted); font-size: 12px; }
  .share-result { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 12px; background: var(--surface-1); border: 1px solid var(--line); }
  .share-result .link-label { flex-basis: 100%; color: var(--ink-muted); font-size: 12px; }
  .share-result a { min-width: 0; flex: 1; overflow-wrap: anywhere; color: var(--accent); font: 12px/18px var(--font-mono); }
  details { min-width: 0; }
  summary { cursor: pointer; color: var(--ink-muted); font-size: 12px; }
  .row-actions { margin-top: 8px; }
  .copy-status { margin-right: auto; color: var(--ink-muted); font-size: 11px; }
</style>
