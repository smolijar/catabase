<script lang="ts">
  import { cellKind } from '../lib/resultsGrid';
  import type { ColumnMeta } from '../lib/types';

  export let column: ColumnMeta;
  export let draft = '';
  export let isNull = false;
  export let pickerOnly = false;
  export let disabled = false;
  export let error = '';
  export let onChange: () => void = () => {};
  export let onStage: () => void = () => {};
  $: kind = cellKind(column);
</script>

{#if pickerOnly}
  <div class="picked-row"><span class="value-mode">Value</span><output class="picked-value" class:null-value={isNull} aria-label={isNull ? `Selected SQL NULL for ${column.name}` : `Selected value for ${column.name}`} title={isNull ? 'SQL NULL' : draft}>{isNull ? 'NULL' : draft || 'empty string'}</output></div>
{:else if isNull}
  <div class="value-mode">Value</div>
  <div class="null-preview">NULL <span>No value will be stored.</span><button type="button" {disabled} on:click={() => { isNull = false; onChange(); }}>Edit value</button></div>
{:else if kind === 'boolean'}
  <div class="value-mode">Value</div>
  <div class="boolean-options" role="group" aria-label="Boolean value">
    <label class:chosen={draft === 'true'}><input type="radio" bind:group={draft} value="true" {disabled} on:change={onChange} /> true</label>
    <label class:chosen={draft === 'false'}><input type="radio" bind:group={draft} value="false" {disabled} on:change={onChange} /> false</label>
  </div>
{:else}
  <div class="value-mode">Value</div>
  <textarea aria-label={`New value for ${column.name}`} aria-invalid={Boolean(error)} bind:value={draft} {disabled} spellcheck="false" inputmode={kind === 'integer' || kind === 'numeric' ? 'decimal' : 'text'} on:input={onChange} on:keydown={(event) => { if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') { event.preventDefault(); event.stopPropagation(); onStage(); } }}></textarea>
{/if}
{#if !pickerOnly}<p class="edit-hint">{kind === 'text' ? 'An empty value is an empty string. Use Set NULL to remove the value.' : kind === 'json' ? 'JSON is validated before staging. JSON null and SQL NULL are distinct.' : kind === 'numeric' || kind === 'integer' ? 'Numbers retain their full precision.' : 'Choose a value, then stage the change.'}</p>{/if}

<style>
  .value-mode { flex-shrink: 0; margin-bottom: 5px; font-size: 11px; color: #adbed3; }
  .picked-row { display: flex; align-items: baseline; gap: 8px; flex-shrink: 0; margin-bottom: 6px; }
  .picked-row .value-mode { margin: 0; }
  .picked-value { display: block; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font: 11px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; }
  .picked-value.null-value { color: #b5c5da; font-style: italic; }
  textarea { display: block; width: 100%; min-height: 100px; resize: vertical; padding: 9px; border: 1px solid #596d87; border-radius: 5px; background: #111a26; color: #eef4ff; font: 12px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; box-sizing: border-box; }
  textarea:focus { outline: 2px solid #86b1f1; outline-offset: 1px; }
  textarea[aria-invalid='true'] { border-color: #f29292; }
  .null-preview { padding: 12px; border: 1px dashed #556a85; border-radius: 5px; color: #b5c5da; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
  .null-preview span { display: block; margin: 4px 0 8px; font-size: 10px; }
  .null-preview button { min-height: 25px; padding: 3px 8px; border: 1px solid #3c485b; border-radius: 5px; background: #242e3c; color: #e4ebf5; font-size: 11px; box-shadow: none; }
  .null-preview button:hover:not(:disabled) { background: #303e50; }
  .null-preview button:disabled { opacity: .45; }
  .null-preview button:focus-visible { outline: 2px solid #90baff; outline-offset: -2px; }
  .boolean-options { display: flex; gap: 8px; }
  .boolean-options label { display: flex; align-items: center; gap: 8px; flex: 1; padding: 10px; border: 1px solid #425674; border-radius: 5px; background: #161e2b; font-size: 12px; }
  .boolean-options label.chosen { border-color: #8cb6f3; background: #293e5d; }
  input[type='radio'] { appearance: auto; width: 14px; height: 14px; min-height: 0; padding: 0; margin: 0; accent-color: #8ab9fc; }
  .edit-hint { margin: 9px 0 11px; font-size: 11px; color: #b9c6d8; line-height: 1.5; }
</style>
