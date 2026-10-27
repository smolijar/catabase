<script lang="ts">
  import Icon from './Icon.svelte';
  import { formatBytes, historyEntryBytes } from '../lib/history';
  import type { HistoryEntry, ConnectionRecord } from '../lib/types';
  export let entry: HistoryEntry;
  export let grouped = false;
  export let connections: ConnectionRecord[];
  export let onCopy: (entry: HistoryEntry) => void;
  export let onOpen: (entry: HistoryEntry) => void;
  export let onDelete: (id: number) => void;
  $: connectionName = connections.find(connection => connection.id === entry.connectionId)?.name ?? 'Unavailable connection';
  $: sizeDetails = `Referenced content: ${formatBytes(historyEntryBytes(entry))}\nSQL: ${formatBytes(entry.querySize)} · shared by ${entry.queryReferences} executions\nResult: ${entry.hasResult ? `${formatBytes(entry.resultSize ?? 0)} · shared by ${entry.resultReferences} executions` : 'No snapshot stored'}\nExecution details: ${formatBytes(entry.executionSize)}\nLogical size, not exclusive disk usage. Shared SQL and results are stored once.${entry.queryHash ? `\nQuery hash: ${entry.queryHash}` : ''}${entry.resultHash ? `\nResult hash: ${entry.resultHash}` : ''}`;
</script>

<article aria-label={`History #${entry.id}`} class:error-card={entry.isError} class:transaction-entry={grouped} class="history-card">
  <div class="history-entry-head">
    <button class="history-sql" aria-label="Copy SQL" title={`Copy SQL\n${entry.query}`} on:click={() => onCopy(entry)}><code>{entry.query}</code><Icon name="copy" size={13} /></button>
    <button class="history-icon-button" aria-label="Open" title="Open SQL and saved result" on:click={() => onOpen(entry)}><Icon name="open" size={15} /></button>
    <button class="history-icon-button danger-quiet" aria-label="Delete history entry" title="Delete history entry" on:click={() => onDelete(entry.id)}><Icon name="trash" size={14} /></button>
  </div>
  <div class="history-meta">
    <span role="img" aria-label={entry.isError ? 'Error' : 'Success'} title={entry.isError ? 'Error' : 'Success'} class:error-status={entry.isError} class="history-status"><Icon name={entry.isError ? 'close' : 'check'} size={12} /></span>
    <time title={entry.executedAt}>{new Date(entry.executedAt).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })}</time>
    <span>{entry.durationMs} ms</span>
    {#if entry.rowCount != null}<span>{entry.rowCount} {entry.rowCount === 1 ? 'row' : 'rows'}</span>{/if}
    <span class="history-size" title={sizeDetails}>{formatBytes(historyEntryBytes(entry))}</span>
  </div>
  {#if !grouped}
    <small class="history-connection" title={connectionName}>{connectionName}</small>
  {/if}
  {#if entry.errorMessage}<small class="history-error" title={entry.errorMessage}>{entry.errorMessage}</small>{/if}
</article>
