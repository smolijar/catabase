import assert from 'node:assert/strict';
import test from 'node:test';
import { editorSearchShortcut, workspaceSearchShortcut } from '../src/lib/editorSearch.ts';

const key = (key: string, extra: Record<string, unknown> = {}) => ({ key, metaKey: true, ctrlKey: false, altKey: false, shiftKey: false, isComposing: false, ...extra });

test('workspace search captures only the unshifted Command/Control+K shortcut', () => {
  assert.equal(workspaceSearchShortcut(key('k')), true);
  assert.equal(workspaceSearchShortcut(key('K', { metaKey: false, ctrlKey: true })), true);
  assert.equal(workspaceSearchShortcut(key('d')), false);
  for (const modifiers of [{ metaKey: false }, { altKey: true }, { shiftKey: true }, { isComposing: true }]) {
    assert.equal(workspaceSearchShortcut(key('k', modifiers)), false);
  }
});

test('Command/Control+F opens editor find, including outside the editor', () => {
  assert.equal(editorSearchShortcut(key('f')), 'find');
  assert.equal(editorSearchShortcut(key('F')), 'find');
  assert.equal(editorSearchShortcut(key('f', { metaKey: false, ctrlKey: true })), 'find');
});

test('IDE replace shortcuts are recognized without capturing unrelated commands', () => {
  assert.equal(editorSearchShortcut(key('f', { altKey: true })), 'replace');
  assert.equal(editorSearchShortcut(key('h', { metaKey: false, ctrlKey: true })), 'replace');
  for (const value of ['f', 'h']) {
    assert.equal(editorSearchShortcut(key(value, { metaKey: false })), null);
    assert.equal(editorSearchShortcut(key(value, { isComposing: true })), null);
    assert.equal(editorSearchShortcut(key(value, { shiftKey: true })), null);
  }
  assert.equal(editorSearchShortcut(key('h', { altKey: true })), null);
  assert.equal(editorSearchShortcut(key('h')), null); // macOS Hide app
  for (const value of ['Enter', 'Escape', 'g', '+', '-']) assert.equal(editorSearchShortcut(key(value)), null);
});
