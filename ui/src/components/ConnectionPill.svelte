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
  export let activity: 'running' | 'cancelling' | null = null;
  export let environment = '';
  export let now = Date.now();
  export let onTest: () => void;
  export let onChoose: () => void;

  $: health = connectionHealth(state, now);
  $: testing = health === 'testing';
  $: running = Boolean(activity) || health === 'running';
  $: busy = testing || running;
  $: message = running ? activity === 'cancelling' ? 'Cancelling…' : 'Running…' : testing ? 'Testing…' : stale ? 'Check stale' : error;
  $: stale = health === 'stale';
  $: error = state?.status === 'error' ? state.message : '';
  $: title = !connection ? 'Choose this tab’s database connection'
    : running ? `${connection.name}: ${activity === 'cancelling' ? 'cancelling the query' : 'query running'}. Wait for it to finish before testing.`
    : testing ? `Testing ${connection.name}…`
    : stale ? `${connection.name}: connection check is over one minute old.${error ? ` Last error: ${error}` : ''} Click to check again, or run a query.`
    : error ? `${connection.name}: ${error}\nClick to test again.`
    : state?.status === 'success' ? `${connection.name}: connection test succeeded. Click to test again.`
    : `Test ${connection.name}. Click to check the connection.`;
</script>

<div class="connection-control" class:split={switcher} class:compact>
<button
  class="chip connection-chip"
  class:connected={health === 'success' && !running}
  class:compact
  class:stale={stale && !running}
  class:testing={busy}
  class:failed={health === 'error' && !running}
  disabled={disabled || busy}
  aria-busy={busy}
  aria-label={connection ? `${running ? activity === 'cancelling' ? 'Cancelling on' : 'Running on' : testing ? 'Testing' : 'Test'} ${connection.name}${!running && stale ? ': check is stale' : !running && error ? `: ${error}` : ''}` : 'Choose connection'}
  {title}
  on:click={() => connection ? onTest() : onChoose()}
>
  <span class="status-dot"></span>
  <span class="connection-name">{connection?.name ?? 'Choose connection'}</span>
  {#if switcher && connection}<span class="connection-user">{connection.username}</span>{/if}
  {#if switcher && environment}<span class="environment">{environment}</span>{/if}
  {#if busy || stale || (!compact && error)}<span class="connection-message" role="status">{message}</span>{/if}
</button>
{#if switcher}<button class="switch-arrow" aria-label="Switch connection" aria-haspopup="dialog" title="Switch or disconnect connection" disabled={disabled || running} on:click={onChoose}><span class="chevron"><Icon name="chevron" size={12} /></span></button>{/if}
</div>

<style>
  .connection-control { display: inline-flex; flex: 0 1 auto; min-width: 0; max-width: min(560px, 42vw); }
  .connection-control.compact { max-width: min(260px, 23vw); }
  .connection-chip { flex: 0 1 auto; min-width: 0; max-width: 100%; background: var(--surface-1); }
  .split .connection-chip { border-radius: 4px 0 0 4px; border-right: 0; }
  .switch-arrow { flex-shrink: 0; width: 28px; padding: 0; border-radius: 0 4px 4px 0; background: var(--surface-1); }
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
