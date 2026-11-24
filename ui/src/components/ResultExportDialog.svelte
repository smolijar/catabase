<script lang="ts">
  import Modal from './Modal.svelte';
  import Dropdown from './Dropdown.svelte';
  import {
    DEFAULT_EXPORT_BATCH, DEFAULT_EXPORT_LIMIT, MAX_EXPORT_BATCH, MAX_EXPORT_LIMIT,
    exportFormats, validExportPagination
  } from '../lib/export';
  import type { ExportFormat, ExportOptions, ExportScope, ResultExportProgress } from '../lib/export';

  export let defaultFormat: ExportFormat;
  export let selectionRows: number;
  export let loadedRows: number;
  export let queryBlockedReason: string;
  export let connectionName: string | undefined;
  export let progress: ResultExportProgress | null;
  export let busy: boolean;
  export let onStart: (options: ExportOptions) => void;
  export let onCancel: () => void;
  export let onClose: () => void;

  const formatOptions = exportFormats.map(value => ({ value, label: value.toUpperCase() }));
  const targetOptions = [
    { value: 'file', label: 'File' },
    { value: 'clipboard', label: 'Clipboard' }
  ];
  let format = defaultFormat;
  let target: ExportOptions['target'] = 'file';
  let scope: ExportScope = selectionRows ? 'selection' : 'loaded';
  let batchSize = DEFAULT_EXPORT_BATCH;
  let rowLimit = DEFAULT_EXPORT_LIMIT;

  $: scopeOptions = [
    { value: 'selection', label: selectionRows ? `Selected cells · ${selectionRows.toLocaleString()} ${selectionRows === 1 ? 'row' : 'rows'}` : 'Selected cells', disabled: !selectionRows },
    { value: 'loaded', label: `Loaded rows · ${loadedRows.toLocaleString()}` },
    { value: 'query', label: 'Query results (paged)', disabled: Boolean(queryBlockedReason) }
  ];
  $: canStart = !busy && (scope !== 'query' || (!queryBlockedReason && validExportPagination(batchSize, rowLimit)));

  function start() {
    if (canStart) onStart({ format, target, scope, batchSize, rowLimit });
  }
</script>

<Modal label="Export results" locked={busy} {onClose}>
  <form class="modal export-dialog" on:submit|preventDefault={start}>
    <div class="panel-head"><h2>Export results</h2><button type="button" disabled={busy} on:click={onClose}>Close</button></div>
    <fieldset disabled={busy}>
      <div class="export-fields">
        <div class="export-field">
          <span aria-hidden="true">Format</span>
          <Dropdown value={format} options={formatOptions} ariaLabel="Format" disabled={busy} onSelect={(value) => (format = value as ExportFormat)} />
        </div>
        <div class="export-field">
          <span aria-hidden="true">Destination</span>
          <Dropdown value={target} options={targetOptions} ariaLabel="Destination" disabled={busy} onSelect={(value) => (target = value as ExportOptions['target'])} />
        </div>
      </div>
      <div class="export-field">
        <span aria-hidden="true">Rows</span>
        <Dropdown value={scope} options={scopeOptions} ariaLabel="Rows" disabled={busy} onSelect={(value) => (scope = value as ExportScope)} />
      </div>
      {#if scope === 'query'}
        <div class="export-fields">
          <label>Batch size <input type="number" bind:value={batchSize} required min="1" max={MAX_EXPORT_BATCH} step="1" /></label>
          <label>Row limit <input type="number" bind:value={rowLimit} required min="1" max={MAX_EXPORT_LIMIT} step="1" /></label>
        </div>
        {#if queryBlockedReason}<p>{queryBlockedReason}</p>
        {:else}<p>Runs the executed SQL again on {connectionName} using LIMIT and OFFSET. Use an ORDER BY with a unique tie-breaker to keep the order consistent across batches.</p>{/if}
      {:else if queryBlockedReason}
        <p class="paging-note">{queryBlockedReason}</p>
      {/if}
    </fieldset>

    {#if progress}
      <div class="export-progress" role={progress.phase === 'failed' ? 'alert' : 'status'}>
        {#if progress.phase === 'choosing-file'}<strong>Choose a filename…</strong>
        {:else if progress.phase === 'complete' || progress.phase === 'cancelled' || progress.phase === 'failed'}
          <strong class:export-error={progress.phase === 'failed'}>{progress.message}</strong>
          {#if progress.phase === 'complete' && progress.queries}
            <span>{progress.queries.toLocaleString()} {progress.queries === 1 ? 'query' : 'queries'} completed</span>
          {/if}
        {:else}
          <strong>{progress.phase === 'cancelling' ? 'Cancelling export…' : progress.phase === 'finishing' ? 'Finishing export…' : 'Exporting results…'}</strong>
          <progress aria-label="Rows exported" value={progress.rows} max={progress.maxRows}></progress>
          <span>{progress.rows.toLocaleString()} / {progress.maxRows.toLocaleString()} rows · {progress.queries.toLocaleString()} {progress.queries === 1 ? 'query' : 'queries'} completed</span>
          {#if progress.currentPage && progress.phase === 'running'}
            <span>Query {(progress.queries + 1).toLocaleString()} · Fetching up to {progress.currentPage.batchSize.toLocaleString()} rows at offset {progress.currentPage.offset.toLocaleString()}</span>
          {/if}
        {/if}
      </div>
    {/if}

    <div class="row-actions">
      {#if busy}
        <button type="button" on:click={onCancel} disabled={progress?.phase !== 'running'}>Cancel export</button>
      {:else}
        <button type="submit" class="primary" disabled={!canStart}>{target === 'file' ? 'Save…' : 'Export to clipboard'}</button>
      {/if}
    </div>
  </form>
</Modal>

<style>
  fieldset { display: grid; gap: 12px; min-width: 0; margin: 0; padding: 0; border: 0; }
  .export-fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  .export-field, label { display: grid; gap: 5px; min-width: 0; color: var(--ink-muted); font-size: 12px; }
  input[type='number'] { appearance: textfield; }
  input::-webkit-inner-spin-button, input::-webkit-outer-spin-button { -webkit-appearance: none; margin: 0; }
  p { margin: 0; color: var(--ink-muted); font-size: 12px; line-height: 1.5; }
  .export-progress { display: grid; gap: 6px; padding: 10px 12px; border: 1px solid var(--line); border-radius: 4px; background: var(--surface-1); font-size: 12px; }
  .export-progress span { color: var(--ink-muted); font-family: var(--font-mono); }
  .export-progress progress { width: 100%; height: 8px; accent-color: var(--accent); }
  .export-error { color: var(--danger); overflow-wrap: anywhere; }
</style>
