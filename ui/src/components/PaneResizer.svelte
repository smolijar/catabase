<script lang="ts">
  import { onDestroy } from 'svelte';
  import { clampPaneWidth } from '../lib/sidebarLayout';
  import type { PaneWidthBounds } from '../lib/sidebarLayout';

  export let width: number;
  export let defaultWidth: number;
  export let bounds: PaneWidthBounds;
  export let label: string;
  export let controls: string;
  export let side: 'left' | 'right' = 'left';
  export let resizing = false;
  export let onResizeEnd: (width: number) => void = () => {};
  let handle: HTMLDivElement;
  let drag: { pointerId: number; x: number; width: number } | null = null;

  $: currentWidth = clampPaneWidth(width, bounds, defaultWidth);
  $: direction = side === 'left' ? 1 : -1;

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
    width = clampPaneWidth(drag.width + (event.clientX - drag.x) * direction, bounds, defaultWidth);
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
    if (event.key === 'ArrowLeft') next = currentWidth - 24 * direction;
    else if (event.key === 'ArrowRight') next = currentWidth + 24 * direction;
    else if (event.key === 'Home') next = bounds.min;
    else if (event.key === 'End') next = bounds.max;
    else return;
    event.preventDefault();
    event.stopPropagation();
    width = clampPaneWidth(next, bounds, defaultWidth);
    onResizeEnd(width);
  }

  function resetWidth() {
    stopResize();
    width = defaultWidth;
    onResizeEnd(width);
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions a11y_no_noninteractive_tabindex -->
<div
  bind:this={handle}
  class="pane-resizer"
  class:resizing
  role="separator"
  aria-label={label}
  aria-controls={controls}
  aria-orientation="vertical"
  aria-valuemin={bounds.min}
  aria-valuemax={bounds.max}
  aria-valuenow={currentWidth}
  aria-valuetext={`${currentWidth} pixels`}
  title="Drag to resize. Left and Right arrows resize. Home and End set limits. Double-click to reset."
  tabindex="0"
  on:pointerdown={startResize}
  on:lostpointercapture={stopResize}
  on:keydown={resizeWithKeyboard}
  on:dblclick={resetWidth}
><span></span></div>

<style>
  .pane-resizer {
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

  .pane-resizer:hover span,
  .pane-resizer:focus-visible span,
  .pane-resizer.resizing span {
    height: 54px;
    background: var(--accent);
  }

</style>
