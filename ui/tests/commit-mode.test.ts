import assert from 'node:assert/strict';
import test from 'node:test';
import { commitFeedbackLabel } from '../src/lib/commitMode.ts';
import type { CommitFeedback } from '../src/lib/types.ts';

const update: CommitFeedback = {
  affectedRows: 3,
  rowVerb: 'updated',
  outcome: 'pending',
  transactionId: 'tx-1'
};

test('write feedback distinguishes pending and immediately committed rows', () => {
  assert.equal(commitFeedbackLabel(update), '3 rows updated · pending commit');
  assert.equal(commitFeedbackLabel({ ...update, affectedRows: 1, outcome: 'committed', transactionId: null }), '1 row updated · committed');
});

test('finishing a transaction replaces pending feedback across its results', () => {
  assert.equal(commitFeedbackLabel(update, { id: 'tx-1', outcome: 'rolledBack' }), 'Transaction rolled back');
  assert.equal(commitFeedbackLabel(update, { id: 'tx-1', outcome: 'committed' }), 'Transaction committed');
  assert.equal(commitFeedbackLabel(update, { id: 'tx-2', outcome: 'committed' }), '3 rows updated · pending commit');
});
