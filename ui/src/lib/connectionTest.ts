export type ConnectionTestState =
  | { status: 'testing' }
  | { status: 'success' }
  | { status: 'error'; message: string };

// Tests belong to saved connections, not whichever tab happens to be visible
// when they finish. Invalidating a configuration also invalidates its old test.
export function createConnectionTester(
  onChange: (id: string, state: ConnectionTestState | null) => void,
  describeError: (error: unknown) => string
) {
  const requests = new Map<string, { promise: Promise<void> }>();

  function test(id: string, probe: () => Promise<void>): Promise<void> {
    const existing = requests.get(id);
    if (existing) return existing.promise;

    const request = { promise: Promise.resolve() };
    requests.set(id, request);
    onChange(id, { status: 'testing' });
    request.promise = Promise.resolve().then(probe).then(
      () => { if (requests.get(id) === request) onChange(id, { status: 'success' }); },
      (error) => { if (requests.get(id) === request) onChange(id, { status: 'error', message: describeError(error) }); }
    ).finally(() => {
      if (requests.get(id) === request) requests.delete(id);
    });
    return request.promise;
  }

  function invalidate(id: string) {
    requests.delete(id);
    onChange(id, null);
  }

  return { test, invalidate };
}
