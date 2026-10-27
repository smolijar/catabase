import type { SupermavenCompletion, SupermavenRequest, SupermavenStatus } from './types';

/** A new edit wakes the official agent's lazy connection/reconnection path. */
export function canRequestSupermaven(status: SupermavenStatus): boolean {
  return status.enabled && !status.activationUrl && ['ready', 'connecting', 'disconnected'].includes(status.state);
}

type Snapshot = Omit<SupermavenRequest, 'requestId'>;

/** One active request, cancellable debounce, and no reuse across changed drafts. */
export function createInlineRequester(
  complete: (request: SupermavenRequest) => Promise<SupermavenCompletion | null>,
  cancelRemote: (requestId: string) => Promise<void>,
  requestId = () => crypto.randomUUID(),
  debounceMs = 100
) {
  let revision = 0;
  let active: string | null = null;
  let abortDelay: (() => void) | null = null;
  let abandon: (() => void) | null = null;
  function cancel() {
    revision++;
    abortDelay?.();
    abortDelay = null;
    abandon?.();
    abandon = null;
    if (active) void cancelRemote(active).catch(() => {});
    active = null;
  }
  return {
    cancel,
    async request(snapshot: Snapshot, signal: AbortSignal): Promise<SupermavenCompletion | null> {
      cancel();
      if (signal.aborted || !snapshot.text.trim()) return null;
      const version = revision;
      const onAbort = () => { if (revision === version) cancel(); };
      signal.addEventListener('abort', onAbort, { once: true });
      try {
        await new Promise<void>((resolve) => {
          const timer = setTimeout(() => { abortDelay = null; resolve(); }, debounceMs);
          abortDelay = () => { clearTimeout(timer); resolve(); };
        });
        if (revision !== version || signal.aborted) return null;
        const id = requestId();
        active = id;
        const abandoned = new Promise<null>((resolve) => { abandon = () => resolve(null); });
        const completion = await Promise.race([complete({ ...snapshot, requestId: id }), abandoned]);
        if (revision !== version || signal.aborted) return null;
        active = null;
        return completion;
      } catch { return null; }
      finally {
        signal.removeEventListener('abort', onAbort);
        if (revision === version) { active = null; abandon = null; }
      }
    }
  };
}

/** Supermaven replaces the rest of a line. Never silently remove existing SQL. */
export function inlineEdit(completion: SupermavenCompletion, line: string, column: number) {
  const before = line.slice(0, column - 1);
  const after = line.slice(column - 1);
  const count = completion.deleteBefore;
  if (!completion.text.trim() || !Number.isInteger(count) || count < 0 || count > before.length) return null;
  if (count && !/^[ \t]+$/.test(before.slice(-count))) return null;
  const firstLine = completion.text.split('\n')[0];
  if (after.trim() && !firstLine.endsWith(after)) return null;
  return { text: completion.text, startColumn: column - count, endColumn: line.length + 1 };
}
