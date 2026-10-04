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
  dialog { width: min(760px, calc(100vw - 40px)); max-height: calc(100vh - 48px); overflow: auto; padding: 0; color: #dce6f7; background: #0d1625; border: 1px solid #3a4b65; border-radius: 12px; box-shadow: 0 20px 70px #0009; }
  dialog::backdrop { background: #030712b8; }
  dialog :global(.modal) { width: 100%; border: 0; box-shadow: none; border-radius: 0; }
</style>
