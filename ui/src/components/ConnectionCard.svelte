<script lang="ts">
  import ActionMenu from './ActionMenu.svelte';
  import type { ConnectionRecord } from '../lib/types';
  import { connectionClipboardAction, serializeConnectionReference } from '../lib/connectionClipboard';

  export let connection: ConnectionRecord;
  export let active = false;
  export let disabled = false;
  export let onActivate: () => void;
  export let onEdit: () => void;
  export let onDuplicate: () => void;
  export let onCopy: () => void;
  export let onPaste: (text?: string) => void;
  export let onDelete: () => void;

  let actions: ActionMenu;
  $: address = `${connection.username}@${connection.host}:${connection.port}/${connection.database}`;
  $: items = [
    { id: 'use', label: active ? 'In use in this tab' : 'Use in this tab', disabled: active || disabled },
    { id: 'edit', label: 'Edit connection…', disabled },
    { id: 'duplicate', label: 'Duplicate connection', disabled },
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
    const clipboardAction = connectionClipboardAction(event);
    if (clipboardAction) {
      event.preventDefault();
      event.stopPropagation();
      // Consume held shortcuts too, so native Paste cannot create more copies.
      if (!disabled && !event.repeat) {
        if (clipboardAction === 'copy') onCopy();
        else onPaste();
      }
      return;
    }
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

  function handleCopy(event: ClipboardEvent) {
    event.preventDefault();
    event.stopPropagation();
    if (!disabled) {
      event.clipboardData?.setData('text/plain', serializeConnectionReference(connection));
      onCopy();
    }
  }

  function handlePaste(event: ClipboardEvent) {
    event.preventDefault();
    event.stopPropagation();
    if (!disabled) onPaste(event.clipboardData?.getData('text/plain'));
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
    else if (id === 'duplicate') onDuplicate();
    else if (id === 'delete') onDelete();
  }
</script>

<article class="connection-item" class:active aria-label={`${connection.name} connection`}>
  <button
    class="connection-select"
    data-connection-id={connection.id}
    type="button"
    aria-label={`Use ${connection.name} in this tab`}
    aria-pressed={active}
    aria-disabled={disabled}
    aria-haspopup="menu"
    title={`${address}${active ? ' · This tab' : ''}\nDouble-click or press Enter to use and test. Option/Alt-click, right-click or Shift+F10 for actions.\nCmd/Ctrl+C to copy. Cmd/Ctrl+V to duplicate a copied connection.`}
    on:click={handleClick}
    on:dblclick={handleDoubleClick}
    on:keydown={handleKeydown}
    on:copy={handleCopy}
    on:paste={handlePaste}
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
    margin-bottom: 0;
    border: 0;
    border-radius: 4px;
    background: transparent;
  }

  .connection-item:hover {
    border-color: var(--line-strong);
    background: var(--surface-2);
  }

  .connection-item.active {
    border-color: var(--accent);
    background: var(--accent-soft);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .connection-select {
    display: block;
    flex: 1;
    width: 100%;
    min-width: 0;
    min-height: 28px;
    padding: 4px 10px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    box-shadow: none;
    color: inherit;
    text-align: left;
    user-select: none;
    font-size: 13px;
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

  strong { font-weight: 500; }
  .connection-username { color: var(--ink-muted); }
</style>
