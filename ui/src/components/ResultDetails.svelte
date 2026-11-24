<script lang="ts">
  import Icon from './Icon.svelte';
  import SqlPreview from './SqlPreview.svelte';
  import type { QueryResult } from '../lib/types';
  import type { ResultSession } from '../lib/workspace';

  export let context: Omit<ResultSession, 'data'>;
  export let result: QueryResult | null;
  export let executedAtLabel: string;
  export let durationMs: number | undefined;
  export let outcome = '';
  export let themeKey: string;
  export let onCopyLog: (ids: number[]) => void;
</script>

<div class="result-details">
  <section aria-labelledby="result-statistics-heading">
    <h3 id="result-statistics-heading">Statistics</h3>
    <dl>
      <div><dt>Connection</dt><dd>{context.connectionName}</dd></div>
      <div><dt>Executed</dt><dd><time datetime={context.executedAt} title={context.executedAt}>{executedAtLabel}</time></dd></div>
      {#if result}
        <div><dt>Rows</dt><dd>{result.rowCount.toLocaleString()}{result.hasMore ? ' loaded · more available' : ''}</dd></div>
        <div><dt>Columns</dt><dd>{result.columns.length.toLocaleString()}</dd></div>
      {/if}
      {#if durationMs !== undefined}<div><dt>Duration</dt><dd>{durationMs.toLocaleString()} ms</dd></div>{/if}
      {#if outcome}<div><dt>Outcome</dt><dd>{outcome}</dd></div>{/if}
      {#if context.snapshot}<div><dt>Source</dt><dd>Query history · read only</dd></div>{/if}
    </dl>
  </section>
  <section aria-labelledby="result-sql-heading">
    <div class="details-heading">
      <h3 id="result-sql-heading">Executed SQL</h3>
      {#if context.historyEntryIds?.length}
        {@const ids = context.historyEntryIds}
        <button on:click={() => onCopyLog(ids)}><Icon name="copy" size={12} /> Copy execution log</button>
      {/if}
    </div>
    <SqlPreview sql={context.execution.sql} {themeKey} />
  </section>
</div>

<style>
  .result-details { display: grid; gap: 14px; padding: 12px; }
  .details-heading { display: flex; align-items: center; flex-wrap: wrap; justify-content: space-between; gap: 6px; margin-bottom: 7px; }
  h3 { margin: 0; font-size: 12px; font-weight: 600; }
  #result-statistics-heading { margin-bottom: 7px; }
  button { min-height: 22px; padding: 1px 6px; font-size: 11px; }
  dl { display: grid; grid-template-columns: repeat(auto-fit, minmax(120px, 1fr)); gap: 8px 12px; margin: 0; padding: 8px 10px; border: 1px solid var(--line); border-radius: 4px; }
  dt { color: var(--ink-muted); font-size: 10px; line-height: 15px; }
  dd { margin: 2px 0 0; font: 11px/16px var(--font-mono); overflow-wrap: anywhere; }
  .result-details :global(pre) { margin: 0; padding: 8px 10px; overflow: auto; border: 1px solid var(--line); border-radius: 4px; background: var(--surface-0); font: 12px/18px var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
