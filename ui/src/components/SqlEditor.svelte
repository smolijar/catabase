<script lang="ts">
  import EditorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';
  import * as monaco from 'monaco-editor/esm/vs/editor/editor.api';
  import 'monaco-editor/esm/vs/editor/contrib/snippet/browser/snippetController2';
  import 'monaco-editor/esm/vs/editor/contrib/suggest/browser/suggestController';
  import 'monaco-editor/esm/vs/editor/contrib/inlineCompletions/browser/inlineCompletions.contribution';
  // editor.api omits the standard word movement, selection and deletion commands.
  import 'monaco-editor/esm/vs/editor/contrib/wordOperations/browser/wordOperations';
  import 'monaco-editor/esm/vs/basic-languages/sql/sql.contribution';
  import { onDestroy, onMount } from 'svelte';
  import { sqlCompletionContext, sqlJoinSuggestions } from '../lib/sqlCompletion';
  import type { SqlCompletionContext } from '../lib/sqlCompletion';
  import { createInlineRequester, inlineEdit } from '../lib/supermavenInline';
  import type { SupermavenCompletion, SupermavenRequest, ExecutionTarget, SchemaItem, StatementRange } from '../lib/types';

  export let value = '';
  export let readOnly = false;
  export let supermavenEnabled = false;
  export let schemaItems: SchemaItem[] = [];
  export let viewState: monaco.editor.ICodeEditorViewState | null = null;
  export let tabId: string;
  export let supermavenComplete: ((request: SupermavenRequest) => Promise<SupermavenCompletion | null>) | undefined = undefined;
  export let supermavenCancel: ((requestId: string) => Promise<void>) | undefined = undefined;

  const SQL_KEYWORDS = [
    'SELECT',
    'FROM',
    'WHERE',
    'JOIN',
    'LEFT JOIN',
    'RIGHT JOIN',
    'FULL JOIN',
    'INNER JOIN',
    'ON',
    'GROUP BY',
    'ORDER BY',
    'HAVING',
    'LIMIT',
    'OFFSET',
    'INSERT INTO',
    'VALUES',
    'UPDATE',
    'SET',
    'DELETE FROM',
    'CREATE TABLE',
    'ALTER TABLE',
    'DROP TABLE',
    'WITH',
    'AS',
    'RETURNING',
    'DISTINCT',
    'UNION',
    'CASE',
    'WHEN',
    'THEN',
    'ELSE',
    'END',
    'AND',
    'OR',
    'NOT',
    'NULL',
    'IS NULL',
    'IS NOT NULL',
    'LIKE',
    'ILIKE',
    'IN',
    'BETWEEN',
    'EXISTS',
    'TRUE',
    'FALSE'
  ];

  const SQL_SNIPPETS = [
    { label: 'select statement', insertText: 'select ${1:*}\nfrom ${2:table}\nlimit ${3:100};' },
    { label: 'insert row', insertText: 'insert into ${1:table} (${2:columns})\nvalues (${3:values});' },
    { label: 'update rows', insertText: 'update ${1:table}\nset ${2:column} = ${3:value}\nwhere ${4:condition};' },
    { label: 'delete rows', insertText: 'delete from ${1:table}\nwhere ${2:condition};' },
    { label: 'cte query', insertText: 'with ${1:name} as (\n  ${2:select * from table}\n)\nselect *\nfrom ${1:name};' }
  ];

  let host: HTMLDivElement;
  let editor: monaco.editor.IStandaloneCodeEditor | undefined;
  let subscription: monaco.IDisposable | undefined;
  let completionProvider: monaco.IDisposable | undefined;
  let inlineProvider: monaco.IDisposable | undefined;
  let highlights: monaco.editor.IEditorDecorationsCollection | undefined;
  let keydownHandler: ((event: KeyboardEvent) => void) | undefined;
  const ownerTabId = tabId;
  const inlineRequester = createInlineRequester(
    (request) => supermavenComplete?.(request) ?? Promise.resolve(null),
    (requestId) => supermavenCancel?.(requestId) ?? Promise.resolve()
  );

  onMount(() => {
    (window as unknown as { MonacoEnvironment: unknown }).MonacoEnvironment = {
      getWorker: () => new EditorWorker()
    };

    monaco.editor.defineTheme('catabase-dark', {
      base: 'vs-dark',
      inherit: true,
      rules: [
        { token: 'keyword', foreground: '9CDCFE' },
        { token: 'string', foreground: 'CE9178' },
        { token: 'comment', foreground: '6A9955' }
      ],
      colors: {
        'editor.background': '#0b1020',
        'editorLineNumber.foreground': '#526078',
        'editorCursor.foreground': '#9ee493'
      }
    });

    editor = monaco.editor.create(host, {
      value,
      language: 'sql',
      theme: 'catabase-dark',
      automaticLayout: true,
      fontFamily: 'JetBrains Mono, SFMono-Regular, Menlo, Consolas, monospace',
      fontSize: 14,
      minimap: { enabled: false },
      inlineSuggest: { enabled: supermavenEnabled },
      quickSuggestions: { other: true, comments: false, strings: false },
      readOnly,
      suggestOnTriggerCharacters: true,
      wordBasedSuggestions: 'off',
      scrollBeyondLastLine: false,
      tabCompletion: 'on',
      wordWrap: 'on'
    });
    if (viewState) editor.restoreViewState(viewState);

    subscription = editor.onDidChangeModelContent((event) => {
      const next = editor?.getValue() ?? '';
      if (next !== value) value = next;
      const suggest = !event.isFlush && event.changes.some((change) => shouldTriggerSuggest(change.text));
      const checkJoin = !event.isFlush && event.changes.some((change) => /\s$/.test(change.text));
      if (suggest || checkJoin) {
        window.setTimeout(() => {
          const model = editor?.getModel();
          const position = editor?.getPosition();
          if (suggest || (model && position && sqlJoinSuggestions(model.getValue(), model.getOffsetAt(position), schemaItems).length)) {
            triggerSuggest();
          }
        }, 0);
      }
    });

    editor.addCommand(monaco.KeyMod.Alt | monaco.KeyCode.Escape, triggerSuggest);
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Space, triggerSuggest);
    keydownHandler = (event) => {
      if (!editor?.hasTextFocus()) return;
      const isSuggestShortcut = (event.altKey && event.key === 'Escape') || ((event.metaKey || event.ctrlKey) && event.code === 'Space');
      if (!isSuggestShortcut) return;
      event.preventDefault();
      event.stopPropagation();
      triggerSuggest();
    };
    window.addEventListener('keydown', keydownHandler, true);

    completionProvider = monaco.languages.registerCompletionItemProvider('sql', {
      triggerCharacters: ['.', ' ', '*'],
      provideCompletionItems: (model, position) => {
        const range = completionRange(model, position);
        const context = sqlCompletionContext(model.getValue(), model.getOffsetAt(position), schemaItems);
        const joins = context.kind === 'unqualified' || context.kind === 'expression'
          ? sqlJoinSuggestions(model.getValue(), model.getOffsetAt(position), schemaItems) : [];
        const suggestions = joins.length ? joins.map((join, index) => {
          const start = model.getPositionAt(join.replaceStart);
          const end = model.getPositionAt(join.replaceEnd);
          return {
            label: join.label,
            insertText: join.insertText,
            kind: monaco.languages.CompletionItemKind.Snippet,
            detail: 'Foreign key',
            documentation: join.detail,
            range: new monaco.Range(start.lineNumber, start.column, end.lineNumber, end.column),
            sortText: `0_join_${index.toString().padStart(3, '0')}`
          };
        }) : [
          ...(context.kind === 'unqualified' ? syntaxSuggestions(range) : []),
          ...reflectionSuggestions(range, context)
        ];
        return { suggestions: dedupeSuggestions(suggestions) };
      }
    });

    inlineProvider = monaco.languages.registerInlineCompletionsProvider('sql', {
      provideInlineCompletions: async (model, position, _context, token) => {
        if (!supermavenEnabled || readOnly || !supermavenComplete || model !== editor?.getModel() || !editor.hasTextFocus()) return { items: [] };
        const offset = model.getOffsetAt(position);
        const text = model.getValue();
        if (!text.slice(0, offset).trim() || token.isCancellationRequested) return { items: [] };
        const version = model.getVersionId();
        const abort = new AbortController();
        const cancellation = token.onCancellationRequested(() => abort.abort());
        try {
          const completion = await inlineRequester.request({ documentId: ownerTabId, text, position: offset }, abort.signal);
          const current = editor?.getPosition();
          if (!completion || abort.signal.aborted || !supermavenEnabled || readOnly || model.isDisposed() || model !== editor?.getModel()
            || version !== model.getVersionId() || current?.lineNumber !== position.lineNumber || current.column !== position.column) return { items: [] };
          const edit = inlineEdit(completion, model.getLineContent(position.lineNumber), position.column);
          if (!edit) return { items: [] };
          return { items: [{ insertText: edit.text, range: new monaco.Range(position.lineNumber, edit.startColumn, position.lineNumber, edit.endColumn) }], enableForwardStability: true };
        } finally { cancellation.dispose(); }
      },
      freeInlineCompletions: () => undefined
    });
  });

  onDestroy(() => {
    inlineRequester.cancel();
    subscription?.dispose();
    completionProvider?.dispose();
    inlineProvider?.dispose();
    highlights?.clear();
    if (keydownHandler) window.removeEventListener('keydown', keydownHandler, true);
    editor?.dispose();
    editor = undefined;
    highlights = undefined;
  });

  $: if (editor && editor.getValue() !== value) {
    const position = editor.getPosition();
    editor.setValue(value ?? '');
    if (position) editor.setPosition(position);
  }

  $: if (editor) editor.updateOptions({ readOnly });
  $: if (editor) {
    editor.updateOptions({ inlineSuggest: { enabled: supermavenEnabled && !readOnly } });
    if (!supermavenEnabled || readOnly) {
      inlineRequester.cancel();
      editor.trigger('catabase', 'editor.action.inlineSuggest.hide', {});
    }
  }

  function triggerSuggest() {
    if (!editor || readOnly) return;
    editor.trigger('catabase', 'editor.action.triggerSuggest', {});
  }

  function shouldTriggerSuggest(text: string): boolean {
    if (!editor || readOnly || text.length !== 1) return false;
    const model = editor.getModel();
    const position = editor.getPosition();
    if (!model || !position) return false;
    const prefix = model.getLineContent(position.lineNumber).slice(0, position.column - 1).trimEnd();
    if (text === '*') return /\.\s*\*$/.test(prefix);
    if (/^[A-Za-z0-9_$".]$/.test(text)) return true;
    if (text !== ' ') return false;

    return /\b(select|from|join|where|and|or|by|order|group|update|into|table|set|with|as|on|having|returning)$/i.test(
      prefix
    );
  }

  function completionRange(model: monaco.editor.ITextModel, position: monaco.Position): monaco.IRange {
    const word = model.getWordUntilPosition(position);
    const linePrefix = model.getLineContent(position.lineNumber).slice(0, position.column - 1);
    const replaceStar = /\.\s*\*$/.test(linePrefix);
    const startColumn = replaceStar ? Math.max(1, position.column - 1) : word.startColumn;
    return new monaco.Range(position.lineNumber, startColumn, position.lineNumber, word.endColumn);
  }

  function syntaxSuggestions(range: monaco.IRange): monaco.languages.CompletionItem[] {
    return [
      ...SQL_SNIPPETS.map((snippet, index) => ({
        label: snippet.label,
        kind: monaco.languages.CompletionItemKind.Snippet,
        insertText: snippet.insertText,
        insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
        detail: 'SQL snippet',
        range,
        sortText: `0_${index.toString().padStart(2, '0')}_${snippet.label}`
      })),
      ...SQL_KEYWORDS.map((keyword) => ({
        label: keyword,
        kind: monaco.languages.CompletionItemKind.Keyword,
        insertText: keyword.toLowerCase(),
        detail: 'SQL keyword',
        range,
        sortText: `1_${keyword}`
      }))
    ];
  }

  function reflectionSuggestions(
    range: monaco.IRange,
    context: SqlCompletionContext
  ): monaco.languages.CompletionItem[] {
    if (!schemaItems.length || context.kind === 'none') return [];
    if (context.kind === 'expression') return context.references.map((reference) => ({
      label: reference.label,
      insertText: reference.insertText,
      kind: reference.kind === 'relation' ? monaco.languages.CompletionItemKind.Reference : monaco.languages.CompletionItemKind.Field,
      detail: reference.detail,
      range,
      sortText: `${reference.kind === 'relation' ? '0_alias' : '1_column'}_${reference.label}`
    }));
    if (context.kind === 'qualified') {
      return [...tableSuggestions(range, context.tables, false), ...columnSuggestions(range, context.columns)];
    }

    return [
      ...schemaNameSuggestions(range),
      ...tableSuggestions(range, schemaItems, false),
      ...tableSuggestions(range, schemaItems, true),
      ...columnSuggestions(range, schemaItems)
    ];
  }

  function schemaNameSuggestions(range: monaco.IRange): monaco.languages.CompletionItem[] {
    return Array.from(new Set(schemaItems.map((item) => item.schema))).map((schema) => ({
      label: schema,
      kind: monaco.languages.CompletionItemKind.Module,
      insertText: identifierInsertText(schema),
      detail: 'schema',
      range,
      sortText: `2_schema_${schema}`
    }));
  }

  function tableSuggestions(
    range: monaco.IRange,
    items: SchemaItem[],
    qualified: boolean
  ): monaco.languages.CompletionItem[] {
    return items.map((item) => {
      const label = qualified ? `${item.schema}.${item.name}` : item.name;
      const insertText = qualified
        ? `${identifierInsertText(item.schema)}.${identifierInsertText(item.name)}`
        : identifierInsertText(item.name);
      return {
        label,
        kind: monaco.languages.CompletionItemKind.Reference,
        insertText,
        detail: item.kind,
        documentation: `${item.schema}.${item.name}`,
        range,
        sortText: `${qualified ? '4' : '3'}_${label}`
      };
    });
  }

  function columnSuggestions(range: monaco.IRange, items: SchemaItem[]): monaco.languages.CompletionItem[] {
    return items.flatMap((item) =>
      item.columns.map((column) => ({
        label: column.name,
        kind: monaco.languages.CompletionItemKind.Field,
        insertText: identifierInsertText(column.name),
        detail: `${item.schema}.${item.name} column (${column.typeName})`,
        range,
        sortText: `5_${column.name}`
      }))
    );
  }

  function dedupeSuggestions(items: monaco.languages.CompletionItem[]): monaco.languages.CompletionItem[] {
    const seen = new Set<string>();
    return items.filter((item) => {
      const label = typeof item.label === 'string' ? item.label : item.label.label;
      const key = `${item.kind}:${label}:${item.insertText}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  }

  function identifierInsertText(name: string): string {
    return /^[a-z_][a-z0-9_$]*$/.test(name) ? name : `"${name.replace(/"/g, '""')}"`;
  }

  export async function getExecutionTarget(
    splitSql: (sql: string) => Promise<StatementRange[]>
  ): Promise<ExecutionTarget> {
    const sourceEditor = editor;
    const model = sourceEditor?.getModel();
    if (!sourceEditor || !model) return { sql: value.trim(), start: 0, end: value.length };

    clearHighlights();
    const selections = sourceEditor.getSelections() ?? [];
    const selected = selections
      .filter((selection) => !selection.isEmpty())
      .map((selection) => ({
        sql: model.getValueInRange(selection).trim(),
        start: model.getOffsetAt(selection.getStartPosition()),
        end: model.getOffsetAt(selection.getEndPosition())
      }))
      .filter((selection) => selection.sql.length > 0);

    if (selected.length) {
      highlightOffsets(selected.map(({ start, end }) => ({ start, end })));
      return {
        sql: selected.map((selection) => selection.sql).join('\n\n'),
        start: selected[0].start,
        end: selected[selected.length - 1].end
      };
    }

    // Resolve the statement from the buffer and cursor at Run time, even when
    // statement splitting finishes after a tab switch or another edit.
    const buffer = model.getValue();
    const version = model.getVersionId();
    const position = sourceEditor.getPosition() ?? model.getFullModelRange().getStartPosition();
    const cursor = model.getOffsetAt(position);
    const ranges = await splitSql(buffer);
    const canHighlight = editor === sourceEditor && !model.isDisposed() && sourceEditor.getModel() === model && model.getVersionId() === version;
    if (!ranges.length) {
      if (canHighlight) highlightOffsets([{ start: 0, end: buffer.length }]);
      return { sql: buffer.trim(), start: 0, end: buffer.length };
    }

    const containing = ranges.find((range) => cursor >= range.start && cursor < range.end);
    const previous = ranges.filter((range) => range.end <= cursor).at(-1);
    const next = ranges.find((range) => range.start >= cursor);
    const target = containing ?? previous ?? next ?? ranges[0];
    if (canHighlight) highlightOffsets([target]);
    return { sql: target.text.trim(), start: target.start, end: target.end };
  }

  export function clearHighlights() {
    highlights?.clear();
    const model = editor?.getModel();
    if (model) monaco.editor.setModelMarkers(model, 'catabase', []);
  }

  export function getViewState() {
    return editor?.saveViewState() ?? null;
  }

  export function getSnapshot() {
    return { tabId: ownerTabId, sql: editor?.getValue() ?? value, viewState: getViewState() };
  }

  export function markError(offset: number, message: string) {
    const model = editor?.getModel();
    if (!model) return;
    const position = model.getPositionAt(Math.max(0, offset));
    monaco.editor.setModelMarkers(model, 'catabase', [
      {
        startLineNumber: position.lineNumber,
        startColumn: position.column,
        endLineNumber: position.lineNumber,
        endColumn: position.column + 1,
        message,
        severity: monaco.MarkerSeverity.Error
      }
    ]);
    editor?.revealPositionInCenter(position);
  }

  function highlightOffsets(ranges: Array<{ start: number; end: number }>) {
    const model = editor?.getModel();
    if (!editor || !model) return;
    highlights = editor.createDecorationsCollection(
      ranges.map((range) => ({
        range: new monaco.Range(
          model.getPositionAt(range.start).lineNumber,
          model.getPositionAt(range.start).column,
          model.getPositionAt(range.end).lineNumber,
          model.getPositionAt(range.end).column
        ),
        options: { inlineClassName: 'execution-highlight' }
      }))
    );
  }
</script>

<div bind:this={host} class="editor-host"></div>
