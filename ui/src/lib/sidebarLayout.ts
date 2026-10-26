export const DEFAULT_SIDEBAR_WIDTH = 280;
const MIN_SIDEBAR_WIDTH = 220;
const MAX_SIDEBAR_WIDTH = 640;

export function sidebarWidthBounds(workspaceWidth: number): { min: number; max: number } {
  const width = Number.isFinite(workspaceWidth) ? Math.max(0, workspaceWidth) : 1280;
  // Below 820px the sidebar overlays the editor instead of sharing its width.
  const editorWidth = width <= 820 ? 0 : 480;
  const max = Math.floor(Math.max(0, Math.min(MAX_SIDEBAR_WIDTH, width - 44 - 5 - editorWidth)));
  return { min: Math.min(MIN_SIDEBAR_WIDTH, max), max };
}

export function clampSidebarWidth(preferredWidth: number, workspaceWidth: number): number {
  const { min, max } = sidebarWidthBounds(workspaceWidth);
  const width = Number.isFinite(preferredWidth) ? preferredWidth : DEFAULT_SIDEBAR_WIDTH;
  return Math.round(Math.min(max, Math.max(min, width)));
}

export function restoreSidebarWidth(saved: string | null): number {
  if (saved === null || !saved.trim()) return DEFAULT_SIDEBAR_WIDTH;
  const width = Number(saved);
  return Number.isFinite(width) && width > 0
    ? Math.round(Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, width)))
    : DEFAULT_SIDEBAR_WIDTH;
}
