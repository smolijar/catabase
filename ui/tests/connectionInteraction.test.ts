import assert from 'node:assert/strict';
import test from 'node:test';
import { createConnectionInteraction } from '../src/lib/connectionInteraction.ts';

test('a single click uses immediately; double-click editing waits for the first activation', async () => {
  let uses = 0, edits = 0, disabled = false;
  let finish!: () => void;
  const actions = createConnectionInteraction(() => {
    uses++; disabled = true;
    return new Promise<void>(resolve => { finish = () => { disabled = false; resolve(); }; });
  }, () => edits++, () => disabled);
  actions.use();
  actions.use();
  const editing = actions.edit();
  assert.equal(uses, 1);
  assert.equal(edits, 0);
  finish();
  await editing;
  assert.equal(edits, 1);
});

test('unrelated busy operations block both use and edit', async () => {
  let calls = 0;
  const actions = createConnectionInteraction(() => { calls++; }, () => { calls++; }, () => true);
  actions.use(); await actions.edit();
  assert.equal(calls, 0);
});

test('editing an idle connection does not activate it; failed activation does not open an editor', async () => {
  let edits = 0;
  const actions = createConnectionInteraction(() => Promise.reject(new Error('failed')), () => edits++, () => false);
  await actions.edit();
  assert.equal(edits, 1);
  actions.use(); await actions.edit();
  assert.equal(edits, 1);
});
