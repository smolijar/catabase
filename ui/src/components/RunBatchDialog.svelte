<script lang="ts">
  import Modal from './Modal.svelte';
  import ConnectionReviewSummary from './ConnectionReviewSummary.svelte';
  import StatementPreviews from './StatementPreviews.svelte';
  import type { ConnectionAppearance } from '../lib/appearance';
  import type { ConnectionRecord } from '../lib/types';

  export let statements: string[];
  export let connection: ConnectionRecord;
  export let appearance: ConnectionAppearance | undefined;
  export let themeKey: string;
  export let onConfirm: () => void;
  export let onClose: () => void;
  $: statementCount = statements.length;
</script>

<Modal label={`Run ${statementCount} statements`} {onClose}>
  <div class="modal" data-conn={appearance?.color ?? 'none'}>
    <h2>Run {statementCount} statements?</h2>
    <p>Review the selected SQL and connection before running this batch.</p>
    <ConnectionReviewSummary {connection} {appearance} />
    <StatementPreviews {statements} {themeKey} />
    <div class="row-actions">
      <!-- svelte-ignore a11y_autofocus (Keep the confirmation focused on Cancel when it opens.) -->
      <button autofocus on:click={onClose}>Cancel</button>
      <button class="primary" on:click={onConfirm}>Run {statementCount} statements</button>
    </div>
  </div>
</Modal>
