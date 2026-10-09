// Keep SQL, database results and connection configuration off the clipboard.
// Paste resolves the live draft by ID in this workspace, like connection copies.
export function serializeTabReference(tab: { id: string }): string {
  return JSON.stringify({ kind: 'catabase.tab', version: 1, id: tab.id });
}

export function parseTabReference(text: string): string | null {
  if (text.length > 4096) return null;
  try {
    const value = JSON.parse(text);
    return value?.kind === 'catabase.tab' && value.version === 1
      && typeof value.id === 'string' && value.id.trim() !== '' && value.id.length <= 128
      ? value.id : null;
  } catch {
    return null;
  }
}
