<script lang="ts">
  import Modal from './Modal.svelte';
  import { formatTiming, timingRows } from '../lib/queryTiming';
  import type { QueryTimingDetails } from '../lib/queryTiming';
  export let details: QueryTimingDetails;
  export let onClose: () => void;
  $: rows = timingRows(details.result, details.client);
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
          <dt>{row.label}<small>{row.note}</small></dt>
          <dd>{row.estimated ? '≈ ' : ''}{formatTiming(row.ms)}</dd>
        </div>
      {/each}
    </dl>
    <p class="timing-note">The {details.batch ? 'latest-batch' : 'status-bar'} duration covers Prepare SQL through Decode rows. These are client-side wall-clock spans, not EXPLAIN execution time. Timings use monotonic clocks and add no database requests.</p>
  </div>
</Modal>

<style>
  .timing-context, .timing-note { margin: 0; color: #94a3b8; font-size: 0.8rem; line-height: 1.5; }
  .timing-totals { display: flex; flex-wrap: wrap; gap: 1.4rem; padding: 0.8rem; border: 1px solid #2c3d57; border-radius: 8px; background: #101b2d; }
  .timing-totals div { display: grid; gap: 0.3rem; }
  .timing-totals span { color: #a6b5cb; font-size: 0.75rem; }
  .timing-totals strong { font-size: 1.15rem; font-variant-numeric: tabular-nums; }
  .timing-breakdown { margin: 0; }
  .timing-breakdown > div { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 1rem; padding: 0.65rem 0; border-bottom: 1px solid #243249; }
  dt { font-size: 0.85rem; }
  dt small { display: block; margin-top: 0.2rem; color: #94a3b8; font-size: 0.73rem; line-height: 1.4; }
  dd { margin: 0; text-align: right; font-size: 0.85rem; font-variant-numeric: tabular-nums; white-space: nowrap; }
</style>
