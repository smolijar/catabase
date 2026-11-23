import { invoke, isTauri } from '@tauri-apps/api/core';
import { startActivity } from './activity';

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

export async function copyText(content: string | (() => string), title = 'Copy to clipboard', subject?: string): Promise<void> {
  const finish = startActivity('clipboard', title, subject);
  let attempt = 'Prepare clipboard text';
  try {
    const text = typeof content === 'function' ? content() : content;
    attempt = 'Write to clipboard';
    try {
      await invoke<void>('clipboard_write_text', { text });
    } catch (error) {
      if (isTauri()) throw error;
      await navigator.clipboard.writeText(text);
    }
    finish('completed');
  } catch (error) {
    finish('failed', attempt);
    throw error;
  }
}
