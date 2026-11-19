<script lang="ts">
  import Modal from './Modal.svelte';
  import Switch from './Switch.svelte';
  import Icon from './Icon.svelte';
  import { copyText } from '../lib/clipboard';
  import { api, errorMessage } from '../lib/ipc';
  import { copyableReport, diagnosticBody, issueDraft } from '../lib/diagnostics';
  import type { DiagnosticReport } from '../lib/diagnostics';
  import { literalTextAttributes } from '../lib/textInput';

  export let report: DiagnosticReport;
  export let onClose: () => void;
  let title = report.title;
  let includeSql = false;
  let body = report.body;
  let status = '';
  let failed = false;
  let busy = false;
  $: fullBody = diagnosticBody({ ...report, body }, includeSql);
  $: draft = issueDraft(title, fullBody);

  async function copy() {
    try { await copyText(copyableReport(title, fullBody)); status = 'Report copied.'; failed = false; }
    catch (error) { status = `Could not copy. Select the report text to copy it manually. ${errorMessage(error)}`; failed = true; }
  }

  async function openIssue() {
    busy = true;
    try {
      if (draft.copyRequired) await copyText(copyableReport(title, fullBody));
      await api.openDiagnosticIssue(draft.title, draft.body);
      status = draft.copyRequired ? 'GitHub opened. Paste the copied report, then review and submit.' : 'GitHub opened. Review the draft there before submitting.';
      failed = false;
    } catch (error) { status = `${errorMessage(error)} You can copy the report instead.`; failed = true; }
    finally { busy = false; }
  }
</script>

<Modal label={report.newCrash ? 'Catabase closed unexpectedly' : 'Report a problem'} {onClose} locked={busy}>
  <div class="modal diagnostic-dialog">
    <div class="panel-head"><h2>{report.newCrash ? 'Catabase closed unexpectedly' : 'Report a problem'}</h2><button disabled={busy} on:click={onClose}>Close</button></div>
    <p>{report.note}</p>
    <p class="privacy">Nothing is sent automatically. GitHub opens a draft for you to review and submit. Technical diagnostics omit credentials, result rows, clipboard contents and personal paths.</p>
    <label>Issue title<input {...literalTextAttributes} maxlength="200" bind:value={title} /></label>
    <div class="sql-sharing">
      <Switch label="Include latest/running SQL" checked={includeSql} disabled={!report.query || busy} onChange={enabled => includeSql = enabled} />
      <p>{report.query ? 'Off by default. SQL can contain sensitive values; review it before sharing.' : 'No SQL context is available for this report.'}</p>
    </div>
    <label>Report<textarea {...literalTextAttributes} rows="14" bind:value={body}></textarea></label>
    {#if includeSql && report.query}<label>SQL to include ({report.query.fromCrash ? 'crashed session' : 'this session'}, {report.query.phase}{report.query.truncated ? ', truncated' : ''})<textarea {...literalTextAttributes} rows="4" readonly value={report.query.sql}></textarea></label>{/if}
    {#if draft.copyRequired}<p class="privacy">This report is too long for a URL. Opening GitHub will copy the full report for you to paste into the issue.</p>{/if}
    {#if status}<p class:error-text={failed} role="status">{status}</p>{/if}
    <div class="row-actions"><button disabled={busy} on:click={copy}><Icon name="copy" size={14} />Copy report</button><button class="primary" disabled={busy || !title.trim() || !body.trim()} on:click={openIssue}><Icon name="open" size={14} />{busy ? 'Opening…' : 'Open GitHub draft'}</button></div>
  </div>
</Modal>

<style>
  .diagnostic-dialog { gap: 16px; }
  p { margin: 0; font-size: 12px; line-height: 18px; }
  .privacy, .sql-sharing p { color: var(--ink-muted); }
  .sql-sharing { display: grid; gap: 6px; }
  textarea { min-height: 180px; max-height: 40vh; resize: vertical; font: 11px/17px var(--font-mono); }
  .row-actions { justify-content: flex-end; }
</style>
