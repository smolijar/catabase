<script lang="ts">
  import type { UpdateState } from '../lib/appUpdates';
  export let state: UpdateState;
  export let automatic = true;
  export let blocked = '';
  export let installError = '';
  export let onCheck: () => void;
  export let onAutomatic: (enabled: boolean) => void;
  export let onInstall: () => void;
  export let onReleases: () => void;
</script>

<section class="settings-card" aria-label="App updates">
  <div class="settings-card-head"><strong>App updates</strong><span>{state.status ? `v${state.status.currentVersion}` : 'Loading…'}</span></div>
  {#if state.status?.enabled}
    <label class="auto-check"><input type="checkbox" checked={automatic} on:change={(event) => onAutomatic(event.currentTarget.checked)} />Check automatically</label>
    <p>Checks when the app is in the foreground, at most every four hours. Installing an update restarts Catabase.</p>
    {#if state.update}
      <p class="available">Version {state.update.version} is available.</p>
      {#if state.update.notes}<details><summary>What’s new</summary><pre>{state.update.notes}</pre></details>{/if}
    {:else if state.checkedAt && !state.error}
      <p>You’re up to date.</p>
    {/if}
    <div class="row-actions">
      <button disabled={state.checking} on:click={onCheck}>{state.checking ? 'Checking…' : 'Check for updates'}</button>
      {#if state.update}<button class="primary" disabled={Boolean(blocked) || state.checking} title={blocked || 'Save SQL drafts, install the update and restart'} on:click={onInstall}>Update & restart</button>{/if}
    </div>
    {#if state.update}<p>{blocked || 'SQL tabs are saved before restarting. Live result grids will be cleared.'}</p>{/if}
    {#if state.checkedAt}<p class="checked-at">Last checked {new Date(state.checkedAt).toLocaleString()}</p>{/if}
  {:else if state.status}
    <p>{state.status.unavailableReason || 'Automatic updates are not configured for this build.'}</p>
    <button on:click={onReleases}>Open GitHub Releases</button>
  {/if}
  {#if state.error || installError}<p class="update-error" role="alert">{installError || state.error}</p>{/if}
  {#if !state.status && state.error}<button on:click={onCheck}>Retry</button>{/if}
</section>

<style>
  .auto-check { display: flex; align-items: center; gap: 8px; }
  .auto-check input { width: auto; }
  .available { color: var(--accent); }
  .update-error { color: var(--danger); }
  .checked-at { font-size: 11px; }
  details { margin: 12px 0; }
  summary { cursor: pointer; }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; font: inherit; max-height: 240px; overflow: auto; }
</style>
