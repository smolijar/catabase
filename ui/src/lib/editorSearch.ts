type SearchKey = Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'isComposing'>;

export function editorSearchShortcut(event: SearchKey): 'find' | 'replace' | null {
  if ((!event.metaKey && !event.ctrlKey) || event.shiftKey || event.isComposing) return null;
  if (event.key.toLowerCase() === 'f') return event.altKey ? 'replace' : 'find';
  if (event.ctrlKey && !event.metaKey && !event.altKey && event.key.toLowerCase() === 'h') return 'replace';
  return null;
}
