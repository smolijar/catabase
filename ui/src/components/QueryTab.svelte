<script lang="ts">
  import { literalTextAttributes } from '../lib/textInput';
  import ActionMenu from './ActionMenu.svelte';
  import Icon from './Icon.svelte';
  import { connectionClipboardAction as clipboardAction } from '../lib/connectionClipboard';
  import { pointerDrag } from '../lib/pointerDrag';
  import { serializeTabReference } from '../lib/tabClipboard';

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
  export let dragging = false;
  export let dropSide: 'before' | 'after' | null = null;
  export let onSelect: () => void;
  export let onRename: () => void;
  export let onDuplicate: () => void;
  export let onClose: () => void;
  export let onCopy: () => void = () => {};
  export let onPaste: (text?: string) => void = () => {};
  export let onRenameValue: (value: string) => void;
  export let onRenameKeydown: (event: KeyboardEvent) => void;
  export let onRenameBlur: () => void;
  export let onDragStart: (x: number, y: number) => void = () => {};
  export let onDragMove: (x: number, y: number) => void = () => {};
  export let onDragEnd: (cancelled: boolean) => void = () => {};
  export let onReorder: (direction: number) => void = () => {};

  let actions: ActionMenu;
  let label: HTMLButtonElement;
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
    const action = clipboardAction(event);
    if (action) {
      event.preventDefault();
      event.stopPropagation();
      // Consume repeats too; one held paste shortcut must not create many tabs.
      if (!event.repeat) {
        if (action === 'copy') onCopy();
        else onPaste();
      }
    } else if (event.altKey && event.shiftKey && !event.metaKey && !event.ctrlKey && ['ArrowLeft', 'ArrowRight'].includes(event.key)) {
      event.preventDefault();
      event.stopPropagation();
      onReorder(event.key === 'ArrowLeft' ? -1 : 1);
    } else if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
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

  function handleCopy(event: ClipboardEvent) {
    event.preventDefault();
    event.stopPropagation();
    event.clipboardData?.setData('text/plain', serializeTabReference({ id }));
    onCopy();
  }

  function handlePaste(event: ClipboardEvent) {
    event.preventDefault();
    event.stopPropagation();
    onPaste(event.clipboardData?.getData('text/plain'));
  }

  function handleClick() {
    label.focus({ preventScroll: true });
    onSelect();
  }
</script>

<div data-tab-chip-id={id} class="tab-chip" class:active class:dragging class:drop-before={dropSide === 'before'} class:drop-after={dropSide === 'after'}>
  {#if renaming}
    <div class="tab-rename"><input {...literalTextAttributes} aria-label="Tab name" value={renameValue} on:input={(event) => onRenameValue(event.currentTarget.value)} on:keydown={onRenameKeydown} on:blur={onRenameBlur} /></div>
  {:else}
    <button
      bind:this={label}
      data-tab-id={id}
      class="tab-label"
      title={`${title} · ${connectionName}\nDouble-click or F2 to rename. Right-click or Shift+F10 for actions.\nDrag or Alt+Shift+Left/Right to reorder. Cmd/Ctrl+C to copy; Cmd/Ctrl+V to duplicate a copied tab.`}
      aria-pressed={active}
      aria-haspopup="menu"
      use:pointerDrag={{ onStart: onDragStart, onMove: onDragMove, onEnd: onDragEnd }}
      on:click={handleClick}
      on:dblclick={onRename}
      on:contextmenu={handleContextMenu}
      on:keydown={handleKeydown}
      on:copy={handleCopy}
      on:paste={handlePaste}
    >
      {#if running}<span class="spinner"></span>{/if}
      {title}{#if pending}<span class="pending-dot" title="Staged edits">•</span>{/if}
      {#if readonly}<small>read only</small>{/if}
    </button>
    <button class="tab-close" aria-label={`Close ${title}`} disabled={closeDisabled} on:click={onClose}><Icon name="close" size={13} /></button>
  {/if}
  <ActionMenu bind:this={actions} contextOnly ariaLabel={`Actions for ${title}`} {items} onSelect={handleAction} />
</div>
