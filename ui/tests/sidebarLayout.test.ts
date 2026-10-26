import assert from 'node:assert/strict';
import test from 'node:test';
import { clampSidebarWidth, DEFAULT_SIDEBAR_WIDTH, restoreSidebarWidth, sidebarWidthBounds } from '../src/lib/sidebarLayout.ts';

test('sidebar widths are bounded while reserving space for the editor and navigation', () => {
  assert.deepEqual(sidebarWidthBounds(1780), { min: 220, max: 640 });
  assert.deepEqual(sidebarWidthBounds(960), { min: 220, max: 431 });
  assert.equal(clampSidebarWidth(100, 1780), 220);
  assert.equal(clampSidebarWidth(1000, 1780), 640);
  assert.equal(clampSidebarWidth(520, 1780), 520);
  assert.equal(clampSidebarWidth(520, 960), 431);
  assert.equal(clampSidebarWidth(Number.NaN, 1780), DEFAULT_SIDEBAR_WIDTH);
});

test('a wide preference is temporarily clamped in a narrow window and restored when widened', () => {
  const preferred = restoreSidebarWidth('600');
  assert.equal(clampSidebarWidth(preferred, 960), 431);
  assert.equal(clampSidebarWidth(preferred, 1280), 600);
  assert.equal(preferred, 600);
});

test('overlay sidebars leave their resize handle on screen even at small viewport widths', () => {
  assert.deepEqual(sidebarWidthBounds(700), { min: 220, max: 640 });
  assert.equal(clampSidebarWidth(640, 700), 640);
  assert.deepEqual(sidebarWidthBounds(240), { min: 191, max: 191 });
  assert.equal(clampSidebarWidth(340, 240), 191);
  assert.equal(clampSidebarWidth(640, 960.5), 431);
  assert.deepEqual(sidebarWidthBounds(0), { min: 0, max: 0 });
});

test('saved preferences restore valid widths and tolerate missing or malformed storage', () => {
  assert.equal(restoreSidebarWidth('484'), 484);
  assert.equal(restoreSidebarWidth('484.4'), 484);
  assert.equal(restoreSidebarWidth('9999'), 640);
  assert.equal(restoreSidebarWidth('100'), 220);
  for (const saved of [null, '', ' ', '0', '-100', 'invalid', 'NaN', 'Infinity']) {
    assert.equal(restoreSidebarWidth(saved), DEFAULT_SIDEBAR_WIDTH);
  }
});
