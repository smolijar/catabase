<script lang="ts">
  import Icon from './Icon.svelte';
  import type { ArchivedTab, WorkspaceTab } from '../lib/workspace';
  import type { ConnectionRecord } from '../lib/types';
  export let tabs: WorkspaceTab[];
  export let archivedTabs: ArchivedTab[];
  export let connections: ConnectionRecord[];
  export let activeId: string;
  export let onSelect: (id: string) => void;
  export let onReopen: (id: string) => void;
  export let onNew: () => void;
  let search = '';
  const connectionName = (tab: { connectionId: string | null }) => connections.find(c => c.id === tab.connectionId)?.name ?? (tab.connectionId ? 'Unavailable connection' : 'No connection');
  const matches = (tab: { title: string; sql: string; connectionId: string | null }, search: string, connections: ConnectionRecord[]) => `${tab.title} ${tab.sql} ${connections.find(c => c.id === tab.connectionId)?.name ?? ''}`.toLowerCase().includes(search.trim().toLowerCase());
  const preview = (sql: string) => sql.replace(/\s+/g, ' ').trim() || 'Empty query';
  $: open = tabs.filter(tab => matches(tab, search, connections));
  $: closed = archivedTabs.filter(tab => matches(tab, search, connections));
</script>

<div class="panel-head">
  <div><h2>Tabs</h2><p>{tabs.length} open · {archivedTabs.length} closed</p></div>
  <button class="icon-label-button" on:click={onNew}><Icon name="plus" size={15} /> New</button>
</div>
<label class="search-field">
  <Icon name="search" size={15} />
  <input aria-label="Search tabs" placeholder="Search names, SQL, connections…" bind:value={search} />
  {#if search}<button aria-label="Clear tab search" on:click={() => search = ''}><Icon name="close" size={13} /></button>{/if}
</label>
<section aria-label="Open tabs">
  <h3>Open <span>{open.length}</span></h3>
  {#each open as tab (tab.id)}
    <button class="tab-row" class:active={tab.id === activeId} aria-current={tab.id === activeId ? 'page' : undefined} title={`${tab.title}\n${connectionName(tab)}\n${tab.sql}`} on:click={() => onSelect(tab.id)}>
      <span class="row-heading"><strong>{tab.title}</strong>{#if tab.kind === 'reference'}<small>Read only</small>{/if}</span>
      <span class="sql-preview">{preview(tab.sql)}</span>
      <small>{connectionName(tab)}</small>
    </button>
  {/each}
</section>
<section aria-label="Closed tabs">
  <h3>Closed <span>{closed.length}</span></h3>
  {#each closed as tab (tab.id)}
    <button class="tab-row" aria-label={`Reopen ${tab.title}`} title={`Reopen ${tab.title}\nClosed ${new Date(tab.closedAt).toLocaleString()}\n${connectionName(tab)}\n${tab.sql}`} on:click={() => onReopen(tab.id)}>
      <span class="row-heading"><strong>{tab.title}</strong><span class="reopen"><Icon name="refresh" size={13} /> Reopen</span></span>
      <span class="sql-preview">{preview(tab.sql)}</span>
      <small>{connectionName(tab)} · {new Date(tab.closedAt).toLocaleDateString()}</small>
    </button>
  {/each}
  {#if !closed.length}<p class="empty">{search.trim() ? 'No closed tabs match.' : 'Closed queries are saved here. Reopen them here or from global search.'}</p>{/if}
</section>

<style>
  section { margin-bottom: 16px; }
  h3 { display: flex; justify-content: space-between; margin: 16px 0 6px; color: var(--ink-muted); font: 11px/18px var(--font-mono); text-transform: uppercase; letter-spacing: .06em; }
  h3 span { color: var(--ink-faint); }
  .tab-row { display: flex; flex-direction: column; align-items: stretch; gap: 3px; width: 100%; padding: 8px 10px; text-align: left; border: 0; border-left: 2px solid transparent; border-radius: 0; background: transparent; }
  .tab-row:hover { background: var(--surface-3); }
  .tab-row.active { border-left-color: var(--accent); background: var(--accent-soft); color: var(--accent); }
  .row-heading { display: flex; align-items: center; gap: 8px; }
  strong { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; font-weight: 500; }
  .sql-preview { font: 11px/17px var(--font-mono); color: var(--ink-muted); }
  .sql-preview, small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  small { color: var(--ink-faint); font-size: 11px; }
  .row-heading small, .reopen { margin-left: auto; flex-shrink: 0; }
  .reopen { display: flex; align-items: center; gap: 4px; color: var(--ink-muted); font-size: 11px; }
</style>
