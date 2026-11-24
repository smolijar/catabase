<script lang="ts">
  import type { ConnectionRecord } from '../lib/types';
  import { connectionHealth } from '../lib/connectionTest';
  import type { ConnectionTestState } from '../lib/connectionTest';
  import Icon from './Icon.svelte';

  export let connection: ConnectionRecord | null = null;
  export let online = Boolean(connection);
  export let state: ConnectionTestState | null = null;
  export let disabled = false;
  export let compact = false;
  export let switcher = false;
  export let connectionLocked = false;
  export let lockedConnectionName = '';
  export let activity: 'running' | 'cancelling' | null = null;
  export let environment = '';
  export let now = Date.now();
  export let onTest: () => void;
  export let onChoose: () => void;

  $: health = connectionHealth(state, now);
  $: testing = health === 'testing';
  $: running = Boolean(activity) || health === 'running';
  $: busy = testing || running;
  $: disconnected = !online || (!running && state?.status === 'error' && state.origin === 'query');
  $: stale = !disconnected && health === 'stale';
  $: testError = state?.status === 'error' && state.origin === 'test' ? state.message : '';
  $: showTestError = Boolean(testError) && (health === 'error' || !online);
  $: message = testing ? 'Testing…' : stale ? 'Check stale'
    : disconnected && connection ? `${connection.name}: ${testError}` : testError;
  $: lockReason = `Connection locked to ${lockedConnectionName || connection?.name || 'this tab'}. Right-click the tab to unlock.`;
  $: title = !connection ? connectionLocked ? `${lockReason} Run a query to review and reconnect.` : 'Choose this tab’s database connection'
    : running ? `${connection.name}: ${activity === 'cancelling' ? 'cancelling the query' : 'query running'}. Wait for it to finish before testing.`
    : testing ? `Testing ${connection.name}…`
    : stale ? `${connection.name}: connection check is over one minute old.${testError ? ` Last error: ${testError}` : ''} Click to check again, or run a query.`
    : testError ? `${connection.name}: ${testError}\nClick to test again.`
    : state?.status === 'success' ? `${connection.name}: connection test succeeded. Click to test again.`
    : `Test ${connection.name}. Click to check the connection.`;
</script>

<div class="connection-control" class:split-button={switcher} class:compact>
<button
  class="chip connection-chip"
  class:split-button-main={switcher}
  class:connected={!disconnected && health === 'success' && !running}
  class:compact
  class:stale={stale && !running}
  class:testing={testing && !running}
  class:running
  class:failed={(health === 'error' || showTestError) && !running}
  tabindex={compact ? -1 : undefined}
  disabled={disabled || busy || (!connection && connectionLocked)}
  aria-busy={busy}
  aria-label={connection ? `${running ? activity === 'cancelling' ? 'Cancelling on' : 'Running on' : testing ? 'Testing' : 'Test'} ${connection.name}${!running && stale ? ': check is stale' : !running && testError ? `: ${testError}` : ''}` : 'Disconnected. Choose connection'}
  {title}
  on:click={() => connection ? onTest() : onChoose()}
>
  <span class="status-dot"></span>
  <span class="connection-name">{disconnected ? 'Disconnected' : connection?.name ?? 'Disconnected'}</span>
  {#if switcher && connection && !disconnected}<span class="connection-user">{connection.username}</span>{/if}
  {#if switcher && environment && !disconnected}<span class="environment">{environment}</span>{/if}
  {#if !running && (testing || stale || (!compact && showTestError))}<span class="connection-message" role="status">{message}</span>{/if}
</button>
{#if switcher}<button class="switch-arrow split-button-secondary" aria-label={connectionLocked ? lockReason : 'Switch connection'} aria-haspopup="dialog" title={connectionLocked ? lockReason : 'Switch or disconnect connection'} disabled={disabled || running || connectionLocked} on:click={onChoose}><Icon name={connectionLocked ? 'lock' : 'chevron-down'} size={12} /></button>{/if}
</div>

<style>
  .connection-control { display: inline-flex; flex: 0 1 auto; min-width: 0; max-width: min(560px, 42vw); }
  .connection-control.compact { max-width: min(260px, 23vw); }
  .connection-chip { flex: 0 1 auto; min-width: 0; max-width: 100%; background: var(--surface-1); }
  .connection-chip.compact { max-width: min(260px, 23vw); min-height: 22px; padding: 2px 4px; border-color: transparent; border-radius: 0; background: transparent; font-size: inherit; box-shadow: none; }
  .compact .connection-message { font-size: inherit; }
  .connection-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .connection-message { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; }
  .connection-user { color: var(--ink-muted); font: 11px var(--font-mono); max-width: 100px; overflow: hidden; text-overflow: ellipsis; }
  .environment { color: var(--accent); background: var(--accent-soft); border-radius: 2px; padding: 0 4px; font: 11px/16px var(--font-mono); text-transform: uppercase; }
  .connection-name:has(+ .connection-message) { max-width: 200px; }
  .status-dot { flex-shrink: 0; }
  .connection-chip.connected { color: var(--ink); }
  .connection-chip.testing { color: var(--warn); opacity: 1; }
  .connection-chip.running { opacity: 1; }
  .connection-chip.stale { color: var(--warn); }
  .running .status-dot, .testing .status-dot, .stale .status-dot { background: var(--warn); }
  .connection-chip.failed { border-color: var(--danger); background: var(--danger-soft); color: var(--danger); }
  .failed .status-dot { background: var(--danger); }
  @media (max-width: 1000px) { .connection-user { display: none; } }
</style>
