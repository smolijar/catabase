import { invoke, isTauri } from '@tauri-apps/api/core';

export async function readClipboardText(): Promise<string> {
  try {
    return await invoke<string>('clipboard_read_text');
  } catch (error) {
    if (isTauri()) throw error;
    const text = await navigator.clipboard.readText();
    if (text === '') throw new Error('Clipboard does not contain text.');
    return text;
  }
}

export async function copyText(text: string): Promise<void> {
  try {
    await invoke<void>('clipboard_write_text', { text });
  } catch (error) {
    if (isTauri()) throw error;
    await navigator.clipboard.writeText(text);
  }
}
