<script lang="ts">
  import EditorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';
  import * as monaco from 'monaco-editor/esm/vs/editor/editor.api';
  import 'monaco-editor/esm/vs/basic-languages/sql/sql.contribution';
  import { onDestroy, onMount } from 'svelte';
  import type { ExecutionTarget, StatementRange } from '../lib/types';

  export let value = '';
  export let readOnly = false;

  let host: HTMLDivElement;
  let editor: monaco.editor.IStandaloneCodeEditor | undefined;
  let subscription: monaco.IDisposable | undefined;
  let highlights: monaco.editor.IEditorDecorationsCollection | undefined;

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
      readOnly,
      scrollBeyondLastLine: false,
      wordWrap: 'on'
    });

    subscription = editor.onDidChangeModelContent(() => {
      const next = editor?.getValue() ?? '';
      if (next !== value) value = next;
    });
  });

  onDestroy(() => {
    subscription?.dispose();
    highlights?.clear();
    editor?.dispose();
  });

  $: if (editor && editor.getValue() !== value) {
    const position = editor.getPosition();
    editor.setValue(value ?? '');
    if (position) editor.setPosition(position);
  }

  $: if (editor) editor.updateOptions({ readOnly });

  export async function getExecutionTarget(
    splitSql: (sql: string) => Promise<StatementRange[]>
  ): Promise<ExecutionTarget> {
    const model = editor?.getModel();
    if (!editor || !model) return { sql: value.trim(), start: 0, end: value.length };

    clearHighlights();
    const selections = editor.getSelections() ?? [];
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

    const buffer = editor.getValue();
    const ranges = await splitSql(buffer);
    if (!ranges.length) {
      highlightOffsets([{ start: 0, end: buffer.length }]);
      return { sql: buffer.trim(), start: 0, end: buffer.length };
    }

    const position = editor.getPosition() ?? model.getFullModelRange().getStartPosition();
    const cursor = model.getOffsetAt(position);
    const containing = ranges.find((range) => cursor >= range.start && cursor <= range.end);
    const previous = ranges.filter((range) => range.end <= cursor).at(-1);
    const next = ranges.find((range) => range.start >= cursor);
    const target = containing ?? previous ?? next ?? ranges[0];
    highlightOffsets([target]);
    return { sql: target.text.trim(), start: target.start, end: target.end };
  }

  export function clearHighlights() {
    highlights?.clear();
    const model = editor?.getModel();
    if (model) monaco.editor.setModelMarkers(model, 'catabase', []);
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
