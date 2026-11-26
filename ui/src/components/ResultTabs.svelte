<script lang="ts">
  import { tick } from 'svelte';
  import ActionMenu from './ActionMenu.svelte';
  import Icon from './Icon.svelte';
  import type { ResultTab } from '../lib/workspace';

  export let results: ResultTab[];
  export let activeId: string | null;
  export let busyId: string | null = null;
  export let busy = false;
  export let onSelect: (id: string) => void;
  export let onClose: (id: string) => void;
  export let onCloseAll: () => void;
  let bar: HTMLDivElement;
  let menu: ActionMenu;
  let menuResultId: string | null = null;
  $: items = [
    { id: 'close', label: 'Close', disabled: busyId === menuResultId },
    { id: 'close-all', label: 'Close all', disabled: busy }
  ];
  $: if (bar && activeId) void revealActiveTab(activeId);

  async function revealActiveTab(id: string) {
    await tick();
    if (activeId === id) bar?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }

  function openMenu(event: MouseEvent | KeyboardEvent, id: string) {
    event.preventDefault();
    event.stopPropagation();
    const button = event.currentTarget as HTMLButtonElement;
    button.focus({ preventScroll: true });
    menuResultId = id;
    const anchor = button.getBoundingClientRect();
    void tick().then(() => menu.openAt(event instanceof MouseEvent ? event.clientX : anchor.left,
      event instanceof MouseEvent ? event.clientY : anchor.bottom));
  }

  function chooseAction(action: string) {
    if (action === 'close' && menuResultId) onClose(menuResultId);
    else if (action === 'close-all') onCloseAll();
  }

  async function handleKey(event: KeyboardEvent, id: string) {
    if (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey)) {
      openMenu(event, id);
      return;
    }
    if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return;
    const ids = results.map(result => result.id);
    const index = ids.indexOf(id);
    let next: string;
    if (event.key === 'ArrowLeft') next = ids[(index - 1 + ids.length) % ids.length];
    else if (event.key === 'ArrowRight') next = ids[(index + 1) % ids.length];
    else if (event.key === 'Home') next = ids[0];
    else if (event.key === 'End') next = ids[ids.length - 1];
    else return;
    event.preventDefault();
    onSelect(next);
    await tick();
    bar.querySelector<HTMLButtonElement>(`#${CSS.escape(`result-set-${next}`)}`)?.focus();
  }
</script>

<div class="result-tabbar">
  <div class="result-tabs" role="tablist" aria-label="Result sets" bind:this={bar}>
    {#each results as result, index (result.id)}
      <div class="result-tab" role="presentation">
        <button type="button" id={`result-set-${result.id}`} role="tab" aria-controls="query-results-panel"
          aria-selected={activeId === result.id} tabindex={activeId === result.id ? 0 : -1} aria-haspopup="menu"
          title={result.error?.message ?? result.context.execution.sql.slice(0, 300)} on:click={() => onSelect(result.id)}
          on:contextmenu={event => openMenu(event, result.id)} on:keydown={event => handleKey(event, result.id)}>
          {#if result.error}<span class="result-error" role="img" aria-label="Failed"><Icon name="close" size={10} /></span>{/if}
          Result {index + 1}
          {#if result.pendingChanges.length}<span class="result-edits" role="img" aria-label="Staged edits">●</span>{/if}
        </button>
        {#if activeId === result.id}
          <button class="close-result" type="button" aria-label={`Close Result ${index + 1}`} title={`Close Result ${index + 1}`}
            disabled={busyId === result.id} on:click={() => onClose(result.id)}><Icon name="close" size={12} /></button>
        {/if}
      </div>
    {/each}
  </div>
  <ActionMenu bind:this={menu} contextOnly ariaLabel="Result tab actions" {items} onSelect={chooseAction} />
</div>

<style>
  .result-tabbar { display: flex; align-items: center; flex: 0 0 auto; min-width: 0; padding: 0 8px; border-bottom: 1px solid var(--line); background: var(--surface-0); }
  .result-tabs { display: flex; min-width: 0; overflow-x: auto; scrollbar-width: none; }
  .result-tab { position: relative; flex: 0 0 auto; }
  [role='tab'] { gap: 4px; min-height: 25px; padding: 0 25px 0 8px; border: 0; border-bottom: 2px solid transparent; border-radius: 0; background: transparent; color: var(--ink-muted); font-size: 11px; white-space: nowrap; }
  [role='tab'][aria-selected='true'] { color: var(--accent); border-bottom-color: var(--accent); }
  [role='tab']:hover, .close-result:hover { color: var(--ink); }
  .result-error { color: var(--danger); }
  .result-edits { color: var(--warn); font-size: 7px; }
  .close-result { position: absolute; inset: 2px 3px auto auto; min-height: 19px; width: 19px; padding: 0; border: 0; background: transparent; color: var(--ink-muted); }
</style>
