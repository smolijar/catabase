<script lang="ts">
  import Dropdown from './Dropdown.svelte';

  export let ariaLabel: string;
  export let label = 'More';
  export let disabled = false;
  export let items: Array<{ id: string; label: string; disabled?: boolean; danger?: boolean }>;
  export let onSelect: (id: string) => void = () => {};

  $: options = items.map(({ id, ...item }) => ({ value: id, ...item }));
</script>

<Dropdown
  value=""
  {options}
  {ariaLabel}
  {disabled}
  compact
  menuMode
  triggerLabel={label}
  {onSelect}
/>
