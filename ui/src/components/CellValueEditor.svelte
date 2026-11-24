<script lang="ts">
  import { literalTextAttributes } from '../lib/textInput';
  import { cellKind } from '../lib/resultsGrid';
  import type { ColumnMeta } from '../lib/types';

  export let column: ColumnMeta;
  export let draft = '';
  export let isNull = false;
  export let useDefault = false;
  export let pickerOnly = false;
  export let disabled = false;
  export let error = '';
  export let onChange: (value: string) => void = () => {};
  export let onEditValue: () => void = () => {};
  $: kind = cellKind(column);
  $: valueLabel = useDefault ? 'DEFAULT' : isNull ? 'NULL' : draft || 'empty string';
</script>

{#if pickerOnly}
  <div class="picked-row"><span class="value-mode">Value</span><output class="picked-value" class:null-value={isNull || useDefault} aria-label={isNull ? `Selected SQL NULL for ${column.name}` : `Selected value for ${column.name}`} title={valueLabel}>{valueLabel}</output></div>
{:else if isNull || useDefault}
  <div class="value-mode">Value</div>
  <div class="null-preview">{valueLabel} <span>{useDefault ? 'PostgreSQL evaluates the default when you apply the edit.' : 'No value will be stored.'}</span><button type="button" {disabled} on:click={onEditValue}>Edit value</button></div>
{:else if kind === 'boolean'}
  <div class="value-mode">Value</div>
  <div class="boolean-options" role="group" aria-label="Boolean value">
    <label class:chosen={draft === 'true'}><input type="radio" name="cell-boolean" checked={draft === 'true'} {disabled} on:change={() => onChange('true')} /> true</label>
    <label class:chosen={draft === 'false'}><input type="radio" name="cell-boolean" checked={draft === 'false'} {disabled} on:change={() => onChange('false')} /> false</label>
  </div>
{:else}
  <div class="value-mode">Value</div>
  <textarea {...literalTextAttributes} aria-label={`New value for ${column.name}`} aria-invalid={Boolean(error)} placeholder={kind === 'array' ? '{"one","two",NULL}' : undefined} value={draft} {disabled} inputmode={kind === 'integer' || kind === 'numeric' ? 'decimal' : 'text'}
    on:input={(event) => { if (!(event instanceof InputEvent && event.isComposing)) onChange(event.currentTarget.value); }}
    on:compositionend={(event) => onChange(event.currentTarget.value)}></textarea>
{/if}
{#if !pickerOnly && !disabled && kind !== 'boolean'}<p class="edit-hint">{kind === 'array' ? 'PostgreSQL array: {"one","two",NULL}. {} is empty; NULL is a null element; "NULL" is text. Use Set NULL for a SQL NULL array. Dimensions and numeric precision are preserved.' : kind === 'text' ? 'An empty value is an empty string. Use Set NULL to remove the value.' : kind === 'json' ? 'JSON is validated before staging. JSON null and SQL NULL are distinct.' : 'Numbers retain their full precision.'}</p>{/if}

<style>
  .value-mode { flex-shrink: 0; margin-bottom: 5px; font-size: 11px; color: var(--ink-muted); }
  .picked-row { display: flex; align-items: baseline; gap: 8px; flex-shrink: 0; margin-bottom: 6px; }
  .picked-row .value-mode { margin: 0; }
  .picked-value { display: block; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font: 11px/1.5 var(--font-mono); }
  .picked-value.null-value { color: var(--ink-muted); font-style: italic; }
  textarea { display: block; width: 100%; min-height: 100px; resize: vertical; padding: 9px; border: 1px solid var(--line-strong); border-radius: 4px; background: var(--surface-1); color: var(--ink); font: 12px/1.5 var(--font-mono); box-sizing: border-box; }
  textarea:focus { outline: 2px solid var(--focus); outline-offset: 1px; }
  textarea[aria-invalid='true'] { border-color: var(--danger); }
  .null-preview { padding: 12px; border: 1px dashed var(--line-strong); border-radius: 4px; color: var(--ink-muted); font-family: var(--font-mono); }
  .null-preview span { display: block; margin: 4px 0 8px; font-size: 11px; }
  .null-preview button { min-height: 25px; padding: 3px 8px; border: 1px solid var(--line-strong); border-radius: 4px; background: var(--surface-2); color: var(--ink); font-size: 11px; box-shadow: none; }
  .null-preview button:hover:not(:disabled) { background: var(--surface-2); }
  .null-preview button:disabled { opacity: .45; }
  .null-preview button:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
  .boolean-options { display: flex; gap: 8px; }
  .boolean-options label { display: flex; align-items: center; gap: 8px; flex: 1; padding: 10px; border: 1px solid var(--line-strong); border-radius: 4px; background: var(--surface-1); font-size: 12px; }
  .boolean-options label.chosen { border-color: var(--accent); background: var(--accent-soft); }
  input[type='radio'] { appearance: auto; width: 14px; height: 14px; min-height: 0; padding: 0; margin: 0; accent-color: var(--accent); }
  .edit-hint { margin: 9px 0 11px; font-size: 11px; color: var(--ink-muted); line-height: 1.5; }
</style>
