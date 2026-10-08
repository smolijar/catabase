export function formatRunningDuration(startedAt: number | string | null | undefined, nowMs: number): string {
  const startMs = typeof startedAt === 'string' ? Date.parse(startedAt) : startedAt;
  const elapsed = startMs == null || !Number.isFinite(startMs) ? 0 : Math.max(0, nowMs - startMs);
  return `${(elapsed / 1000).toFixed(1)}s`;
}
