<script lang="ts">
  // UI-only fixture: synthetic credentials; no database or persistence APIs.
  import ConnectionEditor from '../../src/components/ConnectionEditor.svelte';
  import Modal from '../../src/components/Modal.svelte';
  import type { ConnectionInput } from '../../src/lib/types';
  import type { ConnectionAppearance } from '../../src/lib/appearance';
  let connection: ConnectionInput = { id: 'fixture', name: 'Preview connection', host: 'localhost', port: 5432,
    database: 'zenbase', username: 'zenbase', password: 'synthetic-original-password', ssl: false };
  let appearance: ConnectionAppearance = { color: 'none', environment: 'LOCAL' };
  let open = true;
  let feedback = '';
  function report(action: string) {
    feedback = `${action}: ${connection.name}, ${connection.host}:${connection.port}/${connection.database}, ${connection.username}, SSL ${connection.ssl}; original password ${connection.password === 'synthetic-original-password' ? 'preserved' : 'changed'}.`;
  }
</script>

<button on:click={() => open = true}>Open connection editor</button>
{#if open}
  <Modal label="Connection editor preview" onClose={() => open = false}>
    <ConnectionEditor bind:connection bind:appearance {feedback} onClose={() => open = false}
      onSave={() => report('Save')} onTest={() => report('Test')} />
  </Modal>
{/if}
