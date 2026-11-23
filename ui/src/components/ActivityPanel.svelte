<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import ActionMenu from './ActionMenu.svelte';
  import Dropdown from './Dropdown.svelte';
  import { activityLog, clearActivityLog } from '../lib/activity';
  import type { ActivityCategory, ActivityEvent } from '../lib/activity';
  import { activityCategoryLabels, activitySummary, executionSummary, formatActivityLine, formatExecutionLog, logTimestamp } from '../lib/executionLog';
  import { api, errorMessage } from '../lib/ipc';
  import type { HistoryEntry } from '../lib/types';

  export let historyRevision: number;
  export let connectionLabels: Record<string, string>;
  export let onOpen: (entry: HistoryEntry) => void;
  export let onCopy: (text: string) => void;

  const filters = [
    { value: 'all', label: 'All activity' },
    { value: 'sql', label: 'SQL' },
    { value: 'clipboard', label: 'Clipboard' },
    { value: 'connection', label: 'Connections' },
    { value: 'schema', label: 'Schema' }
  ];
  type TimelineItem =
    | { key: string; category: 'sql'; at: string; entry: HistoryEntry }
    | { key: string; category: ActivityCategory; at: string; event: ActivityEvent };

  let filter = 'all';
  let statements: HistoryEntry[] = [];
  let timeline: TimelineItem[] = [];
  let loading = true;
  let loadError = '';
  let request = 0;
  let activityFeed: HTMLOListElement | undefined;
  let viewport: HTMLDivElement | undefined;
  let following = true;
  let entryMenu: ActionMenu;
  let menuKey: string | null = null;

  $: void refreshStatements(historyRevision);
  $: clearedAt = $activityLog.clearedAt;
  $: timeline = [
    ...[...statements].reverse()
      .filter(entry => clearedAt === null || Date.parse(entry.executedAt) > clearedAt)
      .map(entry => ({ key: `sql-${entry.id}`, category: 'sql' as const, at: entry.executedAt, entry })),
    ...$activityLog.events.map(event => ({ key: event.id, category: event.category, at: event.startedAt, event }))
  ].sort((left, right) => Date.parse(left.at) - Date.parse(right.at));
  $: visible = timeline.filter(item => filter === 'all' || item.category === filter);
  $: if (viewport) void followLatest(viewport, visible);
  $: menuItem = visible.find(item => item.key === menuKey);
  $: entryActions = menuItem?.category === 'sql'
    ? [{ id: 'copy', label: 'Copy execution log' }, { id: 'open', label: 'Open SQL and saved result' }]
    : [{ id: 'copy', label: 'Copy activity entry' }];
  onDestroy(() => { request++; });

  async function refreshStatements(_revision: number) {
    const current = ++request;
    try {
      const page = await api.historyPage(null, 0, 50);
      if (current !== request) return;
      statements = page.entries;
      loadError = '';
    } catch (error) {
      if (current === request) loadError = errorMessage(error);
    } finally {
      if (current === request) loading = false;
    }
  }

  function copyVisible() {
    onCopy(visible.map(formatEntryLine).join('\n'));
  }

  function clearLog() {
    following = true;
    menuKey = null;
    clearActivityLog();
  }

  async function followLatest(target: HTMLDivElement, _entries: TimelineItem[]) {
    if (!following) return;
    await tick();
    if (following && viewport === target) target.scrollTop = target.scrollHeight;
  }

  function updateScroll(event: Event & { currentTarget: HTMLDivElement }) {
    const target = event.currentTarget;
    following = target.scrollHeight - target.scrollTop - target.clientHeight <= 4;
  }

  function setFilter(value: string) {
    following = true;
    filter = value;
  }

  function copySelection(event: ClipboardEvent) {
    if (!activityFeed || !event.clipboardData || event.defaultPrevented) return;
    const selection = window.getSelection();
    if (!selection?.rangeCount || selection.isCollapsed) return;
    const range = selection.getRangeAt(0);
    if (!activityFeed.contains(range.commonAncestorContainer)) return;

    // The native clipboard can treat separately styled pieces as separate
    // lines. Clone the range to retain only the selected part of each entry.
    const selected = range.cloneContents();
    const rows = [...selected.querySelectorAll('.activity-entry')];
    const text = (rows.length ? rows : [selected])
      .map(row => row.textContent?.trim())
      .filter(Boolean)
      .join('\n');
    if (!text) return;
    event.clipboardData.setData('text/plain', text);
    event.preventDefault();
  }

  function summarizeEntry(item: TimelineItem, labels: Record<string, string>) {
    return item.category === 'sql'
      ? executionSummary(item.entry, connectionLabel(item.entry, labels))
      : activitySummary(item.event);
  }

  function formatEntryLine(item: TimelineItem) {
    return formatActivityLine(item.at, item.category, summarizeEntry(item, connectionLabels));
  }

  async function openEntryMenu(item: TimelineItem, anchor: HTMLButtonElement) {
    menuKey = item.key;
    const bounds = anchor.getBoundingClientRect();
    anchor.focus({ preventScroll: true });
    await tick();
    await entryMenu.openAt(bounds.left, bounds.bottom);
  }

  function selectEntryAction(action: string) {
    if (!menuItem) return;
    if (action === 'copy') onCopy(menuItem.category === 'sql'
      ? formatExecutionLog(menuItem.entry, connectionLabel(menuItem.entry, connectionLabels))
      : formatEntryLine(menuItem));
    if (action === 'open' && menuItem.category === 'sql') onOpen(menuItem.entry);
  }

  function connectionLabel(entry: HistoryEntry, labels: Record<string, string>): string {
    return labels[entry.connectionId ?? ''] ?? 'Unavailable connection';
  }

  const timeFormat = new Intl.DateTimeFormat(undefined, {
    hour: '2-digit', minute: '2-digit', second: '2-digit', fractionalSecondDigits: 3, hourCycle: 'h23'
  });

  function displayTime(timestamp: string) {
    const date = new Date(timestamp);
    return Number.isFinite(date.getTime()) ? timeFormat.format(date) : timestamp;
  }
</script>

<svelte:window on:copy={copySelection} />

<section class="activity-panel" aria-label="Activity log">
  <div class="panel-head">
    <h2>Activity</h2>
    <div class="log-actions">
      <button aria-label="Copy visible activity" disabled={!visible.length} on:click={copyVisible}>Copy log</button>
      <button title="Clear all activity" disabled={!timeline.length} on:click={clearLog}>Clear log</button>
    </div>
  </div>
  <div class="activity-scroll" bind:this={viewport} on:scroll={updateScroll}>
    {#if loadError && (filter === 'all' || filter === 'sql')}<p class="error-text" role="alert">Could not load SQL activity. {loadError}</p>{/if}
    {#if visible.length === 0}
      <p class="activity-empty">{loading && (filter === 'all' || filter === 'sql') ? 'Loading activity…' : 'No recent activity'}</p>
    {:else}
      <ol bind:this={activityFeed} class="activity-feed" aria-label="Recent activity" data-selectable-text>
        {#each visible as item (item.key)}
          {@const summary = summarizeEntry(item, connectionLabels)}
          <li class="activity-entry" data-category={item.category}>
            <time datetime={item.at} title={logTimestamp(item.at)}>[{displayTime(item.at)}]</time>{' '}<button
              class="activity-category"
              aria-label={`Actions for ${activityCategoryLabels[item.category]} at ${displayTime(item.at)}`}
              aria-haspopup="menu"
              title="Activity entry actions"
              on:click={event => openEntryMenu(item, event.currentTarget)}
            >[{activityCategoryLabels[item.category]}]</button>{' '}<span
              class="activity-summary"
              class:error-text={item.category === 'sql' ? item.entry.isError : item.event.status === 'failed'}
              class:interrupted={item.category !== 'sql' && item.event.status === 'interrupted'}
              title={summary}
            >{summary}</span>
          </li>
        {/each}
      </ol>
    {/if}
  </div>
  <div class="activity-filter">
    <Dropdown value={filter} options={filters} ariaLabel="Filter activity" onSelect={setFilter} />
  </div>
</section>
<ActionMenu bind:this={entryMenu} contextOnly ariaLabel="Activity entry actions" disabled={!menuItem} items={entryActions} onSelect={selectEntryAction} />

<style>
  .activity-panel { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .activity-scroll { display: flex; flex: 1; flex-direction: column; min-height: 0; overflow-y: auto; }
  .activity-filter { flex-shrink: 0; margin-top: 8px; }
  .activity-empty { margin: 0; color: var(--ink-muted); }
  .activity-feed { flex-shrink: 0; list-style: none; margin: auto 0 0; padding: 0; font: 11px/17px var(--font-mono); }
  .activity-entry { margin-bottom: 2px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .activity-entry time { color: var(--ink-faint); }
  .activity-summary.interrupted { color: var(--warn); }
  .log-actions { display: flex; gap: 12px; }
  .log-actions button, .activity-category { display: inline; min-height: 0; padding: 0; border: 0; background: transparent; }
  .log-actions button { color: var(--ink-muted); font-size: 12px; white-space: nowrap; }
  .log-actions button:hover:not(:disabled), .activity-category:hover { background: transparent; text-decoration: underline; }
  .activity-category { font-weight: 600; white-space: nowrap; }
  [data-category='sql'] .activity-category { color: var(--info); }
  [data-category='clipboard'] .activity-category { color: var(--conn-violet); }
  [data-category='connection'] .activity-category { color: var(--ok); }
  [data-category='schema'] .activity-category { color: var(--warn); }
</style>
