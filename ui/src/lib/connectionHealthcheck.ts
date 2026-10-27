import { connectionHealth } from './connectionTest.ts';
import type { ConnectionTestState } from './connectionTest.ts';

export interface HealthcheckContext {
  connectionId: string | null;
  state: ConnectionTestState | null;
  ready: boolean;
  busy: boolean;
  transactionIdle: boolean;
}

/** Native focus snapshots are asynchronous. A newer blur/focus event must win
 * over a snapshot requested before it, including a snapshot that fails. */
export function createWindowFocusTracker(
  read: () => Promise<boolean>,
  changed: (focused: boolean) => void
) {
  let revision = 0;
  let disposed = false;
  return {
    set(focused: boolean) {
      revision++;
      if (!disposed) changed(focused);
    },
    async refresh() {
      const request = ++revision;
      const focused = await read().catch(() => false);
      if (!disposed && request === revision) changed(focused);
    },
    dispose() { disposed = true; }
  };
}

/** Only the visible, focused workspace's connection is checked. The probe uses
 * the shared tester, which synchronously marks it testing and deduplicates it. */
export function createConnectionHealthcheck(
  context: () => HealthcheckContext,
  check: (connectionId: string) => void,
  clock = () => Date.now()
) {
  let focused = false;
  let visible = false;
  let disposed = false;

  function poll() {
    if (disposed || !focused || !visible) return;
    const current = context();
    if (!current.ready || current.busy || !current.transactionIdle || !current.connectionId) return;
    const health = connectionHealth(current.state, clock());
    if (health === 'stale' || health === 'unknown') check(current.connectionId);
  }

  return {
    poll,
    setFocused(value: boolean) { focused = value; poll(); },
    setVisible(value: boolean) { visible = value; poll(); },
    dispose() { disposed = true; }
  };
}
