export function formatRunningDuration(startedAt: number | string | null | undefined, nowMs: number, precision: 'seconds' | 'tenths' = 'tenths'): string {
  const startMs = typeof startedAt === 'string' ? Date.parse(startedAt) : startedAt;
  const elapsed = startMs == null || !Number.isFinite(startMs) ? 0 : Math.max(0, nowMs - startMs);
  const seconds = elapsed / 1000;
  return `${precision === 'seconds' ? Math.floor(seconds) : seconds.toFixed(1)}s`;
}
