export const DEFAULT_SIDEBAR_WIDTH = 280;
export const DEFAULT_CELL_DETAILS_WIDTH = 330;
const MIN_SIDEBAR_WIDTH = 220;
const MAX_SIDEBAR_WIDTH = 640;
const MIN_CELL_DETAILS_WIDTH = 220;
const MAX_CELL_DETAILS_WIDTH = 720;

export interface PaneWidthBounds { min: number; max: number }

export function sidebarWidthBounds(workspaceWidth: number): PaneWidthBounds {
  const width = Number.isFinite(workspaceWidth) ? Math.max(0, workspaceWidth) : 1280;
  // Below 820px the sidebar overlays the editor instead of sharing its width.
  const editorWidth = width <= 820 ? 0 : 480;
  const max = Math.floor(Math.max(0, Math.min(MAX_SIDEBAR_WIDTH, width - 44 - 5 - editorWidth)));
  return { min: Math.min(MIN_SIDEBAR_WIDTH, max), max };
}

export function clampSidebarWidth(preferredWidth: number, workspaceWidth: number): number {
  return clampPaneWidth(preferredWidth, sidebarWidthBounds(workspaceWidth), DEFAULT_SIDEBAR_WIDTH);
}

export function cellDetailsWidthBounds(gridWidth: number): PaneWidthBounds {
  const width = Number.isFinite(gridWidth) ? gridWidth : 1000;
  // The inspector overlays the results at this breakpoint, leaving an edge of the grid exposed.
  const gridSpace = width <= 640 ? 48 : 240;
  const max = Math.floor(Math.max(0, Math.min(MAX_CELL_DETAILS_WIDTH, width - gridSpace)));
  return { min: Math.min(MIN_CELL_DETAILS_WIDTH, max), max };
}

export function clampPaneWidth(preferredWidth: number, { min, max }: PaneWidthBounds, defaultWidth: number): number {
  const width = Number.isFinite(preferredWidth) ? preferredWidth : defaultWidth;
  return Math.round(Math.min(max, Math.max(min, width)));
}

export function restoreSidebarWidth(saved: string | null): number {
  return restorePaneWidth(saved, { min: MIN_SIDEBAR_WIDTH, max: MAX_SIDEBAR_WIDTH }, DEFAULT_SIDEBAR_WIDTH);
}

export function restoreCellDetailsWidth(saved: string | null): number {
  return restorePaneWidth(saved, { min: MIN_CELL_DETAILS_WIDTH, max: MAX_CELL_DETAILS_WIDTH }, DEFAULT_CELL_DETAILS_WIDTH);
}

function restorePaneWidth(saved: string | null, bounds: PaneWidthBounds, defaultWidth: number): number {
  const width = Number(saved);
  return Number.isFinite(width) && width > 0
    ? clampPaneWidth(width, bounds, defaultWidth)
    : defaultWidth;
}
