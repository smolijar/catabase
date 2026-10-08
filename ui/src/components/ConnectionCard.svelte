<script lang="ts">
  import ActionMenu from './ActionMenu.svelte';
  import type { ConnectionRecord } from '../lib/types';

  export let connection: ConnectionRecord;
  export let active = false;
  export let disabled = false;
  export let onActivate: () => void;
  export let onEdit: () => void;
  export let onDelete: () => void;

  let actions: ActionMenu;
  $: address = `${connection.username}@${connection.host}:${connection.port}/${connection.database}`;
  $: items = [
    { id: 'use', label: active ? 'In use in this tab' : 'Use in this tab', disabled: active || disabled },
    { id: 'edit', label: 'Edit connection…', disabled },
    { id: 'delete', label: 'Delete connection…', danger: true, disabled }
  ];

  function activate() {
    if (!disabled) onActivate();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      event.preventDefault();
      event.stopPropagation();
      void actions.openAt();
    } else if (!event.metaKey && !event.ctrlKey && !event.altKey && (event.key === 'Enter' || event.key === ' ')) {
      event.preventDefault();
      event.stopPropagation();
      activate();
    }
  }

  function handleContextMenu(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    void actions.openAt(event.clientX, event.clientY);
  }

  function handleAction(id: string) {
    if (disabled) return;
    if (id === 'use') activate();
    else if (id === 'edit') onEdit();
    else if (id === 'delete') onDelete();
  }
</script>

<article class="connection-item" class:active aria-label={`${connection.name} connection`}>
  <button
    class="connection-select"
    type="button"
    aria-label={`Use ${connection.name} in this tab`}
    aria-pressed={active}
    aria-disabled={disabled}
    title={`${address}\nDouble-click or press Enter to use and test. Right-click for actions.`}
    on:click={(event) => event.currentTarget.focus()}
    on:dblclick={activate}
    on:keydown={handleKeydown}
    on:contextmenu={handleContextMenu}
  >
    <span class="connection-heading">
      <strong>{connection.name}</strong>
      {#if active}<span class="current-badge"><span class="status-dot"></span>This tab</span>{/if}
    </span>
    <span class="connection-address">{address}</span>
  </button>
  <ActionMenu bind:this={actions} iconOnly ariaLabel={`Actions for ${connection.name}`} items={items} onSelect={handleAction} />
</article>

<style>
  .connection-item {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    min-width: 0;
    margin-bottom: 0.4rem;
    padding: 0.2rem 0.35rem 0.2rem 0.2rem;
    border: 1px solid #1e2b44;
    border-radius: 8px;
    background: #101a2b;
  }

  .connection-item:hover {
    border-color: #354661;
    background: #142036;
  }

  .connection-item.active {
    border-color: #327f80;
    background: #11282e;
    box-shadow: inset 3px 0 0 #54b9af;
  }

  .connection-select {
    display: flex;
    flex: 1;
    flex-direction: column;
    align-items: stretch;
    gap: 0.25rem;
    min-width: 0;
    padding: 0.45rem 0.5rem;
    border: 0;
    border-radius: 5px;
    background: transparent;
    box-shadow: none;
    color: inherit;
    text-align: left;
    user-select: none;
  }

  .connection-select:hover {
    background: transparent;
    transform: none;
  }

  .connection-select[aria-disabled='true'] {
    cursor: default;
  }

  .connection-heading {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }

  strong {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.84rem;
    font-weight: 600;
  }

  .current-badge {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    gap: 0.3rem;
    color: #92dcd4;
    font-size: 0.64rem;
    font-weight: 500;
  }

  .current-badge .status-dot {
    width: 5px;
    height: 5px;
    background: #54b9af;
    box-shadow: none;
  }

  .connection-address {
    color: #91a1ba;
    font-size: 0.71rem;
    line-height: 1.35;
  }
</style>
