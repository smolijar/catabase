export type ConnectionTestState =
  | { status: 'testing' }
  | { status: 'running' }
  | { status: 'success'; checkedAt: number }
  | { status: 'error'; message: string; checkedAt: number };

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

// Tests belong to saved connections, not whichever tab happens to be visible
// when they finish. Invalidating a configuration also invalidates its old test.
export function createConnectionTester(
  onChange: (id: string, state: ConnectionTestState | null) => void,
  describeError: (error: unknown) => string,
  clock = () => Date.now()
) {
  const requests = new Map<string, { promise: Promise<void> }>();

  function test(id: string, probe: () => Promise<void>): Promise<void> {
    const existing = requests.get(id);
    if (existing) return existing.promise;

    const request = { promise: Promise.resolve() };
    requests.set(id, request);
    onChange(id, { status: 'testing' });
    request.promise = Promise.resolve().then(probe).then(
      () => { if (requests.get(id) === request) onChange(id, { status: 'success', checkedAt: clock() }); },
      (error) => { if (requests.get(id) === request) onChange(id, { status: 'error', message: describeError(error), checkedAt: clock() }); }
    ).finally(() => {
      if (requests.get(id) === request) requests.delete(id);
    });
    return request.promise;
  }

  function invalidate(id: string) {
    requests.delete(id);
    onChange(id, null);
  }

  async function query<T>(id: string, execute: () => Promise<T>): Promise<T> {
    // Query traffic is the check itself: no extra DB round trip. A newer query
    // or configuration invalidation supersedes older responses.
    const request = { promise: Promise.resolve() };
    requests.set(id, request);
    onChange(id, { status: 'running' });
    try {
      const result = await execute();
      if (requests.get(id) === request) onChange(id, { status: 'success', checkedAt: clock() });
      return result;
    } catch (error) {
      if (requests.get(id) === request) onChange(id, queryReachedServer(error)
        ? { status: 'success', checkedAt: clock() }
        : { status: 'error', message: describeError(error), checkedAt: clock() });
      throw error;
    } finally {
      if (requests.get(id) === request) requests.delete(id);
    }
  }

  return { test, query, invalidate };
}
