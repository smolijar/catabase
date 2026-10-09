<script lang="ts">
  import { onDestroy } from 'svelte';
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
  export let dragging = false;
  export let dropSide: 'before' | 'after' | null = null;
  export let onSelect: () => void;
  export let onRename: () => void;
  export let onDuplicate: () => void;
  export let onClose: () => void;
  export let onRenameValue: (value: string) => void;
  export let onRenameKeydown: (event: KeyboardEvent) => void;
  export let onRenameBlur: () => void;
  export let onDragStart: (x: number, y: number) => void = () => {};
  export let onDragMove: (x: number, y: number) => void = () => {};
  export let onDragEnd: (cancelled: boolean) => void = () => {};
  export let onReorder: (direction: number) => void = () => {};

  let actions: ActionMenu;
  let label: HTMLButtonElement;
  let drag: { pointerId: number; x: number; y: number; started: boolean } | null = null;
  let suppressClick = false;
  onDestroy(() => finishDrag(true));
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
    if (event.altKey && event.shiftKey && !event.metaKey && !event.ctrlKey && ['ArrowLeft', 'ArrowRight'].includes(event.key)) {
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

  function handleClick(event: MouseEvent) {
    if (suppressClick) {
      suppressClick = false;
      event.preventDefault();
      event.stopPropagation();
    } else onSelect();
  }

  function startDrag(event: PointerEvent) {
    suppressClick = false;
    if (event.button !== 0 || !event.isPrimary || event.metaKey || event.ctrlKey || event.altKey || drag) return;
    drag = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, started: false };
    window.addEventListener('pointermove', moveDrag);
    window.addEventListener('pointerup', endPointerDrag);
    window.addEventListener('pointercancel', cancelPointerDrag);
    window.addEventListener('mouseup', endMouseDrag);
    window.addEventListener('blur', cancelDrag);
    window.addEventListener('keydown', cancelDragWithEscape);
  }

  function moveDrag(event: PointerEvent) {
    if (!drag || drag.pointerId !== event.pointerId) return;
    if (!drag.started && Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 6) return;
    event.preventDefault();
    if (!drag.started) {
      drag.started = true;
      label.setPointerCapture(event.pointerId);
      onDragStart(event.clientX, event.clientY);
    }
    onDragMove(event.clientX, event.clientY);
  }

  function endPointerDrag(event: PointerEvent) {
    if (!drag || drag.pointerId !== event.pointerId) return;
    if (drag.started) onDragMove(event.clientX, event.clientY);
    finishDrag(false);
  }

  function cancelPointerDrag(event: PointerEvent) {
    if (drag?.pointerId === event.pointerId) finishDrag(true);
  }

  function endMouseDrag() { finishDrag(false); }
  function cancelDrag() { finishDrag(true); }
  function cancelDragWithEscape(event: KeyboardEvent) {
    if (event.key !== 'Escape' || !drag?.started) return;
    event.preventDefault();
    event.stopPropagation();
    finishDrag(true);
  }

  function finishDrag(cancelled: boolean) {
    if (!drag) return;
    const { pointerId, started } = drag;
    drag = null;
    window.removeEventListener('pointermove', moveDrag);
    window.removeEventListener('pointerup', endPointerDrag);
    window.removeEventListener('pointercancel', cancelPointerDrag);
    window.removeEventListener('mouseup', endMouseDrag);
    window.removeEventListener('blur', cancelDrag);
    window.removeEventListener('keydown', cancelDragWithEscape);
    if (label?.hasPointerCapture(pointerId)) label.releasePointerCapture(pointerId);
    if (started) {
      suppressClick = true;
      onDragEnd(cancelled);
    }
  }
</script>

<div data-tab-chip-id={id} class="tab-chip" class:active class:dragging class:drop-before={dropSide === 'before'} class:drop-after={dropSide === 'after'}>
  {#if renaming}
    <div class="tab-rename"><input aria-label="Tab name" value={renameValue} on:input={(event) => onRenameValue(event.currentTarget.value)} on:keydown={onRenameKeydown} on:blur={onRenameBlur} /></div>
  {:else}
    <button
      bind:this={label}
      data-tab-id={id}
      class="tab-label"
      title={`${title} · ${connectionName}\nDouble-click or F2 to rename. Right-click or Shift+F10 for actions.\nDrag or Alt+Shift+Left/Right to reorder.`}
      aria-pressed={active}
      aria-haspopup="menu"
      on:click={handleClick}
      on:dblclick={() => { if (!suppressClick) onRename(); }}
      on:contextmenu={handleContextMenu}
      on:keydown={handleKeydown}
      on:pointerdown={startDrag}
      on:lostpointercapture={cancelDrag}
      on:dragstart={(event) => event.preventDefault()}
    >
      {#if running}<span class="spinner"></span>{/if}
      {title}{#if pending}<span class="pending-dot" title="Staged edits">•</span>{/if}
      {#if readonly}<small>read only</small>{/if}
    </button>
    <button class="tab-close" aria-label={`Close ${title}`} disabled={closeDisabled} on:click={onClose}><Icon name="close" size={13} /></button>
  {/if}
  <ActionMenu bind:this={actions} contextOnly ariaLabel={`Actions for ${title}`} {items} onSelect={handleAction} />
</div>
