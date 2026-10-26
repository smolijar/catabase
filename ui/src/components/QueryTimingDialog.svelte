<script lang="ts">
  import Modal from './Modal.svelte';
  import { formatTiming, timingRows } from '../lib/queryTiming';
  import type { QueryTimingDetails } from '../lib/queryTiming';
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';
  import Icon from './Icon.svelte';
  export let details: QueryTimingDetails;
  export let onClose: () => void;
  $: rows = timingRows(details.result, details.client);
  $: longest = Math.max(1, ...rows.map(row => row.ms ?? 0));
  let copyStatus = '';
  async function copyTiming() {
    const text = [`${details.connectionName} · ${details.result.rowCount} rows`, `Query time: ${details.result.durationMs} ms`, `End-to-end through DOM update: ${formatTiming(details.client.totalMs)}`, ...rows.map(row => `${row.label}: ${row.estimated ? '≈ ' : ''}${formatTiming(row.ms)}\n${row.note}`)].join('\n');
    try { await writeText(text).catch(() => navigator.clipboard.writeText(text)); copyStatus = 'Copied'; }
    catch { copyStatus = 'Could not copy'; }
  }
</script>

<Modal label="Query timing" {onClose}>
  <div class="modal query-timing">
    <div class="panel-head"><h2>Query timing</h2><button on:click={onClose}>Close</button></div>
    <p class="timing-context">{details.connectionName} · {details.result.rowCount} {details.result.rowCount === 1 ? 'row' : 'rows'}{details.batch ? ' in this batch' : ''} · {new Date(details.executedAt).toLocaleTimeString()}</p>
    <div class="timing-totals">
      <div><span>{details.batch ? 'Latest batch query time' : 'Query time in status bar'}</span><strong>{details.result.durationMs} ms</strong></div>
      <div><span>End-to-end, through DOM update</span><strong>{formatTiming(details.client.totalMs)}</strong></div>
    </div>
    <dl class="timing-breakdown">
      {#each rows as row}
        <div>
          <dt>{row.label}<small>{row.note}</small><span class="timing-track" aria-hidden="true"><span style:width={`${Math.max(0, (row.ms ?? 0) / longest * 100)}%`}></span></span></dt>
          <dd>{row.estimated ? '≈ ' : ''}{formatTiming(row.ms)}</dd>
        </div>
      {/each}
    </dl>
    <p class="timing-note">The {details.batch ? 'latest-batch' : 'status-bar'} duration covers Prepare SQL through Decode rows. These are client-side wall-clock spans, not EXPLAIN execution time. Timings use monotonic clocks and add no database requests.</p>
    <div class="row-actions"><span role="status">{copyStatus}</span><button on:click={copyTiming}><Icon name="copy" size={14} />Copy as text</button><button class="primary" on:click={onClose}>Done</button></div>
  </div>
</Modal>

<style>
  .timing-context, .timing-note { margin: 0; color: var(--ink-muted); font-size: 12px; line-height: 18px; }
  .timing-totals { display: flex; flex-wrap: wrap; gap: 1.4rem; padding: 12px; border: 1px solid var(--line); border-radius: 4px; background: var(--surface-2); }
  .timing-totals div { display: grid; gap: 0.3rem; }
  .timing-totals span { color: var(--ink-muted); font-size: 11px; }
  .timing-totals strong { font: 22px/28px var(--font-mono); }
  .timing-breakdown { margin: 0; }
  .timing-breakdown > div { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 1rem; padding: 0.65rem 0; border-bottom: 1px solid var(--line); }
  dt { font-size: 13px; }
  dt small { display: block; margin-top: 4px; color: var(--ink-muted); font-size: 11px; line-height: 16px; }
  dd { margin: 0; text-align: right; font: 12px/20px var(--font-mono); white-space: nowrap; }
  .timing-track { display: block; height: 3px; margin-top: 8px; background: var(--surface-3); }
  .timing-track > span { display: block; height: 100%; background: var(--accent); }
  .row-actions > span { margin-right: auto; color: var(--ink-faint); font-size: 11px; }
</style>
