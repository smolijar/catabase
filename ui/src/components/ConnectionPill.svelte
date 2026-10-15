<script lang="ts">
  import type { ConnectionRecord } from '../lib/types';
  import { connectionHealth } from '../lib/connectionTest';
  import type { ConnectionTestState } from '../lib/connectionTest';

  export let connection: ConnectionRecord | null = null;
  export let state: ConnectionTestState | null = null;
  export let disabled = false;
  export let compact = false;
  export let now = Date.now();
  export let onTest: () => void;
  export let onChoose: () => void;

  $: health = connectionHealth(state, now);
  $: testing = health === 'testing';
  $: stale = health === 'stale';
  $: error = state?.status === 'error' ? state.message : '';
  $: title = !connection ? 'Choose this tab’s database connection'
    : testing ? `Testing ${connection.name}…`
    : stale ? `${connection.name}: connection check is over one minute old.${error ? ` Last error: ${error}` : ''} Click to check again, or run a query.`
    : error ? `${connection.name}: ${error}\nClick to test again.`
    : state?.status === 'success' ? `${connection.name}: connection test succeeded. Click to test again.`
    : `Test ${connection.name}. Click to check the connection.`;
</script>

<button
  class="chip connection-chip"
  class:connected={health === 'success'}
  class:compact
  class:stale
  class:testing
  class:failed={health === 'error'}
  disabled={disabled || testing}
  aria-busy={testing}
  aria-label={connection ? `${testing ? 'Testing' : 'Test'} ${connection.name}${stale ? ': check is stale' : error ? `: ${error}` : ''}` : 'Choose connection'}
  {title}
  on:click={() => connection ? onTest() : onChoose()}
>
  <span class="status-dot"></span>
  <span class="connection-name">{connection?.name ?? 'Choose connection'}</span>
  {#if testing || stale || (!compact && error)}<span class="connection-message" role="status">{testing ? 'Testing…' : stale ? 'Check stale' : error}</span>{/if}
</button>

<style>
  .connection-chip { flex: 0 1 auto; min-width: 0; max-width: min(560px, 42vw); background: #0f172a; }
  .connection-chip.compact { max-width: min(360px, 25vw); min-height: 23px; padding: 2px 5px; border-color: transparent; border-radius: 4px; background: transparent; font-size: inherit; box-shadow: none; }
  .compact .connection-message { font-size: inherit; }
  .connection-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .connection-message { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: .72rem; }
  .connection-name:has(+ .connection-message) { flex-shrink: 0; max-width: 200px; }
  .status-dot { flex-shrink: 0; }
  .connection-chip.connected { border-color: #285e59; color: #d5fbf5; }
  .connection-chip.testing { border-color: #9f7938; color: #ffe1a0; opacity: 1; cursor: progress; }
  .testing .status-dot { background: #f0c56b; box-shadow: none; }
  .connection-chip.stale { border-color: #9f7938; color: #ffe1a0; }
  .stale .status-dot { background: #f0c56b; box-shadow: none; }
  .connection-chip.failed { border-color: #ad4c59; background: #341b29; color: #ffc1c9; }
  .connection-chip.failed:hover:not(:disabled) { border-color: #f08a99; background: #472333; color: #ffe0e5; }
  .failed .status-dot { background: #ff7e91; box-shadow: none; }
</style>
