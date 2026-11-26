<script lang="ts">
  import { tick } from 'svelte';
  import type { Snippet } from 'svelte';
  import SqlPreview from './SqlPreview.svelte';

  export let statements: string[];
  export let themeKey: string;
  export let action: Snippet<[number]> | undefined = undefined;
  const pageSize = 5;
  let shown = pageSize;
  let list: HTMLOListElement;
  $: visible = statements.slice(0, shown);
  $: remaining = statements.length - visible.length;

  async function showMore() {
    const firstNew = visible.length;
    shown += Math.min(remaining, pageSize);
    await tick();
    const preview = list?.children[firstNew] as HTMLLIElement | undefined;
    const checkbox = preview?.querySelector('input');
    if (checkbox) checkbox.focus();
    else if (preview) { preview.tabIndex = -1; preview.focus(); }
  }
</script>

<ol class="statement-previews" aria-label="SQL statements" bind:this={list}>
  {#each visible as statement, index}
    <li class="statement-preview" class:single={!action && statements.length === 1}>
      {#if action}{@render action(index)}
      {:else if statements.length > 1}<span class="statement-label">Statement {index + 1}</span>{/if}
      <SqlPreview sql={statement} {themeKey} />
    </li>
  {/each}
</ol>
{#if remaining > 0}
  <button class="more-statements" type="button" on:click={showMore}
    aria-label={`Show ${Math.min(remaining, pageSize)} more ${remaining === 1 ? 'statement' : 'statements'}`}>… and {remaining} more</button>
{/if}

<style>
  .statement-previews { display: grid; gap: 6px; min-width: 0; margin: 0; padding: 0; list-style: none; }
  .statement-preview { display: grid; grid-template-columns: max-content minmax(0, 1fr); align-items: start; gap: 14px; min-width: 0; padding: 7px 10px; border: 1px solid var(--line); border-radius: 4px; background: var(--surface-0); }
  .statement-preview.single { grid-template-columns: minmax(0, 1fr); }
  .statement-label { color: var(--ink-muted); font-size: 11px; line-height: 18px; }
  .statement-preview :global(pre) { max-height: 100px; margin: 0; overflow: auto; color: var(--ink); font: 12px/18px var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
  .more-statements { justify-self: start; min-height: 22px; padding: 1px 3px; border-color: transparent; background: transparent; color: var(--ink-muted); font-size: 12px; }
</style>
