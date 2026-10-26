<script lang="ts">
  import { tick } from 'svelte';
  import { CONNECTION_COLORS } from '../lib/appearance';
  import type { ConnectionAppearance } from '../lib/appearance';
  import type { ConnectionInput } from '../lib/types';

  export let connection: ConnectionInput;
  export let appearance: ConnectionAppearance;
  export let url = '';
  export let feedback = '';
  export let saving = false;
  export let testing = false;
  export let section: 'connection' | 'appearance' = 'connection';
  export let onClose: () => void;
  export let onSave: () => void;
  export let onTest: () => void;
  export let onImport: () => void;
  export let onConnectionInput: () => void;
  let form: HTMLFormElement;
  let tablist: HTMLDivElement;

  async function submit() {
    // The Connection panel stays mounted so switching tabs preserves all fields.
    // Reveal invalid fields before reporting validity, even when saving Appearance.
    if (!form.checkValidity()) {
      section = 'connection';
      await tick();
      form.reportValidity();
      return;
    }
    onSave();
  }

  async function navigate(event: KeyboardEvent) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    section = event.key === 'Home' ? 'connection' : event.key === 'End' ? 'appearance'
      : section === 'connection' ? 'appearance' : 'connection';
    await tick();
    tablist.querySelector<HTMLButtonElement>('[aria-selected="true"]')?.focus();
  }
</script>

<form class="modal" bind:this={form} novalidate on:submit|preventDefault={submit}>
  <div class="panel-head">
    <h2>{connection.id ? 'Edit connection' : 'New connection'}</h2>
    <button type="button" disabled={saving} on:click={onClose}>Close</button>
  </div>
  <div class="section-tabs" role="tablist" aria-label="Connection settings" bind:this={tablist}>
    {#each ['connection', 'appearance'] as tab}
      <button type="button" role="tab" id={`connection-editor-${tab}-tab`} aria-controls={`connection-editor-${tab}-panel`}
        aria-selected={section === tab} tabindex={section === tab ? 0 : -1}
        on:keydown={navigate} on:click={() => section = tab as typeof section}>
        {tab === 'connection' ? 'Connection' : 'Appearance'}
      </button>
    {/each}
  </div>
  <div class="connection-panel" id="connection-editor-connection-panel" role="tabpanel" aria-labelledby="connection-editor-connection-tab" hidden={section !== 'connection'}>
    <label>Name <input disabled={saving} bind:value={connection.name} on:input={onConnectionInput} required /></label>
    <label>Host <input disabled={saving} bind:value={connection.host} on:input={onConnectionInput} required /></label>
    <div class="form-grid">
      <label>Port <input disabled={saving} type="number" min="1" max="65535" bind:value={connection.port} on:input={onConnectionInput} required /></label>
      <label>Database <input disabled={saving} bind:value={connection.database} on:input={onConnectionInput} required /></label>
    </div>
    <label>Username <input disabled={saving} bind:value={connection.username} on:input={onConnectionInput} required /></label>
    <label>Password <input disabled={saving} type="password" bind:value={connection.password} on:input={onConnectionInput} /></label>
    <label class="checkbox"><input disabled={saving} type="checkbox" bind:checked={connection.ssl} on:change={onConnectionInput} /> Use SSL</label>
    <label>Connection URL <input disabled={saving} bind:value={url} /></label>
    <div class="row-actions"><button type="button" disabled={saving} on:click={onImport}>Import URL</button></div>
  </div>
  <div class="connection-panel" id="connection-editor-appearance-panel" role="tabpanel" aria-labelledby="connection-editor-appearance-tab" hidden={section !== 'appearance'}>
    <p>Display preferences for <strong>{connection.name || 'this connection'}</strong>, saved on this device.</p>
    <div>Connection color</div>
    <div class="color-options" role="group" aria-label="Connection color">
      {#each CONNECTION_COLORS as color}
        <button type="button" class="color-option" data-conn={color}
          aria-label={color === 'none' ? 'No color' : color} title={color === 'none' ? 'No color override (use app default)' : color}
          aria-pressed={appearance.color === color} disabled={saving} on:click={() => appearance = { ...appearance, color }}></button>
      {/each}
    </div>
    <label>Environment label <input disabled={saving} maxlength="16" placeholder="e.g. PROD, LOCAL" bind:value={appearance.environment} /></label>
    <p>Color and label identify the connection; they do not change its database permissions.</p>
  </div>
  {#if feedback}<p class="muted" role="status">{feedback}</p>{/if}
  <div class="row-actions">
    {#if section === 'connection'}<button type="button" on:click={onTest} disabled={testing || saving}>{testing ? 'Testing…' : 'Test'}</button>{/if}
    <button type="submit" class="primary" disabled={saving}>{saving ? 'Saving…' : 'Save'}</button>
  </div>
</form>

<style>
  .connection-panel { display: grid; gap: 16px; }
  .connection-panel[hidden] { display: none; }
</style>
