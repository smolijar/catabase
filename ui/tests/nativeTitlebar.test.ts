import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { hasNativeMacTitlebar } from '../src/lib/nativeTitlebar.ts';

test('only native macOS windows reserve an overlaid title bar', () => {
  assert.equal(hasNativeMacTitlebar(true, 'MacIntel'), true);
  assert.equal(hasNativeMacTitlebar(true, 'MacPPC'), true);
  for (const platform of ['Win32', 'Linux x86_64', 'iPhone', '']) assert.equal(hasNativeMacTitlebar(true, platform), false);
  assert.equal(hasNativeMacTitlebar(false, 'MacIntel'), false);
});

test('unified title bar keeps native decorations and grants dragging only on macOS', () => {
  const config = JSON.parse(readFileSync(new URL('../../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
  const window = config.app.windows[0];
  assert.equal(window.decorations, true);
  assert.equal(window.titleBarStyle, 'Overlay');
  assert.equal(window.hiddenTitle, true);
  assert.deepEqual(window.trafficLightPosition, { x: 14, y: 20 });
  const capability = JSON.parse(readFileSync(new URL('../../src-tauri/capabilities/macos-titlebar.json', import.meta.url), 'utf8'));
  assert.deepEqual(capability.platforms, ['macOS']);
  assert.deepEqual(capability.windows, ['main']);
  assert.deepEqual(capability.permissions, ['core:window:allow-start-dragging']);
});
