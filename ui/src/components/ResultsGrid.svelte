<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  import { shortType } from '../lib/appearance';
  import ForeignKeyPicker from './ForeignKeyPicker.svelte';
  import CellValueEditor from './CellValueEditor.svelte';
  import ActionMenu from './ActionMenu.svelte';
  import PaneResizer from './PaneResizer.svelte';
  import { copyText, readClipboardText } from '../lib/clipboard';
  import { cellDisplayType, cellKind, cellText, columnHeaderInfo, parseCellDraft, positionAfterReorder, prettyCell, sameCellValue, selectedResult, selectionBounds, selectionText, sortedRows, visibleRowWindow } from '../lib/resultsGrid';
  import type { CellPosition, GridSort, IndexedRow } from '../lib/resultsGrid';
  import { foreignKeyDraft, foreignKeyTargets } from '../lib/foreignKeyEditing';
  import type { ForeignKeyTarget } from '../lib/foreignKeyEditing';
  import { cellDetailsWidthBounds, clampPaneWidth, DEFAULT_CELL_DETAILS_WIDTH, restoreCellDetailsWidth } from '../lib/sidebarLayout';
  import type { ColumnMeta, ForeignKeyLookup, ForeignKeyValues, QueryResult, SchemaItem } from '../lib/types';
  import type { CellEdit } from '../lib/workspace';

  export let result: QueryResult | null;
  export let hasDetails = false;
  export let editableColumns: string[] = [];
  export let editDisabledReason = '';
  export let readOnly = false;
  export let disabled = false;
  export let changedCells: Array<{ rowIndex: number; column: string; useDefault?: boolean }> = [];
  export let canLoadMore = false;
  export let loadMoreBlockedReason = '';
  export let onLoadMore: () => Promise<void> = async () => {};
  export let onEditMany: (rowIndex: number, edits: Record<string, CellEdit>) => Record<string, unknown> | null;
  export let schemaItems: SchemaItem[] = [];
  export let resultContext = '';
  export let onForeignKeyLookup: (request: ForeignKeyLookup, token: string) => Promise<ForeignKeyValues>;
  export let onCancelLookup: (token: string) => void = () => {};
  export let onMessage: (message: string) => void = () => {};
  export let status: Snippet | undefined = undefined;
  export let actions: Snippet | undefined = undefined;
  export let notices: Snippet | undefined = undefined;
  export let details: Snippet | undefined = undefined;
  export let children: Snippet | undefined = undefined;

  type InspectedCell = { originalIndex: number; column: number };
  type InspectorSource = { cell: InspectedCell; columnName: string; value: unknown; useDefault: boolean };
  type ResultView = 'grid' | 'record' | 'details';
  type InspectorDraft = {
    text: string;
    isNull: boolean;
    useDefault: boolean;
    error: string;
    foreignRow: Record<string, string | null> | null;
  };

  let root: HTMLDivElement;
  let scrollElement: HTMLDivElement | undefined;
  let recordScrollElement: HTMLDivElement | undefined;
  let inspectorElement: HTMLElement | undefined;
  let gridWidth = 1000;
  let inspectorWidth = DEFAULT_CELL_DETAILS_WIDTH;
  let resizingInspector = false;
  const CELL_DETAILS_WIDTH_KEY = 'catabase.cellDetailsWidth';
  let scrollTop = 0;
  let viewportHeight = 300;
  const ROW_HEIGHT = 28;
  const HEADER_HEIGHT = 28;
  let previousRows: IndexedRow[] = [];
  let sort: GridSort = null;
  let widths: Record<string, number> = {};
  let anchor: CellPosition | null = null;
  let focus: CellPosition | null = null;
  let view: ResultView = 'grid';
  const viewScroll = { grid: { top: 0, left: 0 }, record: { top: 0, left: 0 } };
  const views = [
    { id: 'grid', label: 'Grid', title: 'Grid view (Tab from a record value)' },
    { id: 'record', label: 'Record', title: 'Record view (Tab from a grid cell)' },
    { id: 'details', label: 'Details', title: 'Result statistics and executed SQL' }
  ] as const;
  let inspectorCell: InspectedCell | null = null;
  let inspectorSource: InspectorSource | null = null;
  let editorDraft: InspectorDraft = { text: '', isNull: false, useDefault: false, error: '', foreignRow: null };
  let foreignSearch = '';
  const retainedDrafts = new Map<string, Omit<InspectorSource, 'cell'> & { draft: InspectorDraft }>();
  let contextMenu: ActionMenu;
  let contextCell: InspectedCell | null = null;
  let pasteRequest = 0;
  let resize: { column: string; startX: number; startWidth: number } | null = null;
  let requestingMore = false;
  let requestedRows = -1;
  let lastScrollPosition = 0;
  let lastWheelAt = -Infinity;
  let touchPosition: { x: number; y: number } | null = null;

  $: columns = result?.columns ?? [];
  $: jsonValuesAsText = result?.jsonValuesAsText;
  $: rows = result ? sortedRows(result, sort) : [];
  $: headers = columns.map((column) => columnHeaderInfo(column, schemaItems, result?.rowKeys));
  $: canShowRecord = Boolean(rows.length && columns.length);
  $: resultTabs = views.filter((tab) => tab.id === 'grid' || (tab.id === 'record' ? canShowRecord : hasDetails));
  $: if ((view === 'record' && !canShowRecord) || (view === 'details' && !hasDetails)) view = 'grid';
  $: recordMode = view === 'record';
  $: rowWindow = visibleRowWindow(rows.length, scrollTop, viewportHeight, ROW_HEIGHT, HEADER_HEIGHT);
  $: visibleRows = rows.slice(rowWindow.first, rowWindow.last);
  $: reconcileSelection(rows);
  $: bounds = anchor && focus ? selectionBounds(anchor, focus) : null;
  $: selectedCount = bounds ? (bounds.lastRow - bounds.firstRow + 1) * (bounds.lastColumn - bounds.firstColumn + 1) : 0;
  $: record = recordMode && focus ? rows[focus.row] : null;
  $: selectedColumn = inspectorCell ? columns[inspectorCell.column] : undefined;
  $: selectedValue = inspectorCell && selectedColumn ? result?.rows[inspectorCell.originalIndex]?.[selectedColumn.name] : undefined;
  $: columnEditReasons = columns.map((column) => {
    if (column.generated) return 'This value is generated by the database and cannot be edited.';
    if (disabled || readOnly) return editDisabledReason || (disabled ? 'Wait for the current operation to finish before editing.' : 'This result is read-only.');
    if (!column.sourceName) return 'Computed expressions are read-only.';
    return editDisabledReason || (editableColumns.includes(column.name) ? '' : 'This source type cannot be safely edited.');
  });
  $: inspectorDisabledReason = inspectorCell ? columnEditReasons[inspectorCell.column] : '';
  $: inspectorDisabled = Boolean(inspectorDisabledReason);
  $: inspectorWidthBounds = cellDetailsWidthBounds(gridWidth);
  $: visibleInspectorWidth = clampPaneWidth(inspectorWidth, inspectorWidthBounds, DEFAULT_CELL_DETAILS_WIDTH);
  $: foreignTargets = foreignKeyTargets(selectedColumn, columns, schemaItems, editableColumns);
  $: changed = new Set(changedCells.map((cell) => cellKeyId(cell.rowIndex, cell.column)));
  $: defaultCells = new Set(changedCells.filter(cell => cell.useDefault).map(cell => cellKeyId(cell.rowIndex, cell.column)));
  $: if (result) syncInspectorDraft(result, inspectorCell, defaultCells);
  $: inspectorCanReset = Boolean(!disabled && inspectorCell && selectedColumn && (
    changed.has(cellKeyId(inspectorCell.originalIndex, selectedColumn.name)) || editorDraft.error
  ));
  $: contextColumn = contextCell ? columns[contextCell.column] : undefined;
  $: contextCanEdit = Boolean(contextColumn && contextCell && !columnEditReasons[contextCell.column]);
  $: cellActions = [
    { id: 'copy', label: 'Copy' },
    { id: 'reset', label: 'Reset to original', disabled: disabled || !contextCell || !contextColumn || (
      !changed.has(cellKeyId(contextCell.originalIndex, contextColumn.name)) && !retainedDrafts.has(draftKey(contextCell))
      && !(sameCell(contextCell, inspectorCell) && editorDraft.error)
    ) },
    { id: 'null', label: 'Set NULL', disabled: !contextCanEdit || contextColumn?.nullable === false },
    { id: 'default', label: 'Set DEFAULT', disabled: !contextCanEdit }
  ];
  $: totalWidth = 48 + columns.reduce((total, column) => total + columnWidth(column.name, widths), 0);

  onMount(() => {
    try { inspectorWidth = restoreCellDetailsWidth(localStorage.getItem(CELL_DETAILS_WIDTH_KEY)); }
    catch { /* Use the default width when preferences are unavailable. */ }
  });

  function persistInspectorWidth(width: number) {
    try { localStorage.setItem(CELL_DETAILS_WIDTH_KEY, String(width)); }
    catch { /* Keep the width for the current result. */ }
  }

  function nearBottom() {
    return scrollElement && scrollElement.scrollHeight - scrollElement.scrollTop - scrollElement.clientHeight <= ROW_HEIGHT * 2;
  }

  async function requestMore() {
    // Latch the current batch so scroll momentum cannot repeat a failed or
    // cancelled request. Appending rows or a fresh gesture allows another try.
    if (!canLoadMore || readOnly || disabled || requestingMore || requestedRows === rows.length || !nearBottom()) return;
    requestedRows = rows.length;
    requestingMore = true;
    try {
      await onLoadMore();
    } catch (error) {
      onMessage(`Load more failed: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      requestingMore = false;
    }
  }

  function handleScroll() {
    if (!scrollElement) return;
    const next = scrollElement.scrollTop;
    const movingDown = next > lastScrollPosition;
    lastScrollPosition = next;
    scrollTop = next;
    if (!nearBottom()) requestedRows = -1;
    else if (movingDown) void requestMore();
  }

  function handleWheel(event: WheelEvent) {
    if (event.deltaY <= 0 || Math.abs(event.deltaY) < Math.abs(event.deltaX)) return;
    const now = performance.now();
    if (!requestingMore && now - lastWheelAt > 250) requestedRows = -1;
    lastWheelAt = now;
    // Wheel input still arrives when the scrollbar is already at its limit.
    void requestMore();
  }

  function startTouch(event: TouchEvent) {
    const touch = event.touches.length === 1 ? event.touches[0] : undefined;
    touchPosition = touch ? { x: touch.clientX, y: touch.clientY } : null;
    if (touchPosition && !requestingMore) requestedRows = -1;
  }

  function moveTouch(event: TouchEvent) {
    const touch = event.touches.length === 1 ? event.touches[0] : undefined;
    if (touch && touchPosition) {
      const down = touchPosition.y - touch.clientY;
      if (down > 0 && down >= Math.abs(touch.clientX - touchPosition.x)) void requestMore();
    }
    touchPosition = touch ? { x: touch.clientX, y: touch.clientY } : null;
  }

  function reconcileSelection(nextRows: IndexedRow[]) {
    if (anchor && focus && previousRows.length) {
      const nextAnchor = positionAfterReorder(previousRows, nextRows, anchor);
      const nextFocus = positionAfterReorder(previousRows, nextRows, focus);
      if (nextAnchor && nextFocus) {
        anchor = nextAnchor;
        focus = nextFocus;
      } else {
        anchor = null;
        focus = null;
        if (view === 'record') view = 'grid';
      }
    }
    previousRows = nextRows;
  }

  function ensureRowVisible(row: number) {
    if (!scrollElement) return;
    const rowTop = HEADER_HEIGHT + row * ROW_HEIGHT;
    if (rowTop < scrollElement.scrollTop + HEADER_HEIGHT) scrollElement.scrollTop = rowTop - HEADER_HEIGHT;
    else if (rowTop + ROW_HEIGHT > scrollElement.scrollTop + scrollElement.clientHeight) scrollElement.scrollTop = rowTop + ROW_HEIGHT - scrollElement.clientHeight;
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
    if (inspectorCell && !extend) void inspectCell(row, column);
  }

  async function focusCell(position: CellPosition) {
    if (view === 'grid') ensureRowVisible(position.row);
    await tick();
    if (focus?.row === position.row && focus.column === position.column) {
      root?.querySelector<HTMLButtonElement>(`[data-cell="${position.row}:${position.column}"]`)?.focus();
    }
  }

  async function setView(next: ResultView, keyboardPosition?: CellPosition) {
    if (next === view || !resultTabs.some((tab) => tab.id === next)) return;
    const previous = view;
    const viewport = view === 'grid' ? scrollElement : recordScrollElement;
    if (view !== 'details' && viewport) viewScroll[view] = { top: viewport.scrollTop, left: viewport.scrollLeft };
    const position = keyboardPosition ?? focus ?? (next === 'record' ? { row: 0, column: 0 } : null);
    if (next !== 'details' && position && (focus?.row !== position.row || focus.column !== position.column)) selectCell(position.row, position.column);
    view = next;
    await tick();
    if (view !== next) return;
    if (next === 'grid' && scrollElement) {
      scrollElement.scrollTo(viewScroll.grid);
      if (position && previous === 'record') ensureRowVisible(position.row);
      scrollTop = scrollElement.scrollTop;
      // Restoring a viewport at the end of the grid is not a pagination gesture.
      lastScrollPosition = scrollTop;
    } else if (next === 'record') recordScrollElement?.scrollTo(viewScroll.record);
    if (keyboardPosition) await focusCell(keyboardPosition);
  }

  function chooseView(next: ResultView) {
    void setView(next);
    root.querySelector<HTMLButtonElement>(`[data-result-view="${next}"]`)?.focus({ preventScroll: true });
  }

  function viewKey(event: KeyboardEvent) {
    if (event.altKey || event.metaKey || event.ctrlKey || event.shiftKey) return;
    const current = resultTabs.findIndex((tab) => tab.id === view);
    let next: number;
    if (event.key === 'ArrowLeft') next = (current + resultTabs.length - 1) % resultTabs.length;
    else if (event.key === 'ArrowRight') next = (current + 1) % resultTabs.length;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = resultTabs.length - 1;
    else return;
    event.preventDefault();
    chooseView(resultTabs[next].id);
  }

  function moveRecord(offset: number) {
    if (!focus || !rows[focus.row + offset]) return;
    const position = { row: focus.row + offset, column: focus.column };
    selectCell(position.row, position.column);
    void focusCell(position);
  }

  async function inspectCell(row: number, column: number, focusPanel = false) {
    if (!result || !rows[row]) return;
    const originalIndex = rows[row].originalIndex;
    if (inspectorCell?.originalIndex !== originalIndex || inspectorCell.column !== column) {
      inspectorCell = { originalIndex, column };
      foreignSearch = '';
      syncInspectorDraft(result, inspectorCell, defaultCells);
    }
    if (focusPanel) {
      await tick();
      const editor = inspectorElement?.querySelector<HTMLElement>('input[type="search"]:not(:disabled)')
        ?? inspectorElement?.querySelector<HTMLElement>('textarea:not(:disabled)')
        ?? inspectorElement?.querySelector<HTMLElement>('input:not(:disabled)');
      (editor ?? inspectorElement?.querySelector<HTMLElement>('.close-inspector'))?.focus();
    }
  }

  async function closeInspector() {
    const inspectedRow = inspectorCell ? rows.findIndex((entry) => entry.originalIndex === inspectorCell?.originalIndex) : -1;
    const destination = focus ?? (inspectorCell && inspectedRow >= 0 ? { row: inspectedRow, column: inspectorCell.column } : null);
    inspectorCell = null;
    if (!destination) return;
    focus = destination;
    anchor ??= destination;
    await focusCell(destination);
  }

  function inspectorKey(event: KeyboardEvent) {
    if (event.key === 'Escape' && inspectorElement?.contains(event.target as Node)) {
      event.preventDefault();
      void closeInspector();
    }
  }

  function cellKeyId(rowIndex: number, column: string) {
    return `${rowIndex}\u001f${column}`;
  }

  function draftKey(cell: InspectedCell) {
    return `${cell.originalIndex}:${cell.column}`;
  }

  function sameCell(cell: InspectedCell, other: InspectedCell | null) {
    return other?.originalIndex === cell.originalIndex && other.column === cell.column;
  }

  function syncInspectorDraft(data: QueryResult, cell: InspectedCell | null, defaults: Set<string>) {
    const column = cell && data.columns[cell.column];
    const row = cell && data.rows[cell.originalIndex];
    const value = column && row ? row[column.name] : undefined;
    const useDefault = Boolean(cell && column && defaults.has(cellKeyId(cell.originalIndex, column.name)));
    if (inspectorSource?.cell === cell && inspectorSource.columnName === column?.name
      && Object.is(inspectorSource.value, value) && inspectorSource.useDefault === useDefault) return;

    if (inspectorSource && inspectorSource.cell !== cell && editorDraft.error) {
      const { cell: previous, ...source } = inspectorSource;
      retainedDrafts.set(draftKey(previous), { ...source, draft: editorDraft });
    }
    inspectorSource = cell && column && row ? { cell, columnName: column.name, value, useDefault } : null;
    if (!inspectorSource || !cell || !column) return;

    const key = draftKey(cell);
    const retained = retainedDrafts.get(key);
    retainedDrafts.delete(key);
    if (retained?.columnName === column.name && Object.is(retained.value, value) && retained.useDefault === useDefault) editorDraft = retained.draft;
    else resetDraft(column, value, data.jsonValuesAsText, useDefault);
  }

  function resetDraft(column: ColumnMeta | undefined, value: unknown, jsonValuesAsText?: boolean, useDefault = false) {
    if (!column) return;
    editorDraft = {
      text: value == null ? (cellKind(column) === 'array' ? '{}' : '') : prettyCell(value, column, jsonValuesAsText),
      isNull: value == null && !useDefault,
      useDefault,
      error: '',
      foreignRow: null
    };
  }

  function editInspectorValue(text: string) {
    if (inspectorDisabled || !inspectorCell || !selectedColumn || !result) return;
    const cell = inspectorCell;
    const column = selectedColumn;
    const previous = result.rows[cell.originalIndex];
    if (!previous) return;
    editorDraft = { text, isNull: false, useDefault: false, error: '', foreignRow: null };
    try {
      const value = parseCellDraft(column, text, false);
      if (!defaultCells.has(cellKeyId(cell.originalIndex, column.name))
        && sameCellValue(value, previous[column.name], column, true, result.jsonValuesAsText)) return;
      const row = onEditMany(cell.originalIndex, { [column.name]: { value } });
      if (row) inspectorSource = { cell, columnName: column.name, value: row[column.name], useDefault: false };
    } catch (error) {
      editorDraft.error = error instanceof Error ? error.message : String(error);
    }
  }

  async function revealValueEditor() {
    editorDraft = { ...editorDraft, isNull: false, useDefault: false };
    await tick();
    inspectorElement?.querySelector<HTMLElement>('textarea:not(:disabled), input[type="radio"]:not(:disabled)')?.focus();
  }

  function chooseForeignKey(target: ForeignKeyTarget, row: Record<string, string | null>) {
    if (inspectorDisabled || !selectedColumn || !inspectorCell || !result) return;
    const cell = inspectorCell;
    try {
      const values = foreignKeyDraft(target, row);
      const edits = Object.fromEntries(target.mappings.map(({ column }) =>
        [column.name, { value: parseCellDraft(column, values[column.name], false) }]));
      const staged = onEditMany(cell.originalIndex, edits);
      if (!staged) return;
      const value = staged[selectedColumn.name];
      inspectorSource = { cell, columnName: selectedColumn.name, value, useDefault: false };
      editorDraft = { text: values[selectedColumn.name], isNull: false, useDefault: false, error: '', foreignRow: row };
    } catch (error) { editorDraft.error = error instanceof Error ? error.message : String(error); }
  }

  export function captureExport() {
    if (!result?.rows.length) return null;
    const selectionStart = recordMode ? focus : anchor;
    return {
      loaded: sort ? { ...result, rows: rows.map(entry => entry.row) } : result,
      selection: selectionStart && focus ? selectedResult(result, rows, selectionStart, focus) : null
    };
  }

  function changeCell(cell: InspectedCell, action: 'reset' | 'null' | 'default') {
    const column = columns[cell.column];
    const current = result?.rows[cell.originalIndex];
    if (!column || !current || disabled || (action !== 'reset' && columnEditReasons[cell.column])) return;
    if (action === 'null' && column.nullable === false) return;
    const key = cellKeyId(cell.originalIndex, column.name);
    const edit = action === 'null' ? { value: null } : { action };
    const row = action === 'reset' && !changed.has(key) ? current : onEditMany(cell.originalIndex, { [column.name]: edit });
    if (!row) return;
    retainedDrafts.delete(draftKey(cell));
    if (inspectorCell && sameCell(cell, inspectorCell)) {
      const value = row[column.name];
      const useDefault = action === 'default';
      inspectorSource = { cell: inspectorCell, columnName: column.name, value, useDefault };
      resetDraft(column, value, jsonValuesAsText, useDefault);
    }
  }

  async function openCellMenu(event: MouseEvent | KeyboardEvent, row: number, column: number) {
    const entry = rows[row];
    if (!entry) return;
    event.preventDefault();
    event.stopPropagation();
    const button = event.currentTarget as HTMLButtonElement;
    const bounds = button.getBoundingClientRect();
    const pointer = event instanceof MouseEvent && (event.clientX || event.clientY);
    selectCell(row, column);
    button.focus({ preventScroll: true });
    contextCell = { originalIndex: entry.originalIndex, column };
    await tick();
    await contextMenu.openAt(pointer ? event.clientX : bounds.left, pointer ? event.clientY : bounds.bottom);
  }

  async function chooseCellAction(action: string) {
    const cell = contextCell;
    const column = cell && columns[cell.column];
    const row = cell && result?.rows[cell.originalIndex];
    if (!cell || !column || !row) return;
    if (action === 'copy') {
      const value = defaultCells.has(cellKeyId(cell.originalIndex, column.name)) ? 'DEFAULT' : cellText(row[column.name], column, jsonValuesAsText);
      try {
        await copyText(value, 'Copy result selection', '1 cell');
        onMessage('Cell copied.');
      } catch (error) {
        onMessage(`Copy failed: ${error instanceof Error ? error.message : String(error)}`);
      }
    } else if (action === 'reset' || action === 'null' || action === 'default') changeCell(cell, action);
  }

  async function copySelection() {
    if (!anchor || !focus) return;
    const start = recordMode ? focus : anchor;
    const end = focus;
    const count = recordMode ? 1 : selectedCount;
    try {
      await copyText(() => selectionText(rows, columns, start, end, jsonValuesAsText),
        'Copy result selection', `${count} ${count === 1 ? 'cell' : 'cells'}`);
      onMessage(`${count === 1 ? 'Cell' : `${count} cells`} copied.`);
    } catch (error) {
      onMessage(`Copy failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  function pasteCell(row: number, column: number, text: string) {
    const entry = rows[row];
    const target = columns[column];
    if (!entry || !target) return;
    const reason = columnEditReasons[column];
    if (reason) { onMessage(reason); return; }
    selectCell(row, column);
    if (foreignKeyTargets(target, columns, schemaItems, editableColumns).length) {
      void inspectCell(row, column, true);
      foreignSearch = text.trim().slice(0, 512);
      return;
    }
    try {
      const value = parseCellDraft(target, text, false);
      if (!defaultCells.has(cellKeyId(entry.originalIndex, target.name))
        && sameCellValue(value, entry.row[target.name], target, true, jsonValuesAsText)) return;
      const staged = onEditMany(entry.originalIndex, { [target.name]: { value } });
      if (staged) {
        retainedDrafts.delete(draftKey({ originalIndex: entry.originalIndex, column }));
        onMessage('Cell change staged.');
      }
    } catch (error) {
      onMessage(`Paste failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  async function pasteFromClipboard(row: number, column: number, cell: HTMLButtonElement) {
    const reason = columnEditReasons[column];
    if (reason) { onMessage(reason); return; }
    const request = ++pasteRequest;
    const source = result;
    const originalIndex = rows[row]?.originalIndex;
    let text: string;
    try {
      text = await readClipboardText();
    } catch (error) {
      if (request === pasteRequest && cell === document.activeElement) onMessage(`Could not read the clipboard: ${error instanceof Error ? error.message : String(error)}`);
      return;
    }
    if (request === pasteRequest && source === result && cell === document.activeElement && rows[row]?.originalIndex === originalIndex) {
      pasteCell(row, column, text);
    }
  }

  function handleCellPaste(event: ClipboardEvent, row: number, column: number) {
    event.preventDefault();
    event.stopPropagation();
    pasteRequest++;
    if (event.clipboardData?.types.includes('text/plain')) pasteCell(row, column, event.clipboardData.getData('text/plain'));
    else onMessage('Clipboard does not contain text.');
  }

  async function cellKey(event: KeyboardEvent, row: number, column: number) {
    if (event.defaultPrevented || event.isComposing) return;
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      await openCellMenu(event, row, column);
      return;
    }
    if (event.key === 'Tab' && !event.shiftKey && !event.metaKey && !event.ctrlKey && !event.altKey) {
      event.preventDefault();
      if (!event.repeat) await setView(recordMode ? 'grid' : 'record', { row, column });
      return;
    }
    if ((event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === 'v') {
      event.preventDefault();
      event.stopPropagation();
      if (!event.repeat) await pasteFromClipboard(row, column, event.currentTarget as HTMLButtonElement);
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'c') {
      event.preventDefault();
      await copySelection();
      return;
    }
    if (!recordMode && (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'a') {
      event.preventDefault();
      anchor = { row: 0, column: 0 };
      focus = { row: rows.length - 1, column: columns.length - 1 };
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      selectCell(row, column, !recordMode && event.shiftKey);
      await inspectCell(row, column, true);
      return;
    }
    if (event.key === 'Escape') {
      if (recordMode) {
        event.preventDefault();
        await setView('grid', { row, column });
        return;
      }
      inspectorCell = null;
      anchor = { row, column };
      focus = { row, column };
      return;
    }
    let nextRow = row;
    let nextColumn = column;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      const offset = event.key === 'ArrowDown' ? 1 : -1;
      if (recordMode) nextColumn += offset;
      else nextRow += offset;
      if (!recordMode && offset > 0 && row === rows.length - 1) {
        if (!event.repeat && !requestingMore) requestedRows = -1;
        void requestMore();
      }
    }
    else if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
      const offset = event.key === 'ArrowRight' ? 1 : -1;
      if (recordMode) nextRow += offset;
      else nextColumn += offset;
    }
    else if (event.key === 'Home') { nextColumn = 0; if (event.metaKey || event.ctrlKey) nextRow = 0; }
    else if (event.key === 'End') { nextColumn = columns.length - 1; if (event.metaKey || event.ctrlKey) nextRow = rows.length - 1; }
    else return;
    event.preventDefault();
    nextRow = Math.max(0, Math.min(rows.length - 1, nextRow));
    nextColumn = Math.max(0, Math.min(columns.length - 1, nextColumn));
    selectCell(nextRow, nextColumn, !recordMode && event.shiftKey);
    await focusCell({ row: nextRow, column: nextColumn });
  }

  function isSelected(row: number, column: number, selection: ReturnType<typeof selectionBounds> | null) {
    return selection && row >= selection.firstRow && row <= selection.lastRow && column >= selection.firstColumn && column <= selection.lastColumn;
  }
</script>

<svelte:window on:pointermove={moveResize} on:pointerup={() => (resize = null)} on:pointercancel={() => (resize = null)} on:keydown={inspectorKey} />

{#snippet columnLabel(column: ColumnMeta, index: number)}
  <span class="column-label">
    {#if headers[index].primaryKey}<span class="column-key" title="Primary key"><Icon name="key" size={12} /></span>{/if}
    {#if headers[index].foreignKey}<span class="column-link" title="Foreign key"><Icon name="link" size={12} /></span>{/if}
    <span class="column-name">{column.name}</span>
    <span class="column-type">{shortType(column.typeName)}</span>
  </span>
{/snippet}

{#snippet cellButton(entry: IndexedRow, row: number, column: ColumnMeta, index: number, tabbable: boolean)}
  {@const value = entry.row[column.name]}
  {@const useDefault = defaultCells.has(cellKeyId(entry.originalIndex, column.name))}
  {@const displayType = useDefault ? 'other' : cellDisplayType(column, value)}
  {@const empty = !useDefault && value === '' && cellKind(column) !== 'json'}
  {@const label = useDefault ? 'DEFAULT' : empty ? 'empty string' : cellText(value, column, jsonValuesAsText)}
  <button
    class={`cell value-${displayType}`}
    class:false-value={displayType === 'boolean' && (value === false || value === 'false')}
    class:null-value={displayType === 'null'}
    class:default-value={useDefault}
    class:empty-value={empty}
    data-cell={`${row}:${index}`}
    tabindex={tabbable ? 0 : -1}
    aria-label={`Row ${entry.originalIndex + 1}, ${column.name} (${column.typeName}): ${label.slice(0, 200)}`}
    aria-pressed={recordMode ? focus?.column === index : Boolean(isSelected(row, index, bounds))}
    aria-haspopup="menu"
    title={`${column.typeName} · Tab for ${recordMode ? 'Grid' : 'Record'} view · Double-click or Enter for details${columnEditReasons[index] ? '' : ' · ⌘/Ctrl+V to paste'}`}
    on:click={(event) => {
      selectCell(row, index, !recordMode && event.shiftKey);
      event.currentTarget.focus({ preventScroll: true });
    }}
    on:focus={(event) => {
      if (!focus || (!recordMode && (focus.row < rowWindow.first || focus.row >= rowWindow.last) && event.currentTarget.matches(':focus-visible'))) selectCell(row, index);
    }}
    on:dblclick={() => inspectCell(row, index, true)}
    on:contextmenu={(event) => openCellMenu(event, row, index)}
    on:keydown={(event) => cellKey(event, row, index)}
    on:paste={(event) => handleCellPaste(event, row, index)}
  >{#if recordMode}<span class="record-value">{label}</span>{:else}{label}{/if}</button>
{/snippet}

<div class="results-grid" class:resizing={Boolean(resize) || resizingInspector} bind:this={root} bind:clientWidth={gridWidth}>
  <ActionMenu bind:this={contextMenu} ariaLabel="Cell actions" contextOnly items={cellActions} onSelect={chooseCellAction} />
  <div class="grid-tools">
    <div class="grid-navigation">
      {#if resultTabs.length > 1}
        <div class="result-views" role="tablist" aria-label="Result view">
          {#each resultTabs as tab (tab.id)}
            <button
              type="button" role="tab" id={`result-tab-${tab.id}`} data-result-view={tab.id} aria-controls="result-view-panel"
              aria-selected={view === tab.id} tabindex={view === tab.id ? 0 : -1} title={tab.title}
              on:click={() => chooseView(tab.id)} on:keydown={viewKey}
            >{tab.label}</button>
          {/each}
        </div>
      {:else}
        <strong class="results-heading">Results</strong>
      {/if}
      {#if record && focus}
        <div class="record-navigation" role="group" aria-label="Browse records">
          <button class="previous-record" aria-label="Previous record" title="Previous record (←)" disabled={focus.row === 0} on:click={() => moveRecord(-1)}><Icon name="chevron" size={11} /></button>
          <span class="record-position" role="status" title={`${focus.row + 1} of ${rows.length} loaded records`}>{focus.row + 1} / {rows.length}</span>
          <button aria-label="Next record" title="Next record (→)" disabled={focus.row === rows.length - 1} on:click={() => moveRecord(1)}><Icon name="chevron" size={11} /></button>
        </div>
      {:else if view === 'grid'}
        {#if selectedCount}<span class="selection-label" role="status">{selectedCount} {selectedCount === 1 ? 'cell' : 'cells'} selected</span>{/if}
        {#if sort}<button class="reset-sort" title={`Reset the sort of ${rows.length} loaded rows`} on:click={() => { sort = null; anchor = null; focus = null; }}>Reset sort</button>{/if}
      {/if}
      {@render status?.()}
    </div>
    {@render actions?.()}
  </div>
  {@render notices?.()}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex (Focus the Details panel so its contents can be scrolled by keyboard.) -->
  <div
    id="result-view-panel" class="result-panel" class:details-scroll={view === 'details'}
    role={resultTabs.length > 1 ? 'tabpanel' : undefined} aria-labelledby={resultTabs.length > 1 ? `result-tab-${view}` : undefined}
    tabindex={view === 'details' ? 0 : undefined}
  >
    {#if view === 'details'}
      {@render details?.()}
    {:else if rows.length}
      <div class="grid-content" style={`--inspector-width: ${visibleInspectorWidth}px`}>
        {#if record && focus}
          {@const row = focus.row}
          <div class="grid-scroll" role="region" aria-label="Record view" bind:this={recordScrollElement}>
            <table class="record-table" aria-label={`Record view, result row ${record.originalIndex + 1}`}>
              <colgroup><col class="record-field-column" /><col /></colgroup>
              <thead><tr><th scope="col">Field</th><th scope="col">Value</th></tr></thead>
              <tbody>
                {#each columns as column, index (index)}
                  <tr>
                    <th scope="row" title={headers[index].tooltip}>{@render columnLabel(column, index)}</th>
                    <td class:selected={focus.column === index} class:changed={changed.has(`${record.originalIndex}\u001f${column.name}`)}>
                      {@render cellButton(record, row, column, index, focus.column === index)}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else}
          <div
            class="grid-scroll"
            role="region"
            aria-label="Query result rows"
            aria-busy={requestingMore}
            bind:this={scrollElement}
            bind:clientHeight={viewportHeight}
            on:scroll={handleScroll}
            on:wheel|passive={handleWheel}
            on:touchstart|passive={startTouch}
            on:touchmove|passive={moveTouch}
            on:touchend={() => (touchPosition = null)}
            on:touchcancel={() => (touchPosition = null)}
          >
            <table style:width={`${totalWidth}px`} aria-rowcount={rows.length + 1} aria-label="Query result; column sorting applies only to loaded rows">
              <colgroup><col style:width="48px" />{#each columns as column}<col style:width={`${columnWidth(column.name, widths)}px`} />{/each}</colgroup>
              <thead>
                <tr>
                  <th class="row-number" scope="col" title="Original result row number">#</th>
                  {#each columns as column, columnIndex (columnIndex)}
                    {@const sortTooltip = `${headers[columnIndex].tooltip}\nSort loaded rows: ascending → descending → original order`}
                    <th scope="col" aria-sort={sort?.column === column.name ? sort.direction === 'asc' ? 'ascending' : 'descending' : 'none'}>
                      <button class="column-sort" on:click={() => toggleSort(column.name)} title={sortTooltip} aria-label={sortTooltip}>
                        {@render columnLabel(column, columnIndex)}
                        <span class="column-direction" aria-hidden="true">{sort?.column === column.name ? sort.direction === 'asc' ? '↑' : '↓' : '↕'}</span>
                      </button>
                      <button class="column-resize" aria-label={`Resize ${column.name} column, ${columnWidth(column.name, widths)} pixels`} title="Drag to resize. Arrow keys adjust width; Shift for larger steps." on:pointerdown={(event) => startResize(event, column.name)} on:keydown={(event) => resizeKey(event, column.name)}><span></span></button>
                    </th>
                  {/each}
                </tr>
              </thead>
              <tbody>
                {#if rowWindow.top}<tr class="spacer-row" aria-hidden="true"><td colspan={columns.length + 1} style:height={`${rowWindow.top}px`}></td></tr>{/if}
                {#each visibleRows as entry, visibleIndex (entry.originalIndex)}
                  {@const rowIndex = rowWindow.first + visibleIndex}
                  <tr aria-rowindex={rowIndex + 2}>
                    <th scope="row" class="row-number">{entry.originalIndex + 1}</th>
                    {#each columns as column, columnIndex (columnIndex)}
                      {@const tabbable = focus && focus.row >= rowWindow.first && focus.row < rowWindow.last
                        ? focus.row === rowIndex && focus.column === columnIndex : visibleIndex === 0 && columnIndex === 0}
                      <td class:selected={isSelected(rowIndex, columnIndex, bounds)} class:changed={changed.has(`${entry.originalIndex}\u001f${column.name}`)}>
                        {@render cellButton(entry, rowIndex, column, columnIndex, tabbable)}
                      </td>
                    {/each}
                  </tr>
                {/each}
                {#if rowWindow.bottom}<tr class="spacer-row" aria-hidden="true"><td colspan={columns.length + 1} style:height={`${rowWindow.bottom}px`}></td></tr>{/if}
              </tbody>
            </table>
          </div>
        {/if}
        {#if inspectorCell && selectedColumn}
          <PaneResizer bind:width={inspectorWidth} bounds={inspectorWidthBounds} defaultWidth={DEFAULT_CELL_DETAILS_WIDTH} label="Resize cell details" controls="cell-inspector" side="right" bind:resizing={resizingInspector} onResizeEnd={persistInspectorWidth} />
          <aside id="cell-inspector" class="cell-inspector" class:foreign-key={foreignTargets.length > 0} aria-label="Cell details" bind:this={inspectorElement}>
            <div class="inspector-title"><strong title={selectedColumn.name}>{selectedColumn.name}</strong><button class="close-inspector" aria-label="Close cell details" on:click={closeInspector}><Icon name="close" size={14} /></button></div>
            <p class="cell-meta">Row {inspectorCell.originalIndex + 1} · {selectedColumn.typeName}{selectedColumn.generated ? ' · generated' : ''}{selectedColumn.nullable === false ? ' · not nullable' : ''}</p>
            <div class="inspector-editor" class:foreign-key-editor={foreignTargets.length > 0}>
              <div class="inspector-body">
                <CellValueEditor column={selectedColumn} draft={editorDraft.text} isNull={editorDraft.isNull} useDefault={editorDraft.useDefault} pickerOnly={foreignTargets.length > 0} disabled={inspectorDisabled} error={editorDraft.error} onChange={editInspectorValue} onEditValue={revealValueEditor} />
                {#if foreignTargets.length}
                  {#key `${resultContext}:${inspectorCell.originalIndex}:${selectedColumn.name}`}
                    <ForeignKeyPicker targets={foreignTargets} disabled={inspectorDisabled} bind:search={foreignSearch} selectedRow={editorDraft.foreignRow} onLookup={onForeignKeyLookup} onCancel={onCancelLookup} onPick={chooseForeignKey} onTargetChange={() => { editorDraft.foreignRow = null; }} />
                  {/key}
                {/if}
                {#if inspectorDisabled}
                  {#if selectedValue === '' && cellKind(selectedColumn) !== 'json'}<p class="edit-hint">Empty string (0 characters)</p>{/if}
                  <p class="edit-hint">{inspectorDisabledReason}</p>
                {/if}
                {#if editorDraft.error}<p class="draft-error" role="alert">{editorDraft.error} The grid shows the last valid value.</p>{/if}
              </div>
              <div class="inspector-actions">
                <button type="button" disabled={!inspectorCanReset} title="Restore the original value" on:click={() => inspectorCell && changeCell(inspectorCell, 'reset')}>Reset</button>
                <button type="button" disabled={inspectorDisabled || selectedColumn.nullable === false} title={selectedColumn.nullable === false ? 'This column does not allow NULL.' : 'Use SQL NULL'} on:click={() => inspectorCell && changeCell(inspectorCell, 'null')}>Set NULL</button>
              </div>
            </div>
          </aside>
        {/if}
      </div>
      {#if result?.hasMore}
        <footer class="results-foot">
          <span>{rows.length} loaded · more available</span>
          {#if loadMoreBlockedReason}<span>{loadMoreBlockedReason}</span>
          {:else if canLoadMore}<span>{recordMode ? 'Switch to Grid to load more' : 'Scroll down to load more'}</span>{/if}
        </footer>
      {/if}
    {:else}
      {@render children?.()}
    {/if}
  </div>
</div>

<style>
  .results-grid, .result-panel { flex: 1; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; }
  .results-grid { color: var(--ink); background: var(--surface-1); }
  .result-panel { container-type: inline-size; }
  .grid-tools { display: flex; align-items: center; justify-content: space-between; gap: 6px; min-height: 27px; padding: 0 8px; flex-shrink: 0; border-bottom: 1px solid var(--line); background: var(--surface-0); }
  .grid-navigation { display: flex; align-items: center; align-self: stretch; gap: 8px; min-width: 0; overflow-x: auto; scrollbar-width: none; }
  .result-views { display: flex; align-self: stretch; flex-shrink: 0; }
  .results-heading { padding: 0 3px; font-size: 11px; font-weight: 500; }
  .selection-label, .record-position { color: var(--ink-muted); font: 10px/14px var(--font-mono); white-space: nowrap; }
  .record-navigation { display: flex; align-items: center; gap: 2px; flex-shrink: 0; }
  .previous-record :global(.icon) { transform: rotate(180deg); }
  button { display: inline-flex; align-items: center; justify-content: center; gap: 5px; min-height: 25px; padding: 3px 8px; border: 1px solid var(--line-strong); border-radius: 4px; background: var(--surface-2); color: var(--ink); box-shadow: none; font-size: 11px; }
  button:hover:not(:disabled) { background: var(--surface-2); border-color: var(--line-strong); color: var(--ink); }
  .result-views button, .record-navigation button, .reset-sort { min-height: 20px; padding: 0 6px; border: 0; border-radius: 0; background: transparent; color: var(--ink-muted); white-space: nowrap; }
  .result-views button[aria-selected='true'] { color: var(--accent); box-shadow: inset 0 -2px var(--accent); }
  .record-navigation button { width: 20px; padding: 0; }
  .reset-sort { padding: 0 3px; font-size: 10px; }
  button:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
  .result-views button:focus-visible { outline-width: 1px; }
  button:disabled { opacity: .45; }
  .grid-content { position: relative; display: flex; flex: 1; min-height: 0; min-width: 0; }
  .grid-content > :global(.pane-resizer) { flex: 0 0 7px; }
  .grid-scroll { flex: 1; min-width: 0; min-height: 0; overflow: auto; overscroll-behavior: contain; }
  .details-scroll { flex: 1; min-height: 0; overflow: auto; overscroll-behavior: contain; }
  table { table-layout: fixed; border-collapse: separate; border-spacing: 0; font-size: 12px; }
  thead { position: sticky; top: 0; z-index: 2; }
  th, td { padding: 0; max-width: none; min-width: 0; text-align: left; border: 0; border-right: 1px solid var(--line); border-bottom: 1px solid var(--line); white-space: nowrap; overflow: hidden; }
  th { position: relative; top: auto; z-index: auto; background: var(--surface-0); color: var(--ink); }
  .row-number { width: 48px; padding: 0 9px; text-align: right; font: 11px var(--font-mono); color: var(--ink-faint); }
  tbody .row-number { background: var(--surface-0); }
  .column-sort { position: relative; display: grid; grid-template-columns: minmax(0, 1fr) 14px; width: 100%; height: 27px; min-height: 27px; padding: 0 16px 0 10px; border: none; border-radius: 0; text-align: left; background: transparent; }
  .column-sort:hover:not(:disabled) { background: var(--surface-2); }
  .column-label { display: flex; align-items: center; gap: 5px; min-width: 0; overflow: hidden; }
  .column-key, .column-link { display: inline-flex; flex-shrink: 0; }
  .column-key { color: var(--accent); }
  .column-link { color: var(--accent); }
  .column-name { overflow: hidden; text-overflow: ellipsis; font: 500 12px/16px var(--font-mono); }
  .column-type { flex-shrink: 0; color: var(--ink-muted); font: 11px/16px var(--font-mono); }
  .column-direction { color: var(--ink-muted); }
  .column-resize { position: absolute; right: 0; top: 0; width: 9px; height: 100%; min-height: 0; padding: 0; border: 0; border-radius: 0; background: transparent; cursor: col-resize; touch-action: none; }
  .column-resize span { width: 1px; height: 16px; background: var(--line-strong); }
  .column-resize:hover:not(:disabled) { background: var(--accent); }
  td { background: var(--surface-1); }
  td.selected { background: var(--accent-soft); }
  td.selected .cell { outline: 1px solid var(--accent); outline-offset: -1px; }
  td.selected .cell:hover:not(:disabled) { background: var(--accent-soft); }
  .cell { display: block; width: 100%; height: 27px; min-height: 27px; padding: 5px 10px; overflow: hidden; text-overflow: ellipsis; border: 0; border-radius: 0; background: transparent; text-align: left; font-family: var(--font-mono); font-size: 12px; font-weight: 400; line-height: 16px; white-space: nowrap; user-select: none; }
  td.changed .cell { box-shadow: inset 3px 0 var(--warn); }
  .spacer-row td { padding: 0; border: 0; background: var(--surface-1); }
  .cell:hover:not(:disabled) { background: var(--surface-2); color: var(--value-color, var(--ink)); }
  .cell.value-number { --value-color: var(--syn-number); color: var(--value-color); text-align: right; font-variant-numeric: tabular-nums; }
  .cell.value-string { --value-color: var(--syn-string); color: var(--value-color); }
  .cell.value-date { --value-color: var(--syn-function); color: var(--value-color); font-variant-numeric: tabular-nums; }
  .cell.value-boolean { --value-color: var(--syn-literal); color: var(--value-color); font-weight: 600; }
  .cell.false-value { --value-color: var(--syn-literal); }
  .cell.value-json { --value-color: var(--ink-muted); color: var(--value-color); }
  .cell.value-array { --value-color: var(--ink-muted); color: var(--value-color); }
  .cell:active:not(:disabled) { transform: none; }
  .cell.null-value, .cell.empty-value { --value-color: var(--ink-faint); font-style: italic; color: var(--value-color); }
  .cell.default-value { color: var(--warn); font-style: italic; }
  .cell.empty-value { font-size: 11px; }
  .record-table { width: 100%; }
  .record-field-column { width: 32%; }
  .record-table th { padding: 6px 10px; }
  .record-table thead th { font: 500 11px/16px var(--font-mono); }
  .record-table tbody th { background: var(--surface-1); }
  .record-table .cell { height: auto; min-height: 30px; text-align: left; scroll-margin-top: 28px; }
  .record-value { display: -webkit-box; line-clamp: 3; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; white-space: pre-wrap; overflow-wrap: anywhere; }
  .resizing, .resizing .cell { cursor: col-resize; user-select: none; }
  .cell-inspector { display: flex; flex: 0 0 var(--inspector-width); flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; padding: 12px; background: var(--surface-2); }
  .inspector-editor { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .inspector-body { flex: 1; min-height: 0; overflow: auto; overscroll-behavior: contain; }
  .inspector-title, .cell-meta { flex-shrink: 0; }
  .inspector-title { display: flex; align-items: center; gap: 8px; }
  .inspector-title strong { flex: 1; overflow: hidden; text-overflow: ellipsis; font-size: 13px; }
  .close-inspector { min-height: 24px; width: 24px; padding: 3px; background: transparent; border: none; }
  .cell-meta { margin: 3px 0 13px; font-size: 11px; color: var(--ink-muted); }
  .edit-hint { margin: 9px 0 11px; font-size: 11px; color: var(--ink-muted); line-height: 1.5; }
  .draft-error { color: var(--danger); font-size: 11px; line-height: 1.5; margin: 8px 0; }
  .inspector-actions { display: flex; flex-wrap: wrap; flex-shrink: 0; gap: 6px; padding-top: 8px; }
  .inspector-actions button { font-size: 11px; padding: 5px 8px; }
  .foreign-key-editor .inspector-body { display: flex; flex-direction: column; }
  .cell-inspector.foreign-key .cell-meta { margin-bottom: 6px; }
  @container (max-width: 640px) {
    .cell-inspector, .grid-content > :global(.pane-resizer) { position: absolute; z-index: 3; top: 0; bottom: 0; }
    .cell-inspector { right: 0; width: var(--inspector-width); }
    .grid-content > :global(.pane-resizer) { right: var(--inspector-width); width: 7px; }
  }
</style>
