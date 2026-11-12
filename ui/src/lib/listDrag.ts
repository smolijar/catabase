interface ItemBounds { id: string; start: number; end: number }
export interface ListDrop { id: string; side: 'before' | 'after' }

export function listDropTarget(bounds: ItemBounds[], position: number): ListDrop | null {
  if (!bounds.length || !Number.isFinite(position)) return null;
  for (const item of bounds) {
    if (position < (item.start + item.end) / 2) return { id: item.id, side: 'before' };
  }
  return { id: bounds[bounds.length - 1].id, side: 'after' };
}

export function listScrollDelta(start: number, end: number, position: number): number {
  const edge = Math.min(48, (end - start) / 4);
  if (edge <= 0 || !Number.isFinite(position)) return 0;
  if (position < start + edge) return -12 * Math.min(1, (start + edge - position) / edge);
  if (position > end - edge) return 12 * Math.min(1, (position - end + edge) / edge);
  return 0;
}

export function reorderList<T extends { id: string }>(items: T[], movedId: string, drop: ListDrop | null): T[] {
  const moved = items.find((item) => item.id === movedId);
  if (!moved || !drop || movedId === drop.id || !items.some((item) => item.id === drop.id)) return items;
  const remaining = items.filter((item) => item.id !== movedId);
  const index = remaining.findIndex((item) => item.id === drop.id) + (drop.side === 'after' ? 1 : 0);
  const next = [...remaining.slice(0, index), moved, ...remaining.slice(index)];
  return next.every((item, index) => item === items[index]) ? items : next;
}
