<script lang="ts">
  import { onMount } from 'svelte';
  import Dropdown from './Dropdown.svelte';
  import ForeignKeyOption from './ForeignKeyOption.svelte';
  import { createForeignKeyLookup, foreignKeyValuesMatch } from '../lib/foreignKeyEditing';
  import type { ForeignKeyLookupState, ForeignKeyTarget } from '../lib/foreignKeyEditing';
  import type { ForeignKeyLookup, ForeignKeyValues } from '../lib/types';

  export let targets: ForeignKeyTarget[];
  export let disabled = false;
  export let selectedRow: Record<string, string | null> | null = null;
  export let onLookup: (request: ForeignKeyLookup, token: string) => Promise<ForeignKeyValues>;
  export let onCancel: (token: string) => void;
  export let onPick: (target: ForeignKeyTarget, row: Record<string, string | null>) => void;
  export let onTargetChange: () => void = () => {};

  let choice = '';
  let search = '';
  let mounted = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let state: ForeignKeyLookupState = { loading: false, data: null, error: '' };
  const lookup = createForeignKeyLookup((request, token) => onLookup(request, token), (next) => { state = next; }, (token) => onCancel(token));
  $: target = targets.find((item) => item.key.name === choice) ?? targets[0];
  $: validData = state.data && target && foreignKeyValuesMatch(target, state.data) ? state.data : null;
  $: metadataChanged = Boolean(state.data && !validData);
  $: if (mounted) schedule(target, search, disabled);

  onMount(() => {
    mounted = true;
    return () => { mounted = false; if (timer) clearTimeout(timer); lookup.invalidate(); };
  });

  function schedule(next: ForeignKeyTarget, term: string, blocked: boolean) {
    if (timer) clearTimeout(timer);
    lookup.invalidate();
    if (!next || next.unavailableReason || blocked) { state = { loading: false, data: null, error: '' }; return; }
    state = { loading: true, data: null, error: '' };
    timer = setTimeout(() => void lookup.run({ schema: next.schema, table: next.table, constraint: next.key.name, search: term }), term ? 250 : 0);
  }

  function changeTarget(value: string) {
    choice = value;
    search = '';
    onTargetChange();
  }

  function isSelected(row: Record<string, string | null>, picked: typeof selectedRow, columns: string[]) {
    return Boolean(picked && columns.every((name) => picked[name] === row[name]));
  }

</script>

<section class="fk-picker" aria-label="Foreign key values">
  {#if targets.length > 1}
    <Dropdown value={target?.key.name ?? ''} options={targets.map((item) => ({ value: item.key.name, label: `${item.key.name} → ${item.key.referencedSchema}.${item.key.referencedTable}` }))} ariaLabel="Foreign key relationship" {disabled} onSelect={changeTarget} />
  {/if}
  {#if target}
    <p class="fk-source" title={target.key.name}>{target.key.referencedSchema}.{target.key.referencedTable} ({target.key.referencedColumns.join(', ')})</p>
    {#if target.unavailableReason}
      <p class="fk-status">{target.unavailableReason}</p>
    {:else}
      <input type="search" aria-label="Search referenced values" placeholder="Search keys or row preview…" maxlength="512" bind:value={search} {disabled} on:keydown={(event) => { if (event.key === 'Enter') { event.preventDefault(); event.stopPropagation(); } }} />
      {#if state.loading}<p class="fk-status" role="status">Loading referenced values…</p>
      {:else if state.error || metadataChanged}
        <p class="fk-error" role="alert">{state.error || 'This relationship changed. Refresh the schema and re-run the query.'}</p>
        <button type="button" {disabled} on:click={() => schedule(target, search, disabled)}>Retry preview</button>
      {:else if validData}
        <div class="fk-options" aria-label="Available referenced rows">
          {#each validData.rows as row}
            <ForeignKeyOption {row} keyColumns={target.key.referencedColumns} previewColumns={validData.previewColumns} selected={isSelected(row, selectedRow, target.key.referencedColumns)} {disabled} onPick={() => onPick(target, row)} />
          {/each}
        </div>
        <p class="fk-status" role="status" title={`Choose a row, then stage the change.${validData.hasMore ? ' First 50 matches; search to narrow the list.' : ''}${target.mappings.length > 1 ? ' All foreign-key components will be staged together.' : ''}`}>{!validData.rows.length ? 'No matching referenced rows.' : `${validData.rows.length}${validData.hasMore ? '+' : ''} ${validData.rows.length === 1 && !validData.hasMore ? 'match' : 'matches'} · committed rows`}</p>
      {/if}
      {#if !validData}<p class="fk-note" title={target.mappings.length > 1 ? 'All foreign-key components will be staged together.' : undefined}>Committed rows · choose, then stage.{target.mappings.length > 1 ? ' Composite key.' : ''}</p>{/if}
    {/if}
  {/if}
</section>

<style>
  .fk-picker { display: flex; flex-direction: column; flex: 1; min-height: 140px; margin-top: 6px; }
  .fk-source { flex-shrink: 0; margin: 0 0 6px; color: var(--ink-muted); font: 11px/1.5 var(--font-mono); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  input { flex-shrink: 0; width: 100%; min-height: 30px; padding: 6px 8px; border: 1px solid var(--line-strong); border-radius: 4px; background: var(--surface-1); color: var(--ink); font-size: 11px; }
  .fk-options { flex: 1; min-height: 56px; overflow-y: auto; overscroll-behavior: contain; margin-top: 6px; border: 1px solid var(--line); border-radius: 4px; }
  .fk-status, .fk-note, .fk-error { flex-shrink: 0; margin: 4px 0; font-size: 11px; line-height: 1.5; }
  .fk-status, .fk-note { color: var(--ink-muted); }
  .fk-error { color: var(--danger); overflow-wrap: anywhere; }
</style>
