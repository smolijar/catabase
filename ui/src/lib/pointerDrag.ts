interface DragHandlers {
  enabled?: boolean;
  onStart: (x: number, y: number) => void;
  onMove: (x: number, y: number) => void;
  onEnd: (cancelled: boolean) => void;
}

export function pointerDrag(node: HTMLButtonElement, handlers: DragHandlers) {
  let drag: { pointerId: number; x: number; y: number; started: boolean } | null = null;
  let suppressClick = false;

  function start(event: PointerEvent) {
    if (event.button !== 0 || !event.isPrimary || drag) return;
    suppressClick = false;
    if (handlers.enabled === false || event.metaKey || event.ctrlKey || event.altKey) return;
    drag = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, started: false };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', endPointer);
    window.addEventListener('pointercancel', cancelPointer);
    window.addEventListener('mouseup', endMouse);
    window.addEventListener('blur', cancel);
    window.addEventListener('keydown', cancelWithEscape);
  }

  function move(event: PointerEvent) {
    if (!drag || drag.pointerId !== event.pointerId) return;
    const { clientX: x, clientY: y } = event;
    if (!drag.started && Math.hypot(x - drag.x, y - drag.y) < 6) return;
    event.preventDefault();
    if (drag.started) {
      handlers.onMove(x, y);
    } else {
      drag.started = true;
      node.setPointerCapture(event.pointerId);
      handlers.onStart(x, y);
    }
  }

  function endPointer(event: PointerEvent) {
    if (!drag || drag.pointerId !== event.pointerId) return;
    if (drag.started) handlers.onMove(event.clientX, event.clientY);
    finish(false);
  }

  function cancelPointer(event: PointerEvent) {
    if (drag?.pointerId === event.pointerId) finish(true);
  }

  function endMouse() { finish(false); }
  function cancel() { finish(true); }

  function cancelWithEscape(event: KeyboardEvent) {
    if (event.key !== 'Escape' || !drag?.started) return;
    event.preventDefault();
    event.stopPropagation();
    finish(true);
  }

  function finish(cancelled: boolean) {
    if (!drag) return;
    const { pointerId, started } = drag;
    drag = null;
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', endPointer);
    window.removeEventListener('pointercancel', cancelPointer);
    window.removeEventListener('mouseup', endMouse);
    window.removeEventListener('blur', cancel);
    window.removeEventListener('keydown', cancelWithEscape);
    if (node.hasPointerCapture(pointerId)) node.releasePointerCapture(pointerId);
    if (started) {
      suppressClick = true;
      handlers.onEnd(cancelled);
    }
  }

  function preventNativeDrag(event: DragEvent) { event.preventDefault(); }

  function preventDragClick(event: MouseEvent) {
    if (!suppressClick || event.detail === 0) return;
    event.preventDefault();
    event.stopImmediatePropagation();
  }

  node.addEventListener('pointerdown', start);
  node.addEventListener('lostpointercapture', cancelPointer);
  node.addEventListener('dragstart', preventNativeDrag);
  node.addEventListener('click', preventDragClick, true);
  node.addEventListener('dblclick', preventDragClick, true);

  return {
    update(next: DragHandlers) {
      handlers = next;
      if (handlers.enabled === false) cancel();
    },
    destroy() {
      cancel();
      node.removeEventListener('pointerdown', start);
      node.removeEventListener('lostpointercapture', cancelPointer);
      node.removeEventListener('dragstart', preventNativeDrag);
      node.removeEventListener('click', preventDragClick, true);
      node.removeEventListener('dblclick', preventDragClick, true);
    }
  };
}
