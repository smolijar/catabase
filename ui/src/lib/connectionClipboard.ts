// Copy a reference, never a connection URL/configuration containing credentials.
// The backend resolves the saved record when it creates the duplicate.
export function serializeConnectionReference(connection: { id: string }): string {
  return JSON.stringify({ kind: 'catabase.connection', version: 1, id: connection.id });
}

export function parseConnectionReference(text: string): string | null {
  if (text.length > 4096) return null;
  try {
    const value = JSON.parse(text);
    return value?.kind === 'catabase.connection' && value.version === 1
      && typeof value.id === 'string' && value.id.trim() !== '' && value.id.length <= 128
      ? value.id : null;
  } catch {
    return null;
  }
}

export function connectionClipboardAction(event: Pick<KeyboardEvent,
  'key' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'isComposing' | 'repeat' | 'defaultPrevented'
>): 'copy' | 'paste' | null {
  if (event.defaultPrevented || event.isComposing || event.altKey || event.shiftKey
    || !(event.metaKey || event.ctrlKey)) return null;
  const key = event.key.toLowerCase();
  return key === 'c' ? 'copy' : key === 'v' ? 'paste' : null;
}
