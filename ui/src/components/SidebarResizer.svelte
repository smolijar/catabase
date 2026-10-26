<script lang="ts">
  import { onDestroy } from 'svelte';
  import { clampSidebarWidth, DEFAULT_SIDEBAR_WIDTH, sidebarWidthBounds } from '../lib/sidebarLayout';

  export let width = DEFAULT_SIDEBAR_WIDTH;
  export let workspaceWidth = 1280;
  export let resizing = false;
  export let onResizeEnd: (width: number) => void = () => {};
  let handle: HTMLDivElement;
  let drag: { pointerId: number; x: number; width: number } | null = null;

  $: bounds = sidebarWidthBounds(workspaceWidth);
  $: currentWidth = clampSidebarWidth(width, workspaceWidth);

  onDestroy(stopResize);

  function startResize(event: PointerEvent) {
    if (event.button !== 0 || drag) return;
    event.preventDefault();
    handle.focus({ preventScroll: true });
    drag = { pointerId: event.pointerId, x: event.clientX, width: currentWidth };
    resizing = true;
    handle.setPointerCapture(event.pointerId);
    window.addEventListener('pointermove', resize);
    window.addEventListener('pointerup', finishPointerResize);
    window.addEventListener('pointercancel', finishPointerResize);
    window.addEventListener('mouseup', stopResize);
    window.addEventListener('blur', stopResize);
  }

  function resize(event: PointerEvent) {
    if (!drag || event.pointerId !== drag.pointerId) return;
    width = clampSidebarWidth(drag.width + event.clientX - drag.x, workspaceWidth);
  }

  function finishPointerResize(event: PointerEvent) {
    if (drag && event.pointerId === drag.pointerId) stopResize();
  }

  function stopResize() {
    if (!drag) return;
    const pointerId = drag.pointerId;
    drag = null;
    resizing = false;
    window.removeEventListener('pointermove', resize);
    window.removeEventListener('pointerup', finishPointerResize);
    window.removeEventListener('pointercancel', finishPointerResize);
    window.removeEventListener('mouseup', stopResize);
    window.removeEventListener('blur', stopResize);
    if (handle?.hasPointerCapture(pointerId)) handle.releasePointerCapture(pointerId);
    onResizeEnd(width);
  }

  function resizeWithKeyboard(event: KeyboardEvent) {
    if (event.isComposing || event.metaKey || event.ctrlKey || event.altKey) return;
    let next: number;
    if (event.key === 'ArrowLeft') next = currentWidth - 24;
    else if (event.key === 'ArrowRight') next = currentWidth + 24;
    else if (event.key === 'Home') next = bounds.min;
    else if (event.key === 'End') next = bounds.max;
    else return;
    event.preventDefault();
    event.stopPropagation();
    width = clampSidebarWidth(next, workspaceWidth);
    onResizeEnd(width);
  }

  function resetWidth() {
    stopResize();
    width = DEFAULT_SIDEBAR_WIDTH;
    onResizeEnd(width);
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions a11y_no_noninteractive_tabindex -->
<div
  bind:this={handle}
  class="sidebar-resizer"
  class:resizing
  role="separator"
  aria-label="Resize sidebar"
  aria-controls="workspace-sidebar"
  aria-orientation="vertical"
  aria-valuemin={bounds.min}
  aria-valuemax={bounds.max}
  aria-valuenow={currentWidth}
  aria-valuetext={`${currentWidth} pixels`}
  title="Drag to resize sidebar. Left/Right arrows resize; Home/End set limits. Double-click to reset."
  tabindex="0"
  on:pointerdown={startResize}
  on:lostpointercapture={stopResize}
  on:keydown={resizeWithKeyboard}
  on:dblclick={resetWidth}
><span></span></div>

<style>
  .sidebar-resizer {
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 0;
    border-right: 1px solid var(--line);
    background: var(--surface-1);
    cursor: col-resize;
    touch-action: none;
    outline-offset: -1px;
  }

  span {
    width: 3px;
    height: 32px;
    border-radius: 2px;
    background: var(--line-strong);
    transition: background 120ms ease, height 120ms ease;
    pointer-events: none;
  }

  .sidebar-resizer:hover span,
  .sidebar-resizer:focus-visible span,
  .sidebar-resizer.resizing span {
    height: 54px;
    background: var(--accent);
  }

  @media (max-width: 820px) {
    .sidebar-resizer {
      position: absolute;
      z-index: 6;
      top: 0;
      bottom: 0;
      left: calc(44px + var(--sidebar-width, 280px));
      width: 5px;
    }
  }
</style>
