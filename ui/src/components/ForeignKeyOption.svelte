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
  .fk-option { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 28px; padding: 4px 6px; border: 0; border-bottom: 1px solid #35465d; border-radius: 0; background: #141e2b; color: #e4ebf5; text-align: left; box-shadow: none; font-size: 11px; }
  strong { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; font-weight: 500; }
  .key-only, .fk-key { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
  .fk-key { flex: 0 1 auto; min-width: 0; max-width: 48%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #a4b5ce; font-size: 10px; }
  .fk-option:hover:not(:disabled) { background: #26364b; }
  .fk-option.chosen { background: #293e5d; box-shadow: inset 2px 0 #8cb6f3; }
  .fk-option:focus-visible { outline: 2px solid #90baff; outline-offset: -2px; }
  .fk-option:disabled { opacity: .45; }
</style>
