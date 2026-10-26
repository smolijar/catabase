<script lang="ts">
  import type { ConnectionRecord } from '../lib/types';
  import { connectionHealth } from '../lib/connectionTest';
  import type { ConnectionTestState } from '../lib/connectionTest';
  import Icon from './Icon.svelte';

  export let connection: ConnectionRecord | null = null;
  export let state: ConnectionTestState | null = null;
  export let disabled = false;
  export let compact = false;
  export let switcher = false;
  export let environment = '';
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
  disabled={disabled || (testing && !switcher)}
  aria-busy={testing}
  aria-label={switcher ? `Switch connection${connection ? `: ${connection.name}` : ''}` : connection ? `${testing ? 'Testing' : 'Test'} ${connection.name}${stale ? ': check is stale' : error ? `: ${error}` : ''}` : 'Choose connection'}
  aria-haspopup={switcher ? 'dialog' : undefined}
  title={switcher ? `${connection?.name ?? 'Choose connection'} · Switch connection${error ? `\n${error}` : ''}` : title}
  on:click={() => switcher || !connection ? onChoose() : onTest()}
>
  <span class="status-dot"></span>
  <span class="connection-name">{connection?.name ?? 'Choose connection'}</span>
  {#if switcher && connection}<span class="connection-user">{connection.username}</span>{/if}
  {#if switcher && environment}<span class="environment">{environment}</span>{/if}
  {#if testing || stale || (!compact && error)}<span class="connection-message" role="status">{testing ? 'Testing…' : stale ? 'Check stale' : error}</span>{/if}
  {#if switcher}<span class="chevron"><Icon name="chevron" size={12} /></span>{/if}
</button>

<style>
  .connection-chip { flex: 0 1 auto; min-width: 0; max-width: min(560px, 42vw); background: var(--surface-1); }
  .connection-chip.compact { max-width: min(260px, 23vw); min-height: 22px; padding: 2px 4px; border-color: transparent; border-radius: 0; background: transparent; font-size: inherit; box-shadow: none; }
  .compact .connection-message { font-size: inherit; }
  .connection-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .connection-message { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; }
  .connection-user { color: var(--ink-muted); font: 11px var(--font-mono); max-width: 100px; overflow: hidden; text-overflow: ellipsis; }
  .environment { color: var(--accent); background: var(--accent-soft); border-radius: 2px; padding: 0 4px; font: 11px/16px var(--font-mono); text-transform: uppercase; }
  .chevron { transform: rotate(90deg); color: var(--ink-faint); }
  .connection-name:has(+ .connection-message) { flex-shrink: 0; max-width: 200px; }
  .status-dot { flex-shrink: 0; }
  .connection-chip.connected { color: var(--ink); }
  .connection-chip.testing { color: var(--warn); opacity: 1; }
  .testing .status-dot { background: var(--warn); animation: busy 1.06s steps(1) infinite; }
  .connection-chip.stale { color: var(--warn); }
  .stale .status-dot { background: var(--warn); }
  .connection-chip.failed { border-color: var(--danger); background: var(--danger-soft); color: var(--danger); }
  .failed .status-dot { background: var(--danger); }
  @keyframes busy { 50% { opacity: .25; } }
  @media (max-width: 1000px) { .connection-user { display: none; } }
</style>
