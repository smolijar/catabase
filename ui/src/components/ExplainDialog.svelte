<script lang="ts">
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  import Switch from './Switch.svelte';
  import ConnectionReviewSummary from './ConnectionReviewSummary.svelte';
  import StatementPreviews from './StatementPreviews.svelte';
  import { copyText } from '../lib/clipboard';
  import { defaultExplainOptions, explainStageLabel } from '../lib/explain';
  import type { ExplainOptions, ExplainState } from '../lib/explain';
  import { formatRunningDuration } from '../lib/status';
  import { literalTextAttributes } from '../lib/textInput';
  import type { ConnectionAppearance } from '../lib/appearance';
  import type { ConnectionRecord } from '../lib/types';

  export let statements: string[];
  export let connection: ConnectionRecord;
  export let appearance: ConnectionAppearance | undefined;
  export let themeKey: string;
  export let transactionActive: boolean;
  export let state: ExplainState;
  export let now: number;
  export let onStart: (options: ExplainOptions, approvedIndexes: number[]) => void;
  export let onShare: (index: number, title: string, comments: string) => void;
  export let onCancel: () => void;
  export let onClose: () => void;
  export let onOpenLink: (url: string) => Promise<unknown>;
  let options = defaultExplainOptions();
  let approvedIndexes: number[] = [];
  let selectedIndex: number | null = null;
  let copyStatus = '';
  let linkError = '';
  const statusLabels = { queued: 'Queued', running: 'Running', ready: 'Plan ready', failed: 'Failed', cancelled: 'Cancelled', skipped: 'Not run' };
  $: multiple = statements.length > 1;
  $: explainCount = state.outcomes.length || statements.length;
  $: title = multiple ? `Explain ${explainCount} ${explainCount === 1 ? 'statement' : 'statements'}` : 'Explain query';
  $: outcome = state.outcomes.find(item => item.index === (selectedIndex ?? state.activeIndex)) ?? state.outcomes[0];
  $: result = outcome?.result;
  $: configuring = !state.outcomes.length || (!multiple && !result);
  $: approved = multiple ? approvedIndexes : [0];
  $: elapsed = formatRunningDuration(state.stageStartedAt, now);

  function toggleStatement(index: number, checked: boolean) {
    approvedIndexes = checked ? [...approvedIndexes, index] : approvedIndexes.filter(item => item !== index);
  }

  function selectOutcome(index: number) {
    selectedIndex = index;
    copyStatus = '';
    linkError = '';
  }

  async function copy(value: string, label: string) {
    try { await copyText(value, `Copy ${label.toLowerCase()}`); copyStatus = `${label} copied`; }
    catch { copyStatus = `Could not copy ${label.toLowerCase()}`; }
  }
  async function openLink(url: string) {
    linkError = '';
    try { await onOpenLink(url); } catch { linkError = 'Could not open the link. Copy it to your browser instead.'; }
  }
</script>

{#snippet statementApproval(index: number)}
  <label class="statement-approval">
    <input type="checkbox" checked={approvedIndexes.includes(index)} on:change={event => toggleStatement(index, event.currentTarget.checked)} />
    Statement {index + 1}
  </label>
{/snippet}

<Modal label={title} locked={state.busy} {onClose}>
  <div class="modal explain-dialog" data-conn={appearance?.color ?? 'none'}>
    <div class="panel-head"><h2>{title}</h2><button disabled={state.busy} on:click={onClose}>Close</button></div>
    <ConnectionReviewSummary {connection} {appearance} />
    <div class="explain-content">
    {#if configuring}
      {#if multiple}<p>Select each statement you want to explain.</p>{/if}
      <StatementPreviews {statements} {themeKey} action={multiple ? statementApproval : undefined} />
      <label for="explain-preamble">Explain preamble</label>
      <textarea {...literalTextAttributes} id="explain-preamble" class="preamble" rows="2" bind:value={options.preamble} disabled={state.busy}></textarea>
      <Switch label="Use second result" bind:checked={options.useSecondResult} disabled={state.busy} />
      <p class="help">{options.useSecondResult ? 'Run EXPLAIN twice per statement and keep the second plan, after the first run has warmed the cache.' : 'Run EXPLAIN once per statement and keep that plan.'}</p>
      <div class="effect-note">With ANALYZE, each selected query executes{options.useSecondResult ? ' twice' : ''}, including writes. {transactionActive ? 'This will use your open transaction.' : 'Writes can commit immediately in any commit mode.'}</div>
      <Switch label="Share on Dalibo" bind:checked={options.share} disabled={state.busy} />
      <p class="help">{options.share ? 'Send each plan, its SQL, and the title and comments to explain.dalibo.com. Anyone with a plan’s link can view it.' : 'Keep plans locally in this dialog.'}</p>
    {/if}
    {#if multiple && state.outcomes.length}
      <div class="outcome-list" role="group" aria-label="Explain results">
        {#each state.outcomes as item (item.index)}
          <button type="button" aria-pressed={outcome?.index === item.index} on:click={event => { event.currentTarget.focus(); selectOutcome(item.index); }}>
            Statement {item.index + 1} <span class:failed={item.status === 'failed'}>{statusLabels[item.status]}</span>
          </button>
        {/each}
      </div>
      {#if outcome}<StatementPreviews statements={[outcome.query]} {themeKey} />{/if}
    {/if}
    {#if configuring || result?.shareError}
      <div class="metadata-fields">
        <label for="explain-title">Title <span>(optional)</span></label><input {...literalTextAttributes} id="explain-title" bind:value={options.title} disabled={state.busy} placeholder="" />
        <label for="explain-comments">Comments <span>(optional)</span></label><textarea {...literalTextAttributes} id="explain-comments" rows="2" bind:value={options.comments} disabled={state.busy}></textarea>
      </div>
    {/if}
    {#if state.error}<p class="error-message" role="alert">{state.error}</p>{/if}
    {#if outcome?.error}<p class="error-message" role="alert">{outcome.error}</p>{/if}
    {#if result}
      {@const plan = result}
      {#key outcome.index}
      <section class="explain-complete">
        <div class="plan-summary"><strong>Plan ready</strong><span>Using run {plan.usedRun}{#each plan.runs as run, index} · Run {index + 1}: {run.durationMs} ms{/each}</span></div>
        {#if plan.url}
          {@const url = plan.url}
          <div class="share-result"><span class="link-label">Dalibo link</span><a href={url} on:click|preventDefault={() => openLink(url)}>{url}</a><button on:click={() => copy(url, 'Link')}><Icon name="copy" size={14} />Copy link</button></div>
        {/if}
        {#if plan.shareError}<p class="error-message" role="alert">{plan.shareError}</p>{/if}
        {#if linkError}<p class="error-message" role="alert">{linkError}</p>{/if}
        <details open={!plan.url}><summary>Plan</summary><pre class="plan-preview">{plan.plan}</pre></details>
      </section>
      {/key}
    {/if}
    </div>
    {#if state.busy}
      <div class="explain-progress" role="status" aria-live="polite"><span class="spinner" aria-hidden="true"></span><strong>{#if multiple && state.activeIndex !== null}Statement {state.activeIndex + 1} · {/if}{explainStageLabel(state)}</strong><span class="elapsed">{elapsed}</span></div>
      <progress aria-label="Explain progress"></progress>
      {#if state.stage === 'sharing'}<p class="help">Cancelling stops waiting for the upload. Dalibo may already have received it.</p>{/if}
    {/if}
    <div class="row-actions">
      <span class="copy-status" role="status">{copyStatus}</span>
      {#if state.busy}
        <button class="danger-quiet" disabled={state.cancelling} on:click={onCancel}>{state.cancelling ? 'Cancelling…' : 'Cancel'}</button>
      {:else if configuring}
        <button on:click={onClose}>Cancel</button>
        <button class="primary" disabled={!options.preamble.trim() || !approved.length} on:click={() => onStart({ ...options }, approved)}>
          <Icon name="explain" size={14} />{multiple ? approved.length ? `Explain ${approved.length} ${approved.length === 1 ? 'statement' : 'statements'}` : 'Explain selected statements' : 'Run explain'}
        </button>
      {:else}
        {#if result}
          <button on:click={() => result && copy(result.plan, 'Plan')}><Icon name="copy" size={14} />Copy plan</button>
          {#if result.shareError}<button class="primary" on:click={() => onShare(outcome.index, options.title, options.comments)}>Retry sharing</button>{/if}
        {/if}
        <button on:click={onClose}>Done</button>
      {/if}
    </div>
  </div>
</Modal>

<style>
  .explain-dialog { display: flex; flex-direction: column; gap: 10px; max-height: calc(100vh - 52px); }
  .explain-content { display: grid; gap: 10px; overflow: auto; min-height: 0; padding-right: 3px; }
  .panel-head, .row-actions, .explain-progress, progress { flex-shrink: 0; }
  h2 { margin: 0; }
  label { display: block; font-size: 12px; color: var(--ink-muted); }
  label span { color: var(--ink-faint); }
  .plan-preview { margin: 5px 0 0; padding: 12px; background: var(--surface-1); border: 1px solid var(--line); max-height: 280px; overflow: auto; font: 12px/18px var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
  .statement-approval { display: flex; align-items: center; gap: 6px; color: var(--ink); line-height: 18px; cursor: pointer; }
  .statement-approval input { width: 13px; height: 13px; margin: 0; accent-color: var(--accent); }
  .outcome-list { display: flex; gap: 4px; flex-wrap: wrap; }
  .outcome-list button { padding: 3px 8px; color: var(--ink-muted); font-size: 11px; }
  .outcome-list button[aria-pressed='true'] { color: var(--ink); border-color: var(--accent); background: var(--accent-soft); }
  .outcome-list span { color: var(--ink-faint); }
  .outcome-list .failed { color: var(--danger); }
  textarea { resize: vertical; min-height: 48px; width: 100%; }
  .preamble { font: 12px/18px var(--font-mono); }
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
