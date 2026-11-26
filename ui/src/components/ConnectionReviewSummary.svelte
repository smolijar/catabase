<script lang="ts">
  import type { ConnectionAppearance } from '../lib/appearance';
  import type { ConnectionRecord } from '../lib/types';

  export let connection: ConnectionRecord;
  export let appearance: ConnectionAppearance | undefined;
</script>

<div class="connection-review" data-conn={appearance?.color ?? 'none'}>
  <strong>{connection.name}</strong> <span>{connection.username}@{connection.database}</span>
  {#if appearance?.environment}<span class="environment">{appearance.environment}</span>{/if}
</div>

<style>
  .connection-review { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 8px 12px; border: 1px solid var(--line); border-left: 3px solid var(--accent); border-radius: 3px; background: var(--surface-1); }
  .connection-review span { color: var(--ink-muted); font: 11px/16px var(--font-mono); }
  .connection-review .environment { color: var(--accent); background: var(--accent-soft); padding: 1px 5px; border-radius: 2px; }
</style>
