import assert from 'node:assert/strict';
import test from 'node:test';
import { orderConnections, restoreConnectionOrder } from '../src/lib/connectionOrder.ts';

test('refresh keeps saved positions with fresh connection records and appends additions', () => {
  const active = { id: 'second', isActive: true, name: 'Renamed' };
  const first = { id: 'first', isActive: false, name: 'First' };
  const added = { id: 'new', isActive: false, name: 'New' };
  const order = restoreConnectionOrder('["first","deleted","second","second"]');
  const records = [active, added, first];
  assert.deepEqual(orderConnections(records, order), [first, active, added]);
  assert.deepEqual(orderConnections(records, []), records);
});

test('malformed connection order falls back to the returned list', () => {
  assert.deepEqual(restoreConnectionOrder('{'), []);
  assert.deepEqual(restoreConnectionOrder(null), []);
  assert.deepEqual(restoreConnectionOrder('["first",null,1,{},"second"]'), ['first', 'second']);
});
