<script lang="ts">
  export let row: Record<string, string | null>;
  export let keyColumns: string[];
  export let previewColumns: string[];
  export let selected = false;
  export let disabled = false;
  export let onPick: () => void;
  $: key = keyColumns.map((name) => row[name] ?? 'NULL').join(' · ');
  $: previews = previewColumns.filter((name) => row[name] != null && row[name] !== '');
  $: preview = previews.map((name) => row[name]).join(' · ');
  $: details = [...keyColumns, ...previews].map((name) => `${name}: ${row[name] ?? 'NULL'}`).join('\n');
</script>

<button type="button" class="fk-option" class:chosen={selected} aria-pressed={selected} aria-label={`Use referenced key ${key}${preview ? `: ${preview}` : ''}`} title={details} {disabled} on:click={() => { if (!disabled) onPick(); }}>
  <strong class:key-only={!preview}>{preview || key}</strong>
  {#if preview}<span class="fk-key" aria-hidden="true">{key}</span>{/if}
</button>

<style>
  .fk-option { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 28px; padding: 4px 6px; border: 0; border-bottom: 1px solid var(--line); border-radius: 0; background: var(--surface-1); color: var(--ink); text-align: left; box-shadow: none; font-size: 11px; }
  strong { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; font-weight: 500; }
  .key-only, .fk-key { font-family: var(--font-mono); }
  .fk-key { flex: 0 1 auto; min-width: 0; max-width: 48%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--ink-muted); font-size: 11px; }
  .fk-option:hover:not(:disabled) { background: var(--surface-2); }
  .fk-option.chosen { background: var(--accent-soft); box-shadow: inset 2px 0 var(--accent); }
  .fk-option:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
  .fk-option:disabled { opacity: .45; }
</style>
