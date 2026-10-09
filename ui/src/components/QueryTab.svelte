<script lang="ts">
  import ActionMenu from './ActionMenu.svelte';
  import Icon from './Icon.svelte';

  export let id: string;
  export let title: string;
  export let connectionName: string;
  export let active = false;
  export let readonly = false;
  export let running = false;
  export let pending = false;
  export let closeDisabled = false;
  export let renaming = false;
  export let renameValue = '';
  export let onSelect: () => void;
  export let onRename: () => void;
  export let onDuplicate: () => void;
  export let onClose: () => void;
  export let onRenameValue: (value: string) => void;
  export let onRenameKeydown: (event: KeyboardEvent) => void;
  export let onRenameBlur: () => void;

  let actions: ActionMenu;
  $: items = [
    { id: 'close', label: 'Close tab', disabled: closeDisabled },
    { id: 'duplicate', label: 'Duplicate tab' },
    { id: 'rename', label: 'Rename tab…' }
  ];

  function handleContextMenu(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    (event.currentTarget as HTMLElement).focus();
    void actions.openAt(event.clientX, event.clientY);
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      event.preventDefault();
      event.stopPropagation();
      const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
      void actions.openAt(rect.left, rect.bottom);
    } else if (event.key === 'F2' && !event.metaKey && !event.ctrlKey && !event.altKey) {
      event.preventDefault();
      event.stopPropagation();
      onRename();
    }
  }

  function handleAction(action: string) {
    if (action === 'close' && !closeDisabled) onClose();
    else if (action === 'duplicate') onDuplicate();
    else if (action === 'rename') onRename();
  }
</script>

<div class="tab-chip" class:active>
  {#if renaming}
    <div class="tab-rename"><input aria-label="Tab name" value={renameValue} on:input={(event) => onRenameValue(event.currentTarget.value)} on:keydown={onRenameKeydown} on:blur={onRenameBlur} /></div>
  {:else}
    <button
      data-tab-id={id}
      class="tab-label"
      title={`${title} · ${connectionName}\nDouble-click or F2 to rename. Right-click or Shift+F10 for actions.`}
      aria-pressed={active}
      aria-haspopup="menu"
      on:click={onSelect}
      on:dblclick={onRename}
      on:contextmenu={handleContextMenu}
      on:keydown={handleKeydown}
    >
      {#if running}<span class="spinner"></span>{/if}
      {title}{#if pending}<span class="pending-dot" title="Staged edits">•</span>{/if}
      {#if readonly}<small>read only</small>{/if}
    </button>
    <button class="tab-close" aria-label={`Close ${title}`} disabled={closeDisabled} on:click={onClose}><Icon name="close" size={13} /></button>
  {/if}
  <ActionMenu bind:this={actions} contextOnly ariaLabel={`Actions for ${title}`} {items} onSelect={handleAction} />
</div>
