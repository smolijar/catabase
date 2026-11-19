<script lang="ts">
  import { onMount } from 'svelte';
  import { releaseLink, renderReleaseNotes } from '../lib/releaseNotes';
  export let source: string;
  export let onLink: (url: string) => Promise<void>;
  let element: HTMLDivElement;
  let mounted = false;
  let html = '';
  let error = '';
  $: html = mounted ? renderReleaseNotes(source) : '';

  onMount(() => {
    mounted = true;
    const click = (event: MouseEvent) => {
      const anchor = (event.target as Element).closest('a');
      if (!anchor?.hasAttribute('href')) return;
      event.preventDefault();
      const url = releaseLink(anchor.getAttribute('href') ?? '');
      if (!url) { error = 'Only links to the Catabase repository can be opened here.'; return; }
      error = '';
      void onLink(url).catch(() => { error = 'Could not open the link in your browser.'; });
    };
    // Delegate from a DOM listener so Markdown anchors keep native link semantics.
    element.addEventListener('click', click);
    return () => element.removeEventListener('click', click);
  });
</script>

<div class="release-notes" bind:this={element} data-selectable-text>
  {#if mounted}{@html html}{:else}<pre>{source}</pre>{/if}
</div>
{#if error}<p class="error-text" role="alert">{error}</p>{/if}

<style>
  .release-notes { max-height: 260px; overflow: auto; overflow-wrap: anywhere; font-size: 12px; line-height: 18px; padding-right: 6px; }
  .release-notes :global(p), .release-notes :global(ul), .release-notes :global(ol), .release-notes :global(blockquote) { margin: 10px 0; }
  .release-notes :global(h1), .release-notes :global(h2), .release-notes :global(h3), .release-notes :global(h4), .release-notes :global(h5), .release-notes :global(h6) { margin: 14px 0 6px; font-size: 13px; line-height: 18px; font-weight: 600; }
  .release-notes :global(ul), .release-notes :global(ol) { padding-left: 20px; }
  .release-notes :global(li) { margin: 4px 0; }
  .release-notes :global(li p) { margin: 4px 0; }
  .release-notes :global(a) { color: var(--accent); text-decoration: underline; text-underline-offset: 2px; }
  .release-notes :global(code) { font-size: 11px; padding: 1px 3px; background: var(--surface-3); border-radius: 2px; }
  .release-notes :global(pre) { padding: 8px; background: var(--surface-0); overflow: auto; white-space: pre; }
  .release-notes :global(pre code) { padding: 0; background: transparent; }
  .release-notes :global(blockquote) { margin-left: 0; padding-left: 10px; border-left: 2px solid var(--line-strong); color: var(--ink-muted); }
  .release-notes :global(table) { border-collapse: collapse; }
  .release-notes :global(th), .release-notes :global(td) { padding: 4px 6px; border: 1px solid var(--line); }
  .release-notes :global(hr) { border: 0; border-top: 1px solid var(--line); }
  .release-notes > :global(:first-child) { margin-top: 8px; }
</style>
