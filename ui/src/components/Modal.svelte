<script lang="ts">
  import { onMount } from 'svelte';
  export let label: string;
  export let locked = false;
  export let onClose: () => void;
  let dialog: HTMLDialogElement;
  onMount(() => { dialog.showModal(); return () => dialog.close(); });
</script>

<dialog bind:this={dialog} aria-label={label} on:cancel={(event) => { event.preventDefault(); if (!locked) onClose(); }}>
  <slot />
</dialog>

<style>
  dialog { width: min(760px, calc(100vw - 40px)); max-height: calc(100vh - 48px); overflow: auto; padding: 0; color: var(--ink); background: var(--surface-2); border: 1px solid var(--line-strong); border-radius: 6px; box-shadow: var(--shadow-pop); }
  dialog::backdrop { background: var(--backdrop); }
  dialog :global(.modal) { width: 100%; border: 0; box-shadow: none; border-radius: 0; }
</style>
