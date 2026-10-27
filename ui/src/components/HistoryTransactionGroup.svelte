<script lang="ts">
  import { onDestroy } from 'svelte';
  import HistoryEntryCard from './HistoryEntryCard.svelte';
  import { api, errorMessage } from '../lib/ipc';
  import { transactionLabel, transactionStatus } from '../lib/history';
  import type { HistoryTransaction, HistoryEntry, ConnectionRecord } from '../lib/types';
  export let transaction: HistoryTransaction;
  export let connections: ConnectionRecord[];
  export let connectionId: string | null | undefined;
  export let matches: HistoryEntry[] = [];
  export let searching = false;
  export let revision: number;
  export let onRestore: (entry: HistoryEntry) => void;
  export let onOpen: (entry: HistoryEntry) => void;
  export let onStorage: (entry: HistoryEntry) => void;
  export let onDelete: (id: number) => void;
  let open = false;
  let entries: HistoryEntry[] = [];
  let total = transaction.statementCount;
  let loading = false;
  let error = '';
  let generation = 0;
  $: if (open && !searching) refresh(transaction.id, revision);
  $: displayed = searching ? [...matches].sort((a, b) => a.id - b.id) : entries;
  onDestroy(() => { generation++; });

  function refresh(id: string, _revision: number) { void load(id); }
  async function load(id: string, append = false) {
    const request = ++generation;
    loading = true;
    error = '';
    try {
      const wanted = append ? 50 : Math.max(50, entries.length);
      const next: HistoryEntry[] = [];
      let count = 0;
      do {
        const page = await api.historyTransactionPage(id, (append ? entries.length : 0) + next.length, Math.min(200, wanted - next.length));
        if (request !== generation) return;
        next.push(...page.entries);
        count = page.total;
        if (!page.entries.length) break;
      } while (next.length < wanted && (append ? entries.length : 0) + next.length < count);
      entries = append ? [...entries, ...next] : next;
      total = count;
    } catch (cause) {
      if (request === generation) error = errorMessage(cause);
    } finally {
      if (request === generation) loading = false;
    }
  }
</script>

<details class="history-transaction" bind:open>
  <summary title={`Application transaction ID: ${transaction.id}`}>
    <span class="history-transaction-heading"><strong>{transactionLabel(transaction)}</strong><code>{transaction.id.slice(0, 8)}</code></span>
    <span class="history-transaction-detail">{transaction.statementCount} {transaction.statementCount === 1 ? 'statement' : 'statements'} · {transactionStatus(transaction)}</span>
    <span class="history-transaction-detail">{connections.find(connection => connection.id === connectionId)?.name ?? 'Unavailable connection'}</span>
  </summary>
  {#if open}
    <div class="history-transaction-entries">
      <small class="history-transaction-note">{searching ? 'Matching statements · clear search for full transaction' : 'Execution order · success does not imply commit'}</small>
      {#each displayed as entry (entry.id)}
        <HistoryEntryCard {entry} grouped {connections} {onRestore} {onOpen} {onStorage} {onDelete} />
      {/each}
      {#if !searching}
        {#if error}<p class="error-text" role="alert">{error}</p><button on:click={() => load(transaction.id)}>Retry</button>{/if}
        {#if loading}<small role="status">Loading statements…</small>{/if}
        {#if entries.length < total}<button class="history-more" disabled={loading} on:click={() => load(transaction.id, true)}>Load more statements ({entries.length} of {total})</button>{/if}
      {/if}
    </div>
  {/if}
</details>
