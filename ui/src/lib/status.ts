export function formatRunningDuration(
  startedAt: number | string | null | undefined,
  nowMs: number,
  precision: 'adaptive' | 'tenths' = 'tenths'
): string {
  const startMs = typeof startedAt === 'string' ? Date.parse(startedAt) : startedAt;
  const elapsedMs = startMs == null || !Number.isFinite(startMs) ? 0 : Math.max(0, nowMs - startMs);
  if (precision === 'tenths') return `${(elapsedMs / 1000).toFixed(1)}s`;
  if (elapsedMs < 10_000) return `${(Math.floor(elapsedMs / 10) / 100).toFixed(2)}s`;
  return `${Math.floor(elapsedMs / 1000)}s`;
}
