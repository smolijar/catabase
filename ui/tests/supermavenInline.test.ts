import assert from 'node:assert/strict';
import test from 'node:test';
import { canRequestSupermaven, createInlineRequester, inlineEdit } from '../src/lib/supermavenInline.ts';
import type { SupermavenCompletion, SupermavenRequest } from '../src/lib/types.ts';

const snapshot = { documentId: 'query-1', text: 'select ', position: 7 };

test('edit requests can reconnect the agent without bypassing opt-in or activation', () => {
  for (const state of ['ready', 'connecting', 'disconnected']) {
    assert.equal(canRequestSupermaven({ enabled: true, state, message: '' }), true);
    assert.equal(canRequestSupermaven({ enabled: false, state, message: '' }), false);
    assert.equal(canRequestSupermaven({ enabled: true, state, message: '', activationUrl: 'https://supermaven.com/activate' }), false);
  }
  for (const state of ['off', 'starting', 'activation', 'error']) {
    assert.equal(canRequestSupermaven({ enabled: true, state, message: '' }), false);
  }
});

test('debounced requests send only the active SQL document and UTF-16 offset', async () => {
  const calls: SupermavenRequest[] = [];
  const requester = createInlineRequester(async (request) => { calls.push(request); return { text: '1', deleteBefore: 0 }; }, async () => {}, () => 'request-1', 1);
  assert.deepEqual(await requester.request(snapshot, new AbortController().signal), { text: '1', deleteBefore: 0 });
  assert.deepEqual(calls, [{ ...snapshot, requestId: 'request-1' }]);
  assert.equal(Object.keys(calls[0]).length, 4);
});

test('cancellation during debounce never sends SQL', async () => {
  let calls = 0;
  const requester = createInlineRequester(async () => { calls++; return null; }, async () => {}, () => 'id', 30);
  const abort = new AbortController();
  const pending = requester.request(snapshot, abort.signal);
  abort.abort();
  assert.equal(await pending, null);
  assert.equal(calls, 0);
});

test('a new request drops old completions and cancels the remote state', async () => {
  let old: (completion: SupermavenCompletion) => void = () => {};
  const cancelled: string[] = [];
  let id = 0;
  const requester = createInlineRequester((request) => request.requestId === '1' ? new Promise((resolve) => { old = resolve; }) : Promise.resolve({ text: 'new', deleteBefore: 0 }), async (id) => { cancelled.push(id); }, () => String(++id), 0);
  const first = requester.request(snapshot, new AbortController().signal);
  await new Promise((resolve) => setTimeout(resolve, 5));
  const next = requester.request({ ...snapshot, text: 'select n', position: 8 }, new AbortController().signal);
  assert.equal(await first, null);
  assert.deepEqual(await next, { text: 'new', deleteBefore: 0 });
  old({ text: 'stale', deleteBefore: 0 });
  assert.deepEqual(cancelled, ['1']);
});

test('tab disposal or disable resolves an in-flight request even if the provider does not reply', async () => {
  const requester = createInlineRequester(() => new Promise(() => {}), async () => {}, () => 'id', 0);
  const pending = requester.request(snapshot, new AbortController().signal);
  await new Promise((resolve) => setTimeout(resolve, 5));
  requester.cancel();
  assert.equal(await pending, null);
});

test('provider failures quietly leave native schema suggestions available', async () => {
  const requester = createInlineRequester(async () => { throw Error('offline'); }, async () => {}, () => 'id', 0);
  assert.equal(await requester.request(snapshot, new AbortController().signal), null);
});

test('inline ranges preserve suffix SQL and permit only verified whitespace dedent', () => {
  assert.deepEqual(inlineEdit({ text: 'id from users', deleteBefore: 0 }, 'select  from users', 8), { text: 'id from users', startColumn: 8, endColumn: 19 });
  assert.equal(inlineEdit({ text: 'id', deleteBefore: 0 }, 'select  from users', 8), null);
  assert.deepEqual(inlineEdit({ text: 'select 1', deleteBefore: 2 }, '  ', 3), { text: 'select 1', startColumn: 1, endColumn: 3 });
  assert.equal(inlineEdit({ text: 'changed', deleteBefore: 6 }, 'select', 7), null);
  for (const count of [-1, 100, 0.5, NaN]) assert.equal(inlineEdit({ text: 'x', deleteBefore: count }, 'select ', 8), null);
});
