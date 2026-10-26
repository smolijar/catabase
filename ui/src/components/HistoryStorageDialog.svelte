<script lang="ts">
  import Modal from './Modal.svelte';
  import { formatBytes, historyEntryBytes } from '../lib/history';
  import type { HistoryEntry, HistoryStorageStats } from '../lib/types';
  export let entry: HistoryEntry | null = null;
  export let stats: HistoryStorageStats | null = null;
  export let error = '';
  export let onClose: () => void;
</script>

<Modal label={entry ? `History #${entry.id} storage` : 'History storage'} {onClose}>
  <div class="modal">
    <div class="panel-head"><h2>{entry ? `History #${entry.id} storage` : 'History storage'}</h2><button on:click={onClose}>Close</button></div>
    {#if entry}
      <p>This execution references <strong>{formatBytes(historyEntryBytes(entry))}</strong> of content. Shared SQL and snapshots are stored once, not once per execution.</p>
      <dl>
        <div><dt>SQL <small>Shared by {entry.queryReferences} {entry.queryReferences === 1 ? 'execution' : 'executions'}</small></dt><dd>{formatBytes(entry.querySize)}</dd></div>
        <div><dt>Result snapshot <small>{entry.hasResult ? `Shared by ${entry.resultReferences} ${entry.resultReferences === 1 ? 'execution' : 'executions'}` : 'No snapshot stored. Errors and results over the 256 KiB limit retain only execution details.'}</small></dt><dd>{entry.hasResult ? formatBytes(entry.resultSize ?? 0) : 'Not stored'}</dd></div>
        <div><dt>This execution <small>Timing, connection, timestamp, errors and hash references</small></dt><dd>{formatBytes(entry.executionSize)}</dd></div>
      </dl>
      <details><summary>SHA-256 content hashes</summary><div class="hash">Query<code>{entry.queryHash}</code></div>{#if entry.resultHash}<div class="hash">Response<code>{entry.resultHash}</code></div>{/if}</details>
      <p class="note">These are logical content sizes, not exclusive disk usage per item. Deleting an entry frees shared content only when no other history entry references it.</p>
    {:else if error}
      <p class="error-text" role="alert">{error}</p>
    {:else if stats}
      <div class="totals"><div><span>History on disk</span><strong>{formatBytes(stats.allocatedBytes)}</strong></div><div><span>Duplicate payload avoided</span><strong>{formatBytes(stats.savedBytes)}</strong></div></div>
      <dl>
        <div><dt>Unique SQL <small>{stats.uniqueQueries} distinct queries</small></dt><dd>{formatBytes(stats.queryBytes)}</dd></div>
        <div><dt>Unique result snapshots <small>{stats.uniqueResults} distinct responses; timing differences do not create copies</small></dt><dd>{formatBytes(stats.resultBytes)}</dd></div>
        <div><dt>Execution details <small>Each run retains its own timestamp, timing, connection and outcome</small></dt><dd>{formatBytes(stats.executionBytes)}</dd></div>
        <div><dt>Total logical content</dt><dd>{formatBytes(stats.storedBytes)}</dd></div>
      </dl>
      <p class="note">On-disk size includes SQLite pages and indexes belonging to history, not connections or drafts. SQLite may retain freed pages for reuse, so the database file itself need not shrink after deduplication or deletion. Sizes use 1 KB = 1,000 bytes.</p>
    {:else}
      <p role="status">Calculating history storage…</p>
    {/if}
  </div>
</Modal>

<style>
  p { margin: 0; font-size: 13px; line-height: 1.5; color: var(--ink-muted); }
  .totals { display: flex; flex-wrap: wrap; gap: 28px; padding: 12px; border: 1px solid var(--line); }
  .totals div { display: grid; gap: 6px; }
  .totals span, small, .note { color: var(--ink-muted); font-size: 11px; }
  .totals strong { font: 22px var(--font-mono); }
  dl { margin: 0; }
  dl > div { display: flex; justify-content: space-between; gap: 24px; padding: 12px 0; border-bottom: 1px solid var(--line); }
  dt { font-size: 13px; }
  small { display: block; margin-top: 4px; }
  dd { margin: 0; white-space: nowrap; font: 12px var(--font-mono); }
  details { font-size: 12px; }
  summary { cursor: pointer; }
  .hash { display: grid; gap: 4px; margin-top: 12px; }
  code { overflow-wrap: anywhere; user-select: text; font-size: 11px; color: var(--ink-muted); }
</style>
