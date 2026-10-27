<script lang="ts">
  import Icon from './Icon.svelte';
  import { formatBytes, historyEntryBytes } from '../lib/history';
  import type { HistoryEntry, ConnectionRecord } from '../lib/types';
  export let entry: HistoryEntry;
  export let grouped = false;
  export let connections: ConnectionRecord[];
  export let onRestore: (entry: HistoryEntry) => void;
  export let onOpen: (entry: HistoryEntry) => void;
  export let onStorage: (entry: HistoryEntry) => void;
  export let onDelete: (id: number) => void;
</script>

<article class:error-card={entry.isError} class:transaction-entry={grouped} class="history-card">
  <div class="history-meta">
    <span class:error-status={entry.isError} class="history-status">{entry.isError ? 'Error' : 'Success'}</span>
    <time title={entry.executedAt}>{new Date(entry.executedAt).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })}</time>
  </div>
  <code title={entry.query}>{entry.query}</code>
  <small>{entry.durationMs} ms{entry.rowCount != null ? ` · ${entry.rowCount} ${entry.rowCount === 1 ? 'row' : 'rows'}` : ''}</small>
  <button class="history-storage-link" title="Show size, shared content and hashes for this execution" on:click={() => onStorage(entry)}>{formatBytes(historyEntryBytes(entry))}{entry.resultReferences > 1 ? ' · shared result' : entry.queryReferences > 1 ? ' · shared SQL' : ''}</button>
  {#if !grouped}
    <small class="history-connection">{connections.find(connection => connection.id === entry.connectionId)?.name ?? 'Unavailable connection'}</small>
    {#if entry.transaction}<small title={`Application transaction ID: ${entry.transaction.id}`}>In transaction · {entry.transaction.id.slice(0, 8)}</small>{/if}
  {/if}
  {#if entry.errorMessage}<small class="history-error">{entry.errorMessage}</small>{/if}
  <div class="row-actions">
    <button on:click={() => onRestore(entry)}><Icon name="plus" size={14} /> New query</button>
    <button disabled={!entry.hasResult && !entry.isError} on:click={() => onOpen(entry)}>Open result</button>
    <button class="danger-quiet" aria-label="Delete history entry" title="Delete history entry" on:click={() => onDelete(entry.id)}><Icon name="trash" size={14} /></button>
  </div>
</article>
