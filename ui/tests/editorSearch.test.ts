import assert from 'node:assert/strict';
import test from 'node:test';
import { editorSearchShortcut, runInNewTabModifier, tabCloseShortcut, tabNavigationShortcut, workspaceSearchShortcut } from '../src/lib/editorSearch.ts';

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

test('Control+Tab cycles in either direction and Control+1 through 9 select a tab position', () => {
  const control = { ctrlKey: true, metaKey: false };
  assert.deepEqual(tabNavigationShortcut(key('Tab', control)), { step: 1 });
  assert.deepEqual(tabNavigationShortcut(key('Tab', { ...control, shiftKey: true })), { step: -1 });
  for (let i = 1; i <= 9; i++) assert.deepEqual(tabNavigationShortcut(key(String(i), control)), { index: i - 1 });
  assert.equal(tabNavigationShortcut(key('0', control)), null);
  assert.equal(tabNavigationShortcut(key('1', { ...control, shiftKey: true })), null);
  for (const extra of [{ ctrlKey: false }, { altKey: true }, { metaKey: true }, { isComposing: true }]) {
    assert.equal(tabNavigationShortcut(key('Tab', { ...control, ...extra })), null);
  }
});

test('closing a tab uses the unshifted Command or Control chord', () => {
  assert.equal(tabCloseShortcut(key('w')), true);
  assert.equal(tabCloseShortcut(key('w', { metaKey: false, ctrlKey: true })), true);
  assert.equal(tabCloseShortcut(key('w', { shiftKey: true })), false);
  assert.equal(tabCloseShortcut(key('w', { altKey: true })), false);
  assert.equal(tabCloseShortcut(key('w', { isComposing: true })), false);
});

test('the held append modifier uses Command on Mac and Control on other platforms', () => {
  const commandShift = key('Shift', { shiftKey: true });
  const controlShift = key('Shift', { metaKey: false, ctrlKey: true, shiftKey: true });
  assert.equal(runInNewTabModifier(commandShift, 'metaKey'), true);
  assert.equal(runInNewTabModifier(controlShift, 'metaKey'), false);
  assert.equal(runInNewTabModifier(controlShift, 'ctrlKey'), true);
  assert.equal(runInNewTabModifier({ ...commandShift, shiftKey: false }, 'metaKey'), false);
  assert.equal(runInNewTabModifier({ ...commandShift, altKey: true }, 'metaKey'), false);
});
