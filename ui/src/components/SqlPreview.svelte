<script lang="ts">
  import { onMount } from 'svelte';
  import { editor } from 'monaco-editor/esm/vs/editor/editor.api';
  import 'monaco-editor/esm/vs/basic-languages/sql/sql.contribution';

  export let sql: string;
  export let themeKey: string;

  let mounted = false;
  let highlighted = '';
  let revision = 0;

  onMount(() => {
    mounted = true;
    return () => { revision++; };
  });

  $: if (mounted) void highlight(sql, themeKey);

  async function highlight(value: string, _themeKey: string) {
    const current = ++revision;
    highlighted = '';
    try {
      // Monaco's renderer escapes the source text before wrapping it in token markup.
      const html = await editor.colorize(value, 'sql', { tabSize: 2 });
      if (current === revision) highlighted = html;
    } catch { /* Keep the plain SQL visible if tokenization fails. */ }
  }
</script>

<pre class="monaco-tokenized-source" aria-label="SQL preview">{#if highlighted}{@html highlighted}{:else}{sql}{/if}</pre>
