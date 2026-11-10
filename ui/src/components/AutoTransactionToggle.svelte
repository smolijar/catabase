<script lang="ts">
  export let enabled = true;
  export let compact = false;
  export let disabled = false;
  export let busy = false;
  export let onChange: (enabled: boolean) => void;

  $: description = enabled
    ? 'Auto transaction is on. Read-only queries run normally. Writes and unrecognized SQL start a transaction for you to Commit or Rollback.'
    : 'Auto transaction is off. SQL runs normally unless you start a transaction with BEGIN.';
</script>

<button
  class="auto-transaction-toggle"
  class:status-link={compact}
  class:enabled
  aria-label="Auto transaction"
  aria-pressed={enabled}
  aria-busy={busy}
  tabindex={compact ? -1 : undefined}
  title={`${description} Changing this setting never commits or ends an open transaction.`}
  disabled={disabled || busy}
  on:click={() => onChange(!enabled)}
>
  <span>{compact ? 'Auto TX' : 'Auto transaction'}</span>
  <strong>{busy ? 'saving…' : enabled ? 'on' : 'off'}</strong>
</button>

<style>
  .auto-transaction-toggle { display: inline-flex; align-items: center; gap: 6px; flex-shrink: 0; }
  strong { font: inherit; }
  .enabled strong { color: var(--accent); }
</style>
