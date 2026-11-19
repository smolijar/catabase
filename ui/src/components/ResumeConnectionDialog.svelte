<script lang="ts">
  import Modal from './Modal.svelte';
  import Dropdown from './Dropdown.svelte';
  import Icon from './Icon.svelte';
  import type { ConnectionRecord } from '../lib/types';
  import type { ConnectionAppearance } from '../lib/appearance';

  export let connections: ConnectionRecord[];
  export let initialConnectionId: string;
  export let appearances: Record<string, ConnectionAppearance>;
  export let query: string;
  export let locked = false;
  export let globalSelection = false;
  export let onConfirm: (connection: ConnectionRecord) => void;
  export let onClose: () => void;

  let selectedId = initialConnectionId;
  $: connection = connections.find(connection => connection.id === selectedId);
  $: appearance = appearances[selectedId] ?? { color: 'none', environment: '' };
  $: options = connections.map(connection => ({ value: connection.id, label: `${connection.name} (${connection.username})` }));
</script>

<Modal label="Connect before running" {onClose}>
  <div class="modal resume-connection" data-conn={appearance.color}>
    <div class="panel-head"><h2>Connect before running</h2><button on:click={onClose}>Cancel</button></div>
    <p>This tab is disconnected. Review the connection before running this query.</p>
    {#if locked}<p><Icon name="lock" size={12} /> This tab’s connection is locked. Unlock it from the tab menu to choose another connection.</p>
    {:else if globalSelection}<p>Choosing a different connection updates all unlocked tabs.</p>{/if}
    {#if connections.length > 1 && !locked}
      <Dropdown value={selectedId} {options} ariaLabel="Connection to use" onSelect={id => selectedId = id} />
    {/if}
    {#if connection}
      <div class="connection-summary">
        <div class="connection-title"><span class="connection-color" class:empty-color={appearance.color === 'none'} role="img" aria-label={appearance.color === 'none' ? 'No connection color' : `${appearance.color} connection color`}></span><strong>{connection.name}</strong><span class="username">({connection.username})</span>
          {#if appearance.environment}<span class="environment">{appearance.environment}</span>{/if}
          {#if connection.options?.readOnly}<span class="read-only">Read-only default</span>{/if}
        </div>
        <dl><div><dt>Database</dt><dd>{connection.database}</dd></div><div><dt>Host</dt><dd>{connection.host}:{connection.port}</dd></div><div><dt>SSL</dt><dd>{connection.ssl ? 'Required' : 'Disabled'}</dd></div></dl>
      </div>
    {:else}<p class="error-text">This saved connection is no longer available. Choose another connection.</p>{/if}
    <div class="sql-context"><span>SQL to run</span><pre aria-label="SQL to run">{query}</pre></div>
    <div class="row-actions"><button class="primary" disabled={!connection} on:click={() => connection && onConfirm(connection)}><Icon name="play" size={14} />Connect & run</button></div>
  </div>
</Modal>

<style>
  p { margin: 0; color: var(--ink-muted); font-size: 12px; line-height: 18px; }
  .connection-summary { padding: 14px; border: 1px solid var(--line); border-left: 3px solid var(--accent); border-radius: 3px; background: var(--surface-1); }
  .connection-title { display: flex; align-items: center; flex-wrap: wrap; gap: 7px; font-size: 14px; }
  .connection-title strong { color: var(--ink); }
  .username { color: var(--ink-muted); font: 12px var(--font-mono); }
  .connection-color { width: 9px; height: 9px; flex: 0 0 auto; background: var(--accent); }
  .empty-color { background: transparent; border: 1px solid var(--line-strong); }
  .environment { padding: 1px 5px; border-radius: 2px; color: var(--accent); background: var(--accent-soft); font: 12px/18px var(--font-mono); white-space: pre-wrap; }
  .read-only { color: var(--ink-muted); font-size: 11px; }
  dl { display: grid; gap: 6px; margin: 12px 0 0; }
  dl > div { display: grid; grid-template-columns: 65px minmax(0, 1fr); gap: 10px; }
  dt { color: var(--ink-muted); font-size: 12px; }
  dd { margin: 0; color: var(--ink); font: 12px/18px var(--font-mono); overflow-wrap: anywhere; }
  .sql-context > span { color: var(--ink-muted); font-size: 12px; }
  pre { margin: 6px 0 0; padding: 12px; max-height: 180px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; background: var(--surface-0); border: 1px solid var(--line); border-radius: 3px; font-size: 12px; line-height: 18px; }
  .row-actions { justify-content: flex-end; }
</style>
