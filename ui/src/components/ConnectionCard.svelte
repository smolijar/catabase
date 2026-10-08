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

  function handleClick(event: MouseEvent) {
    (event.currentTarget as HTMLElement).focus();
    if (event.altKey) {
      event.preventDefault();
      event.stopPropagation();
      void actions.openAt(event.clientX, event.clientY);
    }
  }

  function handleDoubleClick(event: MouseEvent) {
    if (!event.altKey) activate();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      event.preventDefault();
      event.stopPropagation();
      const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
      void actions.openAt(rect.left, rect.bottom);
    } else if (!event.metaKey && !event.ctrlKey && !event.altKey && (event.key === 'Enter' || event.key === ' ')) {
      event.preventDefault();
      event.stopPropagation();
      activate();
    }
  }

  function handleContextMenu(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    (event.currentTarget as HTMLElement).focus();
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
    aria-haspopup="menu"
    title={`${address}${active ? ' · This tab' : ''}\nDouble-click or press Enter to use and test. Option/Alt-click, right-click or Shift+F10 for actions.`}
    on:click={handleClick}
    on:dblclick={handleDoubleClick}
    on:keydown={handleKeydown}
    on:contextmenu={handleContextMenu}
  >
    <span class="connection-label"><strong>{connection.name}</strong><span class="connection-username">{` (${connection.username})`}</span></span>
  </button>
  <ActionMenu bind:this={actions} contextOnly ariaLabel={`Actions for ${connection.name}`} items={items} onSelect={handleAction} />
</article>

<style>
  .connection-item {
    display: flex;
    align-items: center;
    min-width: 0;
    margin-bottom: 2px;
    border: 1px solid transparent;
    border-radius: 5px;
    background: transparent;
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
    display: block;
    flex: 1;
    width: 100%;
    min-width: 0;
    min-height: 28px;
    padding: 4px 10px;
    border: 0;
    border-radius: 5px;
    background: transparent;
    box-shadow: none;
    color: inherit;
    text-align: left;
    user-select: none;
    font-size: .78rem;
    line-height: 20px;
  }

  .connection-select:hover {
    background: transparent;
    transform: none;
  }

  .connection-select[aria-disabled='true'] {
    cursor: default;
  }

  .connection-select:focus-visible { outline-offset: -2px; }

  .connection-label {
    display: block;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  strong { font-weight: 700; }
  .connection-username { color: #91a1ba; }
</style>
