<script lang="ts">
  // UI-only fixture: these synthetic rows never invoke database or storage APIs.
  import ConnectionCard from '../../src/components/ConnectionCard.svelte';
  import type { ConnectionRecord } from '../../src/lib/types';

  let selected = 0;
  let action = 'No action yet';
  const connections: ConnectionRecord[] = Array.from({ length: 60 }, (_, index) => ({
    id: `fixture-${index}`, name: `zenbase docker ${index + 1}`, username: 'zenbase',
    host: 'localhost', port: 5432, database: 'zenbase', ssl: false,
    isActive: false, createdAt: '', updatedAt: ''
  }));
</script>

<div class="preview">
  <aside class="sidebar connections-sidebar">
    <div class="panel-head"><h2>Connections</h2><span>60 synthetic rows</span></div>
    <ul class="connection-list" aria-label="Saved connections">
      {#each connections as connection, index}
        <li><ConnectionCard {connection} active={selected === index}
          onActivate={() => { selected = index; action = `Selected ${connection.name}`; }}
          onEdit={() => { action = `Edit ${connection.name}`; }}
          onDelete={() => { action = `Delete ${connection.name}`; }} /></li>
      {/each}
    </ul>
  </aside>
  <p role="status">{action}</p>
</div>

<style>
  .preview { display: flex; height: 420px; margin: 20px; border: 1px solid #33415f; }
  .preview > p { padding: 20px; }
</style>
