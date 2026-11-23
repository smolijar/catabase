<script lang="ts">
  import Dropdown from './Dropdown.svelte';
  import { commitModeOptions } from '../lib/commitMode';
  import type { CommitMode } from '../lib/types';

  export let mode: CommitMode;
  export let compact = false;
  export let disabled = false;
  export let onChange: (mode: CommitMode) => void;

  function select(value: string) {
    const selected = commitModeOptions.find((option) => option.value === value);
    if (selected) onChange(selected.value);
  }
</script>

<div class:compact>
  <Dropdown value={mode} options={commitModeOptions} ariaLabel="Commit mode" {disabled} {compact} onSelect={select} />
</div>

<style>
  .compact { flex: 0 0 auto; }
  .compact :global(.dropdown.compact .dropdown-trigger) {
    min-height: 22px;
    max-height: 22px;
    gap: 4px;
    padding: 2px 4px;
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--ink);
    font: inherit;
  }
  .compact :global(.dropdown.compact .dropdown-trigger:hover:not(:disabled)) { background: var(--surface-3); }
  .compact :global(.dropdown-menu) { font: 13px/18px var(--font-sans); }
</style>
