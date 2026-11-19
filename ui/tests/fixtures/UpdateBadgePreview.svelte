<script lang="ts">
  import Cat from '../../src/components/Cat.svelte';
  import Icon from '../../src/components/Icon.svelte';
  import '@fontsource/ibm-plex-sans/latin-400.css';
  import '@fontsource/ibm-plex-sans/latin-500.css';
  import '@fontsource/silkscreen/latin-400.css';
  import '../../src/tokens.css';
  import '../../src/app.css';
</script>

<main class="badge-preview">
  <section><div class="caption">Current</div><header class="toolbar"><div class="lights"><i></i><i></i><i></i></div><div class="brand"><Cat animate={false} /><span class="wordmark"><b>CAT</b>ABASE</span></div><button class="update-pill">Update available</button></header></section>
  <section><div class="caption">Proposed</div><header class="toolbar"><div class="lights"><i></i><i></i><i></i></div><div class="brand"><Cat animate={false} /><span class="wordmark"><b>CAT</b>ABASE</span></div><button class="update-badge"><Icon name="download" size={12} /><span>Update available</span></button></header></section>
  <p>Compact, square corners. Subtle border and a small accent icon.</p>
</main>

<style>
  :global(html) { background: #141518; }
  .badge-preview { width: 620px; padding: 30px; display: grid; gap: 24px; margin: 0 auto; }
  section { border: 1px solid var(--line); border-radius: 6px; overflow: hidden; }
  .caption { background: var(--surface-1); color: var(--ink-muted); font-size: 11px; padding: 8px 12px; border-bottom: 1px solid var(--line); }
  header.toolbar { height: 56px; border: 0; gap: 16px; padding: 0 16px; }
  .lights { display: flex; gap: 7px; margin-right: 10px; }
  .lights i { width: 11px; height: 11px; border-radius: 50%; background: #ff6058; }
  .lights i:nth-child(2) { background: #ffbd2f; }
  .lights i:nth-child(3) { background: #28c840; }
  .update-pill { flex-shrink: 0; min-height: 24px; border-color: var(--brand); border-radius: 999px; padding: 2px 9px; font-size: 11px; font-weight: 500; line-height: 16px; white-space: nowrap; }
  p { margin: 0; font-size: 12px; color: var(--ink-faint); }
</style>
