export function formatRunningDuration(startedAt: number | null | undefined, nowMs: number): string {
  const elapsed = startedAt == null ? 0 : Math.max(0, nowMs - startedAt);
  return `${(elapsed / 1000).toFixed(1)}s`;
}
