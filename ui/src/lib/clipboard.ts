import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager';

export async function readClipboardText(): Promise<string> {
  try {
    return await readText();
  } catch {
    const text = await navigator.clipboard.readText();
    if (text === '') throw new Error('Clipboard does not contain text.');
    return text;
  }
}

export async function copyText(text: string): Promise<void> {
  try {
    await writeText(text);
  } catch {
    await navigator.clipboard.writeText(text);
  }
}
