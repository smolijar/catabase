<script lang="ts">
  import { tick } from 'svelte';
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  export let label: string;
  export let placeholder: string;
  export let items: Array<{ id: string; label: string; detail: string; group: string; disabled?: boolean; active?: boolean }>;
  export let onSelect: (id: string) => void;
  export let onClose: () => void;
  let search = '';
  let selected = 0;
  let input: HTMLInputElement;
  let list: HTMLDivElement;
  $: matches = items.filter(item => `${item.label} ${item.detail} ${item.group}`.toLowerCase().includes(search.toLowerCase())).slice(0, 80);
  $: if (search !== undefined) selected = 0;
  $: if (selected >= matches.length) selected = 0;

  async function keydown(event: KeyboardEvent) {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      selected = (selected + (event.key === 'ArrowDown' ? 1 : -1) + matches.length) % (matches.length || 1);
      await tick();
      list.querySelector(`[data-index="${selected}"]`)?.scrollIntoView({ block: 'nearest' });
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const item = matches[selected];
      if (item && !item.disabled) onSelect(item.id);
    }
  }
</script>

<Modal {label} {onClose}>
  <div class="picker">
    <div class="picker-head"><Icon name="search" size={16} /><input bind:this={input} bind:value={search} {placeholder} aria-label={label} role="combobox" aria-expanded="true" aria-controls="workspace-picker-list" aria-autocomplete="list" aria-activedescendant={matches.length ? `picker-option-${selected}` : undefined} on:keydown={keydown} /><button aria-label={`Close ${label.toLowerCase()}`} on:click={onClose}><Icon name="close" size={16} /></button></div>
    <div class="picker-list" bind:this={list} id="workspace-picker-list" role="listbox" aria-label={label}>
      {#each matches as item, index (item.id)}
        {#if index === 0 || matches[index - 1].group !== item.group}<div class="group-label" role="presentation">{item.group}</div>{/if}
        <button id={`picker-option-${index}`} data-index={index} role="option" aria-selected={selected === index} class:selected={selected === index} disabled={item.disabled} on:focus={() => selected = index} on:keydown={keydown} on:click={() => onSelect(item.id)}>
          <span class="item-label">{item.label}</span><span class="detail">{item.detail}</span>{#if item.active}<Icon name="check" size={14} />{/if}
        </button>
      {/each}
      {#if !matches.length}<p class="empty">No matches. Try another name.</p>{/if}
    </div>
    <footer><span><kbd>↑</kbd> <kbd>↓</kbd> navigate</span><span><kbd>↵</kbd> open</span><span><kbd>esc</kbd> close</span></footer>
  </div>
</Modal>

<style>
  .picker { background: var(--surface-2); }
  .picker-head { display: flex; align-items: center; gap: 8px; padding: 12px; border-bottom: 1px solid var(--line); }
  input { flex: 1; border: 0; background: transparent; outline: 0; }
  .picker-head button { width: 28px; padding: 0; background: transparent; border: 0; }
  .picker-list { max-height: min(420px, 60vh); overflow: auto; padding: 4px; }
  .group-label { padding: 8px 8px 4px; color: var(--ink-faint); font: 11px/16px var(--font-mono); text-transform: uppercase; letter-spacing: .08em; }
  .picker-list button { width: 100%; justify-content: flex-start; border: 0; background: transparent; padding: 6px 8px; min-height: 32px; }
  .picker-list button.selected { background: var(--surface-3); }
  .item-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .detail { margin-left: auto; max-width: 45%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--ink-faint); font: 11px/16px var(--font-mono); }
  footer { display: flex; gap: 16px; border-top: 1px solid var(--line); padding: 8px 12px; color: var(--ink-faint); font-size: 11px; }
</style>
