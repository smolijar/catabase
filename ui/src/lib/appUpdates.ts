export interface UpdaterStatus {
  currentVersion: string;
  enabled: boolean;
  unavailableReason: string | null;
}

export interface AvailableUpdate {
  version: string;
  notes: string | null;
  publishedAt: string | null;
}

export interface UpdateCheck {
  currentVersion: string;
  update: AvailableUpdate | null;
}

export interface UpdateProgress {
  phase: 'downloading' | 'installing' | 'restarting';
  downloadedBytes: number;
  totalBytes: number | null;
}

export interface UpdateState {
  status: UpdaterStatus | null;
  update: AvailableUpdate | null;
  checking: boolean;
  checkedAt: number | null;
  error: string;
}

export const UPDATE_CHECK_INTERVAL = 4 * 60 * 60 * 1000;
export const AUTO_UPDATE_CHECK_KEY = 'catabase.checkUpdatesAutomatically';
export const initialUpdateState = (): UpdateState => ({ status: null, update: null, checking: false, checkedAt: null, error: '' });

export function updateInstallBlock(context: {
  ready: boolean; busy: boolean; transactionKnown: boolean; transactionActive: boolean; stagedEdits: number; cellEditing: boolean;
}): string {
  if (!context.ready) return 'Wait for the workspace to finish loading.';
  if (context.busy) return 'Finish the current operation or close its dialog before updating.';
  if (!context.transactionKnown) return 'Transaction status is unavailable. Recheck the connection before updating.';
  if (context.transactionActive) return 'Commit or roll back the open transaction before updating.';
  if (context.stagedEdits) return 'Apply or discard staged cell edits in all tabs before updating.';
  if (context.cellEditing) return 'Finish cell editing in all tabs before updating.';
  return '';
}

/** Coalesce manual/focus/timer checks. Failures use the same retry interval so
 * an offline app never turns a foreground timer into a request loop. */
export function createUpdateChecker(
  api: { status: () => Promise<UpdaterStatus>; check: () => Promise<UpdateCheck> },
  changed: (state: UpdateState) => void,
  describeError: (error: unknown) => string,
  clock = () => Date.now()
) {
  let state = initialUpdateState();
  let statusRequest: Promise<void> | null = null;
  let request: Promise<void> | null = null;
  let lastAttempt: number | null = null;
  let disposed = false;
  const publish = (patch: Partial<UpdateState>) => {
    state = { ...state, ...patch };
    if (!disposed) changed(state);
  };

  function initialize(): Promise<void> {
    if (disposed || state.status) return Promise.resolve();
    if (statusRequest) return statusRequest;
    statusRequest = api.status().then(status => publish({ status, error: '' }))
      .catch(error => publish({ error: describeError(error) }))
      .finally(() => { statusRequest = null; });
    return statusRequest;
  }

  function check(): Promise<void> {
    if (disposed) return Promise.resolve();
    if (request) return request;
    lastAttempt = clock();
    publish({ checking: true, error: '' });
    request = (async () => {
      await initialize();
      if (disposed || !state.status?.enabled) return;
      try {
        const result = await api.check();
        publish({ status: { ...state.status, currentVersion: result.currentVersion }, update: result.update, checkedAt: clock() });
      } catch (error) {
        publish({ error: describeError(error) });
      }
    })().finally(() => { request = null; publish({ checking: false }); });
    return request;
  }

  return {
    initialize,
    check,
    poll(context: { automatic: boolean; foreground: boolean; idle: boolean }) {
      if (disposed || !state.status?.enabled || !context.automatic || !context.foreground || !context.idle) return;
      if (lastAttempt === null || clock() - lastAttempt >= UPDATE_CHECK_INTERVAL) void check();
    },
    dispose() { disposed = true; }
  };
}
