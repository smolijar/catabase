export const DEFAULT_EDITOR_FONT_SIZE = 14;
export const MIN_EDITOR_FONT_SIZE = 8;
export const MAX_EDITOR_FONT_SIZE = 48;
export const EDITOR_FONT_STORAGE_KEY = 'catabase.editorFontSize';
export type FontSizeAction = 'in' | 'out' | 'reset';

type FontKey = Pick<KeyboardEvent, 'key' | 'code' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'isComposing'>;

export function fontSizeShortcut(event: FontKey): FontSizeAction | null {
  if ((!event.metaKey && !event.ctrlKey) || event.altKey || event.isComposing) return null;
  if (event.key === '+' || event.key === '=' || event.code === 'NumpadAdd' || event.code === 'Equal') return 'in';
  if (event.key === '-' || event.key === '_' || event.code === 'NumpadSubtract' || event.code === 'Minus') return 'out';
  if (!event.shiftKey && (event.key === '0' || event.code === 'Digit0' || event.code === 'Numpad0')) return 'reset';
  return null;
}

export function restoreEditorFontSize(value: string | null): number {
  const size = Number(value);
  return value && value.trim() && Number.isInteger(size) && size >= MIN_EDITOR_FONT_SIZE && size <= MAX_EDITOR_FONT_SIZE ? size : DEFAULT_EDITOR_FONT_SIZE;
}

export function resizeEditorFont(size: number, action: FontSizeAction): number {
  if (action === 'reset') return DEFAULT_EDITOR_FONT_SIZE;
  const current = Number.isFinite(size) ? Math.round(size) : DEFAULT_EDITOR_FONT_SIZE;
  return Math.min(MAX_EDITOR_FONT_SIZE, Math.max(MIN_EDITOR_FONT_SIZE, current + (action === 'in' ? 1 : -1)));
}
