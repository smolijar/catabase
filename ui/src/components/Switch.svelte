<script lang="ts">
  export let label: string;
  export let checked = false;
  export let disabled = false;
  export let onChange: ((checked: boolean) => void) | undefined = undefined;
</script>

<label class="switch">
  <input type="checkbox" role="switch" bind:checked {disabled}
    on:change={(event) => onChange?.(event.currentTarget.checked)} />
  <span class="track" aria-hidden="true"></span>
  <span>{label}</span>
</label>

<style>
  .switch {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    width: fit-content;
    max-width: 100%;
    min-height: 26px;
    color: var(--ink);
    font-size: 13px;
    cursor: pointer;
  }

  input {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: 0;
    clip-path: inset(50%);
  }

  .track {
    position: relative;
    flex: 0 0 30px;
    height: 16px;
    border: 1px solid var(--line-strong);
    border-radius: 8px;
    background: var(--surface-3);
  }

  .track::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--ink-muted);
  }

  input:checked + .track { background: var(--accent); border-color: var(--accent); }
  input:checked + .track::after { transform: translateX(14px); background: var(--on-accent); }
  input:focus-visible + .track { outline: 2px solid var(--focus); outline-offset: 2px; }
  .switch:has(input:disabled) { opacity: .5; cursor: not-allowed; }
</style>
