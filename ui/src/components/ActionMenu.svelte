<script lang="ts">
  import Dropdown from './Dropdown.svelte';

  export let ariaLabel: string;
  export let label = 'More';
  export let disabled = false;
  export let iconOnly = false;
  export let contextOnly = false;
  export let items: Array<{ id: string; label: string; disabled?: boolean; danger?: boolean }>;
  export let onSelect: (id: string) => void = () => {};
  let dropdown: Dropdown;

  export async function openAt(x?: number, y?: number): Promise<void> {
    await dropdown.openAt(x, y);
  }

  $: options = items.map(({ id, ...item }) => ({ value: id, ...item }));
</script>

<Dropdown
  bind:this={dropdown}
  value=""
  {options}
  {ariaLabel}
  {disabled}
  {iconOnly}
  hideTrigger={contextOnly}
  compact
  menuMode
  triggerLabel={label}
  {onSelect}
/>
