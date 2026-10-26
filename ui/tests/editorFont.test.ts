import assert from 'node:assert/strict';
import test from 'node:test';
import { DEFAULT_EDITOR_FONT_SIZE, MAX_EDITOR_FONT_SIZE, MIN_EDITOR_FONT_SIZE, fontSizeShortcut, resizeEditorFont, restoreEditorFontSize } from '../src/lib/editorFont.ts';

const key = (key: string, extra: Record<string, unknown> = {}) => ({ key, code: '', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false, isComposing: false, ...extra });

test('Command/Control plus, equals, minus, zero and numeric keypad control editor font size', () => {
  for (const command of [{ metaKey: true }, { metaKey: false, ctrlKey: true }]) {
    for (const value of ['+', '=']) assert.equal(fontSizeShortcut(key(value, command)), 'in');
    assert.equal(fontSizeShortcut(key('+', { ...command, shiftKey: true })), 'in');
    assert.equal(fontSizeShortcut(key('-', command)), 'out');
    assert.equal(fontSizeShortcut(key('0', command)), 'reset');
    assert.equal(fontSizeShortcut(key('Unidentified', { ...command, code: 'NumpadAdd' })), 'in');
    assert.equal(fontSizeShortcut(key('Unidentified', { ...command, code: 'NumpadSubtract' })), 'out');
  }
});

test('typing, IME input, Alt shortcuts, shifted zero and unrelated commands never resize text', () => {
  for (const value of ['+', '-', '0']) {
    assert.equal(fontSizeShortcut(key(value, { metaKey: false })), null);
    assert.equal(fontSizeShortcut(key(value, { altKey: true })), null);
    assert.equal(fontSizeShortcut(key(value, { isComposing: true })), null);
  }
  assert.equal(fontSizeShortcut(key('0', { shiftKey: true })), null);
  for (const value of ['f', 'Enter', 't']) assert.equal(fontSizeShortcut(key(value)), null);
});

test('font size persists as a bounded integer; missing/corrupt preferences use the default', () => {
  for (const value of [null, '', ' ', 'NaN', 'Infinity', '-1', '0', '14.5', '49', '{}']) assert.equal(restoreEditorFontSize(value), DEFAULT_EDITOR_FONT_SIZE);
  for (const size of [MIN_EDITOR_FONT_SIZE, 18, MAX_EDITOR_FONT_SIZE]) assert.equal(restoreEditorFontSize(String(size)), size);
  assert.equal(resizeEditorFont(14, 'in'), 15);
  assert.equal(resizeEditorFont(14, 'out'), 13);
  assert.equal(resizeEditorFont(MAX_EDITOR_FONT_SIZE, 'in'), MAX_EDITOR_FONT_SIZE);
  assert.equal(resizeEditorFont(MIN_EDITOR_FONT_SIZE, 'out'), MIN_EDITOR_FONT_SIZE);
  assert.equal(resizeEditorFont(25, 'reset'), DEFAULT_EDITOR_FONT_SIZE);
  assert.equal(resizeEditorFont(NaN, 'in'), DEFAULT_EDITOR_FONT_SIZE + 1);
});
