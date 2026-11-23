import type { AppError } from './types';

export type ConnectionTestState =
  | { status: 'testing' }
  | { status: 'running' }
  | { status: 'success'; checkedAt: number }
  | { status: 'error'; origin: 'test' | 'query'; message: string; checkedAt: number };

export function connectionHealth(state: ConnectionTestState | null, now = Date.now()) {
  if (!state) return 'unknown';
  if (state.status === 'testing' || state.status === 'running') return state.status;
  return now - state.checkedAt >= 60_000 ? 'stale' : state.status;
}

// PostgreSQL statement errors (including cancellation) prove the connection
// worked; failed authentication, transport errors and FATAL responses do not.
export function queryReachedServer(error: unknown): boolean {
  if (!error || typeof error !== 'object') return false;
  const { code, severity } = error as { code?: string; severity?: string };
  return severity === 'ERROR' && typeof code === 'string' && /^[0-9A-Z]{5}$/.test(code) && !code.startsWith('08');
}

export function queryLostConnection(error: unknown): boolean {
  const { code, severity, message, connectionFailure } = (error ?? {}) as Partial<AppError>;
  if (connectionFailure || code?.startsWith('08') || severity === 'FATAL' || severity === 'PANIC') return true;
  // Server and local validation messages can contain arbitrary SQL or names.
  // The fallback only recognizes transport errors without structured diagnostics.
  return connectionFailure === undefined && !code && !severity
    && /error connecting to server|error communicating with the server|connection (?:refused|closed|reset|timed out)|authentication failed|server closed|broken pipe|network is unreachable|tls handshake|ssl connection/i.test(message ?? '');
}

// Tests belong to saved connections, not whichever tab happens to be visible
// when they finish. Invalidating a configuration also invalidates its old test.
export function createConnectionTester(
  onChange: (id: string, state: ConnectionTestState | null) => void,
  describeError: (error: unknown) => string,
  clock = () => Date.now(),
  onConnectionLost?: (id: string) => void
) {
  const requests = new Map<string, { promise: Promise<void> }>();
  const settledStates = new Map<string, Extract<ConnectionTestState, { checkedAt: number }>>();

  function setStatus(id: string, state: ConnectionTestState | null) {
    if (!state) settledStates.delete(id);
    else if ('checkedAt' in state) settledStates.set(id, state);
    onChange(id, state);
  }

  function test(id: string, probe: () => Promise<void>, onSuccess?: () => void): Promise<void> {
    const existing = requests.get(id);
    if (existing) return existing.promise;

    const request = { promise: Promise.resolve() };
    requests.set(id, request);
    setStatus(id, { status: 'testing' });
    request.promise = Promise.resolve().then(probe).then(
      () => {
        if (requests.get(id) !== request) return;
        setStatus(id, { status: 'success', checkedAt: clock() });
        onSuccess?.();
      },
      (error) => {
        if (requests.get(id) !== request) return;
        setStatus(id, { status: 'error', origin: 'test', message: describeError(error), checkedAt: clock() });
        onConnectionLost?.(id);
      }
    ).finally(() => {
      if (requests.get(id) === request) requests.delete(id);
    });
    return request.promise;
  }

  function invalidate(id: string) {
    requests.delete(id);
    setStatus(id, null);
  }

  async function query<T>(id: string, execute: () => Promise<T>): Promise<T> {
    // Query traffic is the check itself: no extra DB round trip. A newer query
    // or configuration invalidation supersedes older responses.
    const request = { promise: Promise.resolve() };
    requests.set(id, request);
    setStatus(id, { status: 'running' });
    try {
      const result = await execute();
      if (requests.get(id) === request) setStatus(id, { status: 'success', checkedAt: clock() });
      return result;
    } catch (error) {
      if (requests.get(id) === request) {
        if (queryLostConnection(error)) {
          setStatus(id, { status: 'error', origin: 'query', message: describeError(error), checkedAt: clock() });
          onConnectionLost?.(id);
        } else if (queryReachedServer(error)) {
          setStatus(id, { status: 'success', checkedAt: clock() });
        } else {
          // Local errors leave both the last settled check and its age intact.
          setStatus(id, settledStates.get(id) ?? null);
        }
      }
      throw error;
    } finally {
      if (requests.get(id) === request) requests.delete(id);
    }
  }

  return { test, query, invalidate };
}
