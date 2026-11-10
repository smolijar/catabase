<script lang="ts">
  import ActionMenu from './ActionMenu.svelte';
  import Icon from './Icon.svelte';
  import type { ConnectionAppearance } from '../lib/appearance';
  import type { ConnectionRecord } from '../lib/types';
  import { connectionClipboardAction, serializeConnectionReference } from '../lib/connectionClipboard';
  import { createConnectionInteraction } from '../lib/connectionInteraction';

  export let connection: ConnectionRecord;
  export let active = false;
  export let appearance: ConnectionAppearance | undefined = undefined;
  export let disabled = false;
  export let onActivate: () => void | Promise<void>;
  export let onDisconnect: () => void = () => {};
  export let onEdit: () => void;
  export let onDuplicate: () => void;
  export let onCopy: () => void;
  export let onPaste: (text?: string) => void;
  export let onDelete: () => void;

  let actions: ActionMenu;
  let selectButton: HTMLButtonElement;
  const interaction = createConnectionInteraction(() => onActivate(), () => onEdit(), () => disabled);
  $: address = `${connection.username}@${connection.host}:${connection.port}/${connection.database}`;
  $: color = appearance?.color === 'none' ? undefined : appearance?.color;
  $: environment = appearance?.environment;
  $: items = [
    { id: 'use', label: active ? 'In use in this tab' : 'Use in this tab', disabled: active || disabled },
    ...(active ? [{ id: 'disconnect', label: 'Disconnect', disabled }] : []),
    { id: 'edit', label: 'Edit connection…', disabled },
    { id: 'duplicate', label: 'Duplicate connection', disabled },
    { id: 'delete', label: 'Delete connection…', danger: true, disabled }
  ];

  function activate() {
    interaction.use();
  }

  function handleClick(event: MouseEvent) {
    (event.currentTarget as HTMLElement).focus();
    if (event.altKey) {
      event.preventDefault();
      event.stopPropagation();
      void actions.openAt(event.clientX, event.clientY);
    } else if (event.detail < 2) {
      activate();
    }
  }

  function handleDoubleClick(event: MouseEvent) {
    if (!event.altKey) void interaction.edit();
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
    if (event.key === 'F2' && !event.metaKey && !event.ctrlKey && !event.altKey) {
      event.preventDefault();
      void interaction.edit();
    } else if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
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
    else if (id === 'disconnect') onDisconnect();
    else if (id === 'edit') onEdit();
    else if (id === 'duplicate') onDuplicate();
    else if (id === 'delete') onDelete();
  }

  function disconnect() {
    // The disconnect button disappears when the row is no longer active.
    selectButton.focus();
    onDisconnect();
  }
</script>

<article class="connection-item" class:active aria-label={`${connection.name} connection`}>
  <button
    bind:this={selectButton}
    class="connection-select"
    data-connection-id={connection.id}
    type="button"
    aria-label={`Use ${connection.name} in this tab${environment ? ` (${environment})` : ''}`}
    aria-pressed={active}
    aria-disabled={disabled}
    aria-haspopup="menu"
    title={`${address}${active ? ' · This tab' : ''}\nClick or press Enter to use and test. Double-click or F2 to edit. Option/Alt-click, right-click or Shift+F10 for actions.\nCmd/Ctrl+C to copy. Cmd/Ctrl+V to duplicate a copied connection.`}
    on:click={handleClick}
    on:dblclick={handleDoubleClick}
    on:keydown={handleKeydown}
    on:copy={handleCopy}
    on:paste={handlePaste}
    on:contextmenu={handleContextMenu}
  >
    {#if color}<span class="connection-color" data-conn={color} aria-hidden="true"></span>{/if}
    <span class="connection-label"><strong>{connection.name}</strong><span class="connection-username">{` (${connection.username})`}</span></span>
    {#if environment}<span class="environment" data-conn={color} title={environment}>{environment}</span>{/if}
  </button>
  {#if active}
    <button type="button" class="icon-button disconnect-button" {disabled}
      aria-label={`Disconnect ${connection.name}`} title={`Disconnect ${connection.name}`} on:click={disconnect}>
      <Icon name="disconnect" size={16} />
    </button>
  {/if}
  <ActionMenu bind:this={actions} contextOnly ariaLabel={`Actions for ${connection.name}`} items={items} onSelect={handleAction} />
</article>

<style>
  .connection-item {
    display: flex;
    align-items: center;
    min-width: 0;
    margin-bottom: 0;
    border: 0;
    border-radius: 0;
  }

  .connection-item:not(.active):hover {
    background: var(--surface-2);
  }

  .connection-select {
    display: flex;
    justify-content: flex-start;
    flex: 1;
    width: 100%;
    min-width: 0;
    min-height: 28px;
    padding: 4px 10px;
    border: 0;
    border-radius: 0;
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

  .connection-color { flex: 0 0 6px; height: 6px; background: var(--accent); }

  .environment {
    flex-shrink: 0;
    max-width: 40%;
    padding: 0 4px;
    border-radius: 2px;
    background: var(--surface-3);
    color: var(--ink-muted);
    font: 10px/16px var(--font-mono);
    text-transform: uppercase;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .environment[data-conn] { color: var(--accent); background: var(--accent-soft); }

  .disconnect-button {
    flex: 0 0 28px;
    border: 0;
    background: transparent;
    color: var(--ink-muted);
  }

  .disconnect-button:hover:not(:disabled) { background: var(--surface-3); color: var(--ink); }
  .disconnect-button:focus-visible { outline-offset: -2px; }

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
