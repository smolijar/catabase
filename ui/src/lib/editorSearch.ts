type SearchKey = Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'isComposing'>;

export function workspaceSearchShortcut(event: SearchKey): boolean {
  return !event.isComposing && !event.altKey && !event.shiftKey
    && (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k';
}

export function editorSearchShortcut(event: SearchKey): 'find' | 'replace' | null {
  if ((!event.metaKey && !event.ctrlKey) || event.shiftKey || event.isComposing) return null;
  if (event.key.toLowerCase() === 'f') return event.altKey ? 'replace' : 'find';
  if (event.ctrlKey && !event.metaKey && !event.altKey && event.key.toLowerCase() === 'h') return 'replace';
  return null;
}

export function tabNavigationShortcut(event: SearchKey): { step: number } | { index: number } | null {
  if (!event.ctrlKey || event.metaKey || event.altKey || event.isComposing) return null;
  if (event.key === 'Tab') return { step: event.shiftKey ? -1 : 1 };
  return !event.shiftKey && /^[1-9]$/.test(event.key) ? { index: Number(event.key) - 1 } : null;
}

export function tabCloseShortcut(event: SearchKey): boolean {
  return !event.isComposing && !event.altKey && !event.shiftKey
    && (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'w';
}

export function runInNewTabModifier(event: Pick<SearchKey, 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'>, primary: 'metaKey' | 'ctrlKey'): boolean {
  return event[primary] && event.shiftKey && !event.altKey;
}
