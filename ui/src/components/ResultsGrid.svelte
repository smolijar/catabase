<script lang="ts">
  import { tick } from 'svelte';
  import Icon from './Icon.svelte';
  import { copyText } from '../lib/clipboard';
  import { cellKind, cellText, parseCellDraft, positionAfterReorder, prettyCell, selectionBounds, selectionText, sortedRows, visibleRowWindow } from '../lib/resultsGrid';
  import type { CellPosition, GridSort, IndexedRow } from '../lib/resultsGrid';
  import type { QueryResult } from '../lib/types';

  export let result: QueryResult;
  export let editMode = false;
  export let readOnly = false;
  export let disabled = false;
  export let changedCells: Array<{ rowIndex: number; column: string }> = [];
  export let onEdit: (rowIndex: number, column: string, newValue: unknown) => void = () => {};
  export let onMessage: (message: string) => void = () => {};

  let root: HTMLDivElement;
  let scrollElement: HTMLDivElement;
  let inspectorElement: HTMLElement | undefined;
  let scrollTop = 0;
  let viewportHeight = 300;
  const ROW_HEIGHT = 33;
  const HEADER_HEIGHT = 49;
  let previousRows: IndexedRow[] = [];
  let sort: GridSort = null;
  let widths: Record<string, number> = {};
  let anchor: CellPosition | null = null;
  let focus: CellPosition | null = null;
  let inspectorCell: { originalIndex: number; column: number } | null = null;
  let draft = '';
  let draftNull = false;
  let draftError = '';
  let resize: { column: string; startX: number; startWidth: number } | null = null;

  $: rows = sortedRows(result, sort);
  $: rowWindow = visibleRowWindow(rows.length, scrollTop, viewportHeight, ROW_HEIGHT, HEADER_HEIGHT);
  $: visibleRows = rows.slice(rowWindow.first, rowWindow.last);
  $: reconcileSelection(rows);
  $: bounds = anchor && focus ? selectionBounds(anchor, focus) : null;
  $: selectedCount = bounds ? (bounds.lastRow - bounds.firstRow + 1) * (bounds.lastColumn - bounds.firstColumn + 1) : 0;
  $: selectedColumn = inspectorCell ? result.columns[inspectorCell.column] : undefined;
  $: selectedValue = inspectorCell && selectedColumn ? result.rows[inspectorCell.originalIndex]?.[selectedColumn.name] : undefined;
  $: canEdit = Boolean(editMode && !readOnly && selectedColumn && !selectedColumn.generated);
  $: kind = selectedColumn ? cellKind(selectedColumn) : 'text';
  $: changed = new Set(changedCells.map((cell) => `${cell.rowIndex}\u001f${cell.column}`));
  $: totalWidth = 48 + result.columns.reduce((total, column) => total + columnWidth(column.name, widths), 0);
  $: resetDraft(result, inspectorCell, editMode);

  function reconcileSelection(nextRows: IndexedRow[]) {
    if (anchor && focus && previousRows.length) {
      const nextAnchor = positionAfterReorder(previousRows, nextRows, anchor);
      const nextFocus = positionAfterReorder(previousRows, nextRows, focus);
      if (nextAnchor && nextFocus) {
        anchor = nextAnchor;
        focus = nextFocus;
      } else { anchor = null; focus = null; }
    }
    previousRows = nextRows;
  }

  function ensureRowVisible(row: number) {
    if (!scrollElement) return;
    const rowTop = HEADER_HEIGHT + row * ROW_HEIGHT;
    if (rowTop < scrollElement.scrollTop + HEADER_HEIGHT) scrollElement.scrollTop = rowTop - HEADER_HEIGHT;
    else if (rowTop + ROW_HEIGHT > scrollElement.scrollTop + viewportHeight) scrollElement.scrollTop = rowTop + ROW_HEIGHT - viewportHeight;
    scrollTop = scrollElement.scrollTop;
  }

  function columnWidth(name: string, currentWidths = widths) {
    return currentWidths[name] ?? Math.min(320, Math.max(160, name.length * 8 + 55));
  }

  function setWidth(name: string, width: number) {
    widths = { ...widths, [name]: Math.round(Math.max(90, Math.min(900, width))) };
  }

  function startResize(event: PointerEvent, column: string) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    resize = { column, startX: event.clientX, startWidth: columnWidth(column) };
  }

  function moveResize(event: PointerEvent) {
    if (resize) setWidth(resize.column, resize.startWidth + event.clientX - resize.startX);
  }

  function resizeKey(event: KeyboardEvent, name: string) {
    if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
      event.preventDefault();
      setWidth(name, columnWidth(name) + (event.key === 'ArrowLeft' ? -1 : 1) * (event.shiftKey ? 50 : 10));
    } else if (event.key === 'Home') {
      event.preventDefault();
      setWidth(name, 160);
    }
  }

  function toggleSort(column: string) {
    sort = sort?.column !== column ? { column, direction: 'asc' }
      : sort.direction === 'asc' ? { column, direction: 'desc' } : null;
    anchor = null;
    focus = null;
  }

  function selectCell(row: number, column: number, extend = false) {
    if (!extend || !anchor) anchor = { row, column };
    focus = { row, column };
    if (editMode && !extend) inspectCell(row, column);
  }

  async function inspectCell(row: number, column: number, focusPanel = false) {
    if (!rows[row]) return;
    inspectorCell = { originalIndex: rows[row].originalIndex, column };
    if (focusPanel) {
      await tick();
      const editor = inspectorElement?.querySelector<HTMLElement>('textarea:not(:disabled)')
        ?? inspectorElement?.querySelector<HTMLElement>('input:not(:disabled)');
      (editor ?? inspectorElement?.querySelector<HTMLElement>('.close-inspector'))?.focus();
    }
  }

  function inspectSelection() {
    if (focus) void inspectCell(focus.row, focus.column, true);
  }

  async function closeInspector() {
    const inspectedRow = inspectorCell ? rows.findIndex((entry) => entry.originalIndex === inspectorCell?.originalIndex) : -1;
    const destination = focus ?? (inspectorCell && inspectedRow >= 0 ? { row: inspectedRow, column: inspectorCell.column } : null);
    inspectorCell = null;
    if (!destination) return;
    focus = destination;
    anchor ??= destination;
    ensureRowVisible(destination.row);
    await tick();
    root.querySelector<HTMLButtonElement>(`[data-cell="${destination.row}:${destination.column}"]`)?.focus();
  }

  function inspectorKey(event: KeyboardEvent) {
    if (event.key === 'Escape' && inspectorElement?.contains(event.target as Node)) {
      event.preventDefault();
      void closeInspector();
    }
  }

  function resetDraft(data: QueryResult, cell: typeof inspectorCell, _editMode: boolean) {
    if (!cell || !data.columns[cell.column]) return;
    const column = data.columns[cell.column];
    const value = data.rows[cell.originalIndex]?.[column.name];
    draftNull = value == null;
    draft = value == null ? (cellKind(column) === 'boolean' ? 'false' : '') : prettyCell(value, column, data.jsonValuesAsText);
    draftError = '';
  }

  function stageEdit() {
    if (!canEdit || disabled || !inspectorCell || !selectedColumn) return;
    try {
      const next = parseCellDraft(selectedColumn, draft, draftNull);
      if ((next == null && selectedValue == null) || (next != null && selectedValue != null && cellText(next, selectedColumn, true) === cellText(selectedValue, selectedColumn, result.jsonValuesAsText))) {
        draftError = '';
        onMessage('The value is unchanged.');
        return;
      }
      onEdit(inspectorCell.originalIndex, selectedColumn.name, next);
      draftError = '';
      onMessage('Cell change staged. Review and apply your pending changes to save it.');
    } catch (error) {
      draftError = error instanceof Error ? error.message : String(error);
    }
  }

  async function copySelection() {
    if (!anchor || !focus) return;
    try {
      await copyText(selectionText(rows, result.columns, anchor, focus, result.jsonValuesAsText));
      onMessage(`${selectedCount === 1 ? 'Cell' : `${selectedCount} cells`} copied.`);
    } catch (error) {
      onMessage(`Copy failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  async function copyValue() {
    if (!selectedColumn) return;
    try {
      await copyText(prettyCell(selectedValue, selectedColumn, result.jsonValuesAsText));
      onMessage('Cell value copied.');
    } catch (error) {
      onMessage(`Copy failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  async function cellKey(event: KeyboardEvent, row: number, column: number) {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'c') {
      event.preventDefault();
      await copySelection();
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'a') {
      event.preventDefault();
      anchor = { row: 0, column: 0 };
      focus = { row: rows.length - 1, column: result.columns.length - 1 };
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      selectCell(row, column, event.shiftKey);
      await inspectCell(row, column, true);
      return;
    }
    if (event.key === 'Escape') {
      inspectorCell = null;
      anchor = { row, column };
      focus = { row, column };
      return;
    }
    let nextRow = row;
    let nextColumn = column;
    if (event.key === 'ArrowDown') nextRow += 1;
    else if (event.key === 'ArrowUp') nextRow -= 1;
    else if (event.key === 'ArrowLeft') nextColumn -= 1;
    else if (event.key === 'ArrowRight') nextColumn += 1;
    else if (event.key === 'Home') { nextColumn = 0; if (event.metaKey || event.ctrlKey) nextRow = 0; }
    else if (event.key === 'End') { nextColumn = result.columns.length - 1; if (event.metaKey || event.ctrlKey) nextRow = rows.length - 1; }
    else return;
    event.preventDefault();
    nextRow = Math.max(0, Math.min(rows.length - 1, nextRow));
    nextColumn = Math.max(0, Math.min(result.columns.length - 1, nextColumn));
    selectCell(nextRow, nextColumn, event.shiftKey);
    ensureRowVisible(nextRow);
    await tick();
    root.querySelector<HTMLButtonElement>(`[data-cell="${nextRow}:${nextColumn}"]`)?.focus();
  }

  function isSelected(row: number, column: number, selection: ReturnType<typeof selectionBounds> | null) {
    return selection && row >= selection.firstRow && row <= selection.lastRow && column >= selection.firstColumn && column <= selection.lastColumn;
  }
</script>

<svelte:window on:pointermove={moveResize} on:pointerup={() => (resize = null)} on:pointercancel={() => (resize = null)} on:keydown={inspectorKey} />

<div class="results-grid" class:resizing={Boolean(resize)} bind:this={root}>
  <div class="grid-tools">
    <span class="selection-label" aria-live="polite">{selectedCount ? `${selectedCount} cell${selectedCount === 1 ? '' : 's'} selected` : 'Select a cell · Shift-click to select a range'}</span>
    {#if sort}<span class="sort-scope">Sorting {rows.length} loaded rows only</span><button class="subtle" on:click={() => { sort = null; anchor = null; focus = null; }}>Reset sort</button>{/if}
    <span class="tools-spacer"></span>
    <button disabled={!focus} on:click={copySelection} title="Copy selected cells as tab-separated text (⌘/Ctrl+C)"><Icon name="copy" size={13} /> Copy selection</button>
    <button disabled={!focus} on:click={inspectSelection}>Cell details</button>
  </div>
  <div class="grid-content">
    <div class="grid-scroll" bind:this={scrollElement} bind:clientHeight={viewportHeight} on:scroll={() => (scrollTop = scrollElement.scrollTop)}>
      <table style:width={`${totalWidth}px`} aria-rowcount={rows.length + 1} aria-label="Query result; column sorting applies only to loaded rows">
        <colgroup><col style:width="48px" />{#each result.columns as column}<col style:width={`${columnWidth(column.name, widths)}px`} />{/each}</colgroup>
        <thead>
          <tr>
            <th class="row-number" scope="col" title="Original result row number">#</th>
            {#each result.columns as column, columnIndex (columnIndex)}
              <th scope="col" aria-sort={sort?.column === column.name ? sort.direction === 'asc' ? 'ascending' : 'descending' : 'none'}>
                <button class="column-sort" on:click={() => toggleSort(column.name)} title={`Sort ${column.name} within loaded rows; click again for descending, then original order`}>
                  <span class="column-name">{column.name}</span>
                  <span class="column-direction" aria-hidden="true">{sort?.column === column.name ? sort.direction === 'asc' ? '↑' : '↓' : '↕'}</span>
                  <small>{column.typeName}{column.primaryKey ? ' · PK' : ''}{column.generated ? ' · generated' : ''}</small>
                </button>
                <button class="column-resize" aria-label={`Resize ${column.name} column, ${columnWidth(column.name, widths)} pixels`} title="Drag to resize. Arrow keys adjust width; Shift for larger steps." on:pointerdown={(event) => startResize(event, column.name)} on:keydown={(event) => resizeKey(event, column.name)}><span></span></button>
              </th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#if rowWindow.top}<tr class="spacer-row" aria-hidden="true"><td colspan={result.columns.length + 1} style:height={`${rowWindow.top}px`}></td></tr>{/if}
          {#each visibleRows as entry, visibleIndex (entry.originalIndex)}
            {@const rowIndex = rowWindow.first + visibleIndex}
            <tr class:alternate={rowIndex % 2 === 1} aria-rowindex={rowIndex + 2}>
              <th scope="row" class="row-number">{entry.originalIndex + 1}</th>
              {#each result.columns as column, columnIndex (columnIndex)}
                <td class:selected={isSelected(rowIndex, columnIndex, bounds)} class:changed={changed.has(`${entry.originalIndex}\u001f${column.name}`)}>
                  <button
                    class="cell"
                    class:null-value={entry.row[column.name] == null}
                    class:empty-value={entry.row[column.name] === '' && cellKind(column) !== 'json'}
                    data-cell={`${rowIndex}:${columnIndex}`}
                    tabindex={focus && focus.row >= rowWindow.first && focus.row < rowWindow.last ? focus.row === rowIndex && focus.column === columnIndex ? 0 : -1 : visibleIndex === 0 && columnIndex === 0 ? 0 : -1}
                    aria-label={`Row ${entry.originalIndex + 1}, ${column.name}: ${entry.row[column.name] === '' && cellKind(column) !== 'json' ? 'empty string' : cellText(entry.row[column.name], column, result.jsonValuesAsText).slice(0, 200)}`}
                    aria-pressed={Boolean(isSelected(rowIndex, columnIndex, bounds))}
                    title="Click to select, Shift-click for a range, double-click or Enter for details"
                    on:click={(event) => selectCell(rowIndex, columnIndex, event.shiftKey)}
                    on:focus={(event) => { if (!focus || ((focus.row < rowWindow.first || focus.row >= rowWindow.last) && event.currentTarget.matches(':focus-visible'))) selectCell(rowIndex, columnIndex); }}
                    on:dblclick={() => inspectCell(rowIndex, columnIndex, true)}
                    on:keydown={(event) => cellKey(event, rowIndex, columnIndex)}
                  >{entry.row[column.name] === '' && cellKind(column) !== 'json' ? 'empty string' : cellText(entry.row[column.name], column, result.jsonValuesAsText)}</button>
                </td>
              {/each}
            </tr>
          {/each}
          {#if rowWindow.bottom}<tr class="spacer-row" aria-hidden="true"><td colspan={result.columns.length + 1} style:height={`${rowWindow.bottom}px`}></td></tr>{/if}
        </tbody>
      </table>
    </div>
    {#if inspectorCell && selectedColumn}
      <aside class="cell-inspector" aria-label="Cell details" bind:this={inspectorElement}>
        <div class="inspector-title"><strong title={selectedColumn.name}>{selectedColumn.name}</strong><button class="close-inspector" aria-label="Close cell details" on:click={closeInspector}><Icon name="close" size={14} /></button></div>
        <p class="cell-meta">Row {inspectorCell.originalIndex + 1} · {selectedColumn.typeName}{selectedColumn.generated ? ' · generated' : ''}{selectedColumn.nullable === false ? ' · not nullable' : ''}</p>
        {#if canEdit}
          <form on:submit|preventDefault={stageEdit}>
            <div class="value-mode"><span>Value</span><label><input type="checkbox" bind:checked={draftNull} disabled={disabled || selectedColumn.nullable === false} on:change={() => (draftError = '')} /> Set NULL</label></div>
            {#if draftNull}
              <div class="null-preview">NULL <span>No value will be stored.</span></div>
            {:else if kind === 'boolean'}
              <div class="boolean-options" role="group" aria-label="Boolean value">
                <label class:chosen={draft === 'true'}><input type="radio" bind:group={draft} value="true" {disabled} /> true</label>
                <label class:chosen={draft === 'false'}><input type="radio" bind:group={draft} value="false" {disabled} /> false</label>
              </div>
            {:else}
              <textarea aria-label={`New value for ${selectedColumn.name}`} aria-invalid={Boolean(draftError)} bind:value={draft} {disabled} spellcheck="false" inputmode={kind === 'integer' || kind === 'numeric' ? 'decimal' : 'text'} on:input={() => (draftError = '')} on:keydown={(event) => { if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') { event.preventDefault(); event.stopPropagation(); stageEdit(); } }}></textarea>
            {/if}
            {#if draftError}<p class="draft-error" role="alert">{draftError}</p>{/if}
            <p class="edit-hint">{kind === 'text' ? 'An empty value is an empty string. Select Set NULL to remove the value.' : kind === 'json' ? 'JSON is validated before staging. JSON null and SQL NULL are distinct.' : kind === 'numeric' || kind === 'integer' ? 'Numbers retain their full precision.' : 'Choose a value, then stage the change.'}</p>
            <div class="inspector-actions"><button class="stage-button" type="submit" {disabled}>Stage change</button><button type="button" {disabled} on:click={() => resetDraft(result, inspectorCell, editMode)}>Reset</button></div>
          </form>
        {:else}
          <pre class:null-preview={selectedValue == null} class="cell-value">{prettyCell(selectedValue, selectedColumn, result.jsonValuesAsText)}</pre>
          {#if selectedValue === '' && kind !== 'json'}<p class="edit-hint">Empty string (0 characters)</p>{/if}
          <button on:click={copyValue}><Icon name="copy" size={13} /> Copy full value</button>
          <p class="edit-hint">{selectedColumn.generated ? 'This value is generated by the database and cannot be edited.' : readOnly ? 'This result is read-only.' : 'Enable Edit cells to change this value.'}</p>
        {/if}
      </aside>
    {/if}
  </div>
</div>

<style>
  .results-grid { flex: 1; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; color: #e0e7f0; background: #151a23; container-type: inline-size; }
  .grid-tools { display: flex; align-items: center; gap: 8px; min-height: 37px; padding: 5px 12px; flex-shrink: 0; overflow-x: auto; border-bottom: 1px solid #303a48; font-size: 11px; background: #1a202b; }
  .grid-tools button { flex-shrink: 0; }
  .selection-label { color: #b9c5d6; white-space: nowrap; }
  .sort-scope { color: #f2ce84; }
  .tools-spacer { flex: 1; }
  button { display: inline-flex; align-items: center; justify-content: center; gap: 5px; min-height: 25px; padding: 3px 8px; border: 1px solid #3c485b; border-radius: 5px; background: #242e3c; color: #e4ebf5; box-shadow: none; font-size: 11px; }
  button:hover:not(:disabled) { background: #303e50; border-color: #6d8099; color: #fff; }
  button:focus-visible { outline: 2px solid #90baff; outline-offset: -2px; }
  button:disabled { opacity: .45; }
  .subtle { border: none; background: none; color: #bac9de; }
  .grid-content { position: relative; display: flex; flex: 1; min-height: 0; min-width: 0; }
  .grid-scroll { flex: 1; min-width: 0; min-height: 0; overflow: auto; overscroll-behavior: contain; }
  table { table-layout: fixed; border-collapse: separate; border-spacing: 0; font-size: 12px; }
  thead { position: sticky; top: 0; z-index: 2; }
  th, td { padding: 0; max-width: none; min-width: 0; text-align: left; border: 0; border-right: 1px solid #2d3745; border-bottom: 1px solid #2d3745; white-space: nowrap; overflow: hidden; }
  th { position: relative; top: auto; z-index: auto; background: #232c39; color: #e8eef8; }
  .row-number { width: 48px; padding: 0 9px; text-align: right; font: 10px ui-monospace, SFMono-Regular, Menlo, monospace; color: #a2b0c3; }
  tbody .row-number { background: #1a212c; }
  .column-sort { position: relative; display: grid; grid-template-columns: minmax(0, 1fr) 14px; width: 100%; height: 48px; min-height: 48px; padding: 7px 16px 6px 10px; border: none; border-radius: 0; text-align: left; background: transparent; }
  .column-sort:hover:not(:disabled) { background: #2b384a; }
  .column-name { overflow: hidden; text-overflow: ellipsis; font-weight: 600; font-size: 12px; }
  .column-direction { color: #a4bad9; }
  .column-sort small { grid-column: 1 / -1; display: block; overflow: hidden; text-overflow: ellipsis; font-size: 10px; line-height: 1.2; color: #b0bfd1; font-weight: 400; }
  .column-resize { position: absolute; right: 0; top: 0; width: 9px; height: 100%; min-height: 0; padding: 0; border: 0; border-radius: 0; background: transparent; cursor: col-resize; touch-action: none; }
  .column-resize span { width: 1px; height: 22px; background: #667990; }
  .column-resize:hover:not(:disabled) { background: #5275a3; }
  td { background: #151b25; }
  tr.alternate td { background: #19202b; }
  td.selected, tr.alternate td.selected { background: #2a4264; }
  td.changed { box-shadow: inset 3px 0 #f0c56b; }
  .cell { display: block; width: 100%; height: 32px; min-height: 32px; padding: 7px 10px; overflow: hidden; text-overflow: ellipsis; border: 0; border-radius: 0; background: transparent; text-align: left; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11px; font-weight: 400; line-height: 17px; white-space: nowrap; user-select: none; }
  .spacer-row td { padding: 0; border: 0; background: #151b25; }
  .cell:hover:not(:disabled) { background: #26364b; }
  .cell:active:not(:disabled) { transform: none; }
  .cell.null-value, .cell.empty-value { font-style: italic; color: #a1b0c4; }
  .cell.empty-value { font-size: 10px; }
  .resizing, .resizing .cell { cursor: col-resize; user-select: none; }
  .cell-inspector { flex: 0 0 330px; max-width: 46%; overflow: auto; padding: 12px; border-left: 1px solid #46546a; background: #1c2430; }
  .inspector-title { display: flex; align-items: center; gap: 8px; }
  .inspector-title strong { flex: 1; overflow: hidden; text-overflow: ellipsis; font-size: 13px; }
  .close-inspector { min-height: 24px; width: 24px; padding: 3px; background: transparent; border: none; }
  .cell-meta { margin: 3px 0 13px; font-size: 11px; color: #adbed3; }
  .cell-value { margin: 0 0 10px; padding: 10px; min-height: 65px; max-height: 320px; overflow: auto; background: #131a23; border: 1px solid #36445a; border-radius: 5px; white-space: pre-wrap; overflow-wrap: anywhere; font: 12px/1.55 ui-monospace, SFMono-Regular, Menlo, monospace; }
  .value-mode { display: flex; justify-content: space-between; gap: 8px; align-items: center; margin-bottom: 8px; font-size: 11px; }
  .value-mode label { display: flex; align-items: center; gap: 6px; color: #dae6f6; font-size: 11px; }
  input[type='checkbox'], input[type='radio'] { appearance: auto; width: 14px; height: 14px; min-height: 0; padding: 0; margin: 0; accent-color: #8ab9fc; }
  textarea { display: block; width: 100%; min-height: 100px; resize: vertical; padding: 9px; border: 1px solid #596d87; border-radius: 5px; background: #111a26; color: #eef4ff; font: 12px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; box-sizing: border-box; }
  textarea:focus { outline: 2px solid #86b1f1; outline-offset: 1px; }
  textarea[aria-invalid='true'] { border-color: #f29292; }
  .null-preview { padding: 12px; border: 1px dashed #556a85; border-radius: 5px; color: #b5c5da; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
  .null-preview span { display: block; margin-top: 4px; font-family: inherit; font-size: 10px; }
  .boolean-options { display: flex; gap: 8px; }
  .boolean-options label { display: flex; align-items: center; gap: 8px; flex: 1; padding: 10px; border: 1px solid #425674; border-radius: 5px; background: #161e2b; font-size: 12px; }
  .boolean-options label.chosen { border-color: #8cb6f3; background: #293e5d; }
  .edit-hint { margin: 9px 0 11px; font-size: 11px; color: #b9c6d8; line-height: 1.5; }
  .draft-error { color: #ffb6b6; font-size: 11px; line-height: 1.5; margin: 8px 0; }
  .inspector-actions { display: flex; gap: 8px; }
  .stage-button { background: #345684; border-color: #6c95cb; }
  .stage-button:hover:not(:disabled) { background: #436a9e; }
  @media (max-width: 1000px) { .selection-label { max-width: 170px; overflow: hidden; text-overflow: ellipsis; } .grid-tools { gap: 5px; } .cell-inspector { flex-basis: 280px; } }
  @container (max-width: 640px) { .cell-inspector { position: absolute; z-index: 3; top: 0; bottom: 0; right: 0; width: min(330px, 100%); max-width: 100%; } .selection-label { max-width: 110px; } .sort-scope { display: none; } }
</style>
