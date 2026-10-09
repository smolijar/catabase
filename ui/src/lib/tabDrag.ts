export interface TabBounds { id: string; left: number; right: number }
export interface TabDrop { id: string; side: 'before' | 'after' }

export function tabDropTarget(bounds: TabBounds[], x: number): TabDrop | null {
  if (!bounds.length || !Number.isFinite(x)) return null;
  for (const tab of bounds) {
    if (x < (tab.left + tab.right) / 2) return { id: tab.id, side: 'before' };
  }
  return { id: bounds[bounds.length - 1].id, side: 'after' };
}

export function tabScrollDelta(left: number, right: number, x: number): number {
  const edge = Math.min(48, (right - left) / 4);
  if (edge <= 0 || !Number.isFinite(x)) return 0;
  if (x < left + edge) return -12 * Math.min(1, (left + edge - x) / edge);
  if (x > right - edge) return 12 * Math.min(1, (x - right + edge) / edge);
  return 0;
}

// Move existing objects, never recreate their editor/results/staged-edit state.
export function reorderTabs<T extends { id: string }>(tabs: T[], movedId: string, drop: TabDrop | null): T[] {
  const moved = tabs.find((tab) => tab.id === movedId);
  if (!moved || !drop || movedId === drop.id || !tabs.some((tab) => tab.id === drop.id)) return tabs;
  const remaining = tabs.filter((tab) => tab.id !== movedId);
  const index = remaining.findIndex((tab) => tab.id === drop.id) + (drop.side === 'after' ? 1 : 0);
  const next = [...remaining.slice(0, index), moved, ...remaining.slice(index)];
  return next.every((tab, index) => tab === tabs[index]) ? tabs : next;
}
