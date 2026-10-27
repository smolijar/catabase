<script lang="ts">
  import { tick } from 'svelte';
  import { CONNECTION_COLORS } from '../lib/appearance';
  import type { ConnectionAppearance } from '../lib/appearance';
  import type { ConnectionInput } from '../lib/types';
  import { connectionOptions } from '../lib/connectionOptions';
  import { applyConnectionUrl, displayConnectionUrl, editsMaskedPassword, redactConnectionUrl } from '../lib/connectionUrl';

  export let connection: ConnectionInput;
  export let appearance: ConnectionAppearance;
  export let feedback = '';
  export let saving = false;
  export let testing = false;
  export let section: 'connection' | 'appearance' | 'options' = 'connection';
  export let onClose: () => void;
  export let onSave: () => void;
  export let onTest: () => void;
  let form: HTMLFormElement;
  let tablist: HTMLDivElement;
  let body: HTMLDivElement;
  let urlInput: HTMLInputElement;
  let url = displayConnectionUrl(connection);
  let urlError = '';
  const sections = ['connection', 'appearance', 'options'] as const;
  let options = connectionOptions(connection.options);
  $: connection.options = options;
  $: urlInput?.setCustomValidity(urlError);

  async function onConnectionInput() {
    // Svelte input bindings update after the event handler.
    await tick();
    url = displayConnectionUrl(connection);
    urlError = '';
  }

  function updateUrl(value: string, cursor: number) {
    const hasPassword = Boolean(connection.password);
    const redacted = redactConnectionUrl(value, hasPassword);
    const offset = redactConnectionUrl(value.slice(0, cursor), hasPassword).length;
    url = redacted;
    urlInput.value = redacted;
    urlInput.setSelectionRange(Math.min(offset, redacted.length), Math.min(offset, redacted.length));
    try { connection = applyConnectionUrl(redacted, connection); urlError = ''; }
    catch (error) { urlError = (error as Error).message; }
  }

  function beforeUrlInput(event: InputEvent) {
    if (editsMaskedPassword(url, urlInput.selectionStart ?? 0, urlInput.selectionEnd ?? 0, event.inputType)) event.preventDefault();
  }

  function pasteUrl(event: ClipboardEvent) {
    event.preventDefault();
    const start = urlInput.selectionStart ?? 0, end = urlInput.selectionEnd ?? 0;
    if (editsMaskedPassword(url, start, end, 'insertFromPaste')) return;
    const text = event.clipboardData?.getData('text/plain') ?? '';
    updateUrl(url.slice(0, start) + text + url.slice(end), start + text.length);
  }

  function changeSection(next: typeof section) {
    section = next;
    if (body) body.scrollTop = 0;
  }

  async function submit(action = onSave) {
    // The Connection panel stays mounted so switching tabs preserves all fields.
    // Reveal invalid fields before reporting validity, even when saving Appearance.
    if (!form.checkValidity()) {
      changeSection(form.querySelector('input:invalid')?.closest('[role="tabpanel"]')?.id === 'connection-editor-options-panel' ? 'options' : 'connection');
      await tick();
      form.reportValidity();
      return;
    }
    action();
  }

  async function navigate(event: KeyboardEvent) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const index = sections.indexOf(section);
    changeSection(event.key === 'Home' ? sections[0] : event.key === 'End' ? sections[sections.length - 1]
      : sections[(index + (event.key === 'ArrowLeft' ? -1 : 1) + sections.length) % sections.length]);
    await tick();
    tablist.querySelector<HTMLButtonElement>('[aria-selected="true"]')?.focus();
  }
</script>

<form class="modal connection-editor" bind:this={form} novalidate on:submit|preventDefault={() => submit()}>
  <div class="panel-head">
    <h2>{connection.id ? 'Edit connection' : 'New connection'}</h2>
    <button type="button" disabled={saving} on:click={onClose}>Close</button>
  </div>
  <div class="section-tabs" role="tablist" aria-label="Connection settings" bind:this={tablist}>
    {#each sections as tab}
      <button type="button" role="tab" id={`connection-editor-${tab}-tab`} aria-controls={`connection-editor-${tab}-panel`}
        aria-selected={section === tab} tabindex={section === tab ? 0 : -1}
        on:keydown={navigate} on:click={() => changeSection(tab)}>
        {tab === 'connection' ? 'Connection' : tab === 'appearance' ? 'Appearance' : 'Options'}
      </button>
    {/each}
  </div>
  <div class="connection-editor-body" bind:this={body}>
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
    <label>Connection URL
      <input disabled={saving} value={url} bind:this={urlInput} spellcheck="false" autocomplete="off"
        aria-describedby="connection-url-help connection-url-error" aria-invalid={Boolean(urlError)}
        on:beforeinput={beforeUrlInput} on:paste={pasteUrl}
        on:input={() => updateUrl(urlInput.value, urlInput.selectionStart ?? urlInput.value.length)} />
    </label>
    <p id="connection-url-help">URL changes update the fields automatically. The password is hidden and can only be changed in the Password field; passwords in pasted URLs are ignored.</p>
    <p id="connection-url-error" role="status">{urlError}</p>
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
    <p>Changes preview in the workspace when this connection is selected. Close without saving to revert.</p>
  </div>
  <div class="connection-panel" id="connection-editor-options-panel" role="tabpanel" aria-labelledby="connection-editor-options-tab" hidden={section !== 'options'}>
    <label>Connection timeout (seconds)
      <input type="number" min="1" max="300" step="1" required disabled={saving} bind:value={options.connectTimeoutSeconds} />
    </label>
    <p>Time allowed to connect or wait for an available pooled connection (1–300 seconds).</p>
    <label>Query timeout (seconds)
      <input type="number" min="0" max="86400" step="1" required disabled={saving} bind:value={options.queryTimeoutSeconds} />
    </label>
    <label>Lock timeout (seconds)
      <input type="number" min="0" max="86400" step="1" required disabled={saving} bind:value={options.lockTimeoutSeconds} />
    </label>
    <p>PostgreSQL cancels statements or lock waits that exceed these limits. Use 0 to keep the server default.</p>
    <label class="checkbox"><input type="checkbox" disabled={saving} bind:checked={options.readOnly} /> Read-only by default</label>
    <p>Starts PostgreSQL transactions read-only and disables cell editing. Explicit SQL can override the session default; use a read-only database role for enforced access restrictions.</p>
    <p>Connection and option changes apply on Save and reset affected results and pooled sessions. Finish any open transaction first.</p>
  </div>
  </div>
  <div class="connection-editor-footer">
    {#if feedback}<p class="muted" role="status">{feedback}</p>{/if}
    <div class="row-actions">
      {#if section === 'connection'}<button type="button" on:click={() => submit(onTest)} disabled={testing || saving}>{testing ? 'Testing…' : 'Test'}</button>{/if}
      <button type="submit" class="primary" disabled={saving}>{saving ? 'Saving…' : 'Save'}</button>
    </div>
  </div>
</form>

<style>
  .connection-editor { height: min(720px, calc(100dvh - 52px)); grid-template-rows: auto auto minmax(0, 1fr) auto; overflow: hidden; }
  .connection-editor-body { min-height: 0; overflow-y: auto; scrollbar-gutter: stable; padding: 3px 5px 5px 3px; }
  .connection-editor-footer { display: grid; gap: 8px; }
  .connection-editor-footer .row-actions { justify-content: flex-end; }
  .section-tabs { overflow-x: auto; flex-shrink: 0; }
  .connection-panel { display: grid; gap: 16px; }
  .connection-panel[hidden] { display: none; }
  #connection-url-error:empty { display: none; }
</style>
