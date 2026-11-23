import type { CommitFeedback, CommitMode, CommitOutcome, TxCompletion } from './types';

export const commitModeLabels: Record<CommitMode, string> = {
  auto: 'Auto-commit',
  manual: 'Manual commit',
  smart: 'Smart commit'
};

export const commitModeOptions = [
  { value: 'auto', label: commitModeLabels.auto, description: 'Successful writes commit immediately outside an open transaction.' },
  { value: 'manual', label: commitModeLabels.manual, description: 'The first statement opens a transaction. You choose when to Commit or Rollback.' },
  { value: 'smart', label: commitModeLabels.smart, description: 'Default. Read-only queries run normally. Writes open a transaction for you to Commit or Rollback.' }
] as const;

const outcomeLabels: Record<CommitOutcome, string> = {
  pending: 'pending commit',
  committed: 'committed',
  rolledBack: 'rolled back',
  interrupted: 'transaction interrupted'
};

export function transactionCompletionLabel(outcome: TxCompletion['outcome']): string {
  return outcome === 'interrupted' ? 'Transaction interrupted' : `Transaction ${outcomeLabels[outcome]}`;
}

export function commitFeedbackLabel(feedback: CommitFeedback, completion?: TxCompletion): string {
  if (feedback.outcome === 'pending' && completion && completion.id === feedback.transactionId) {
    // A committed transaction can include a rollback to a savepoint.
    return transactionCompletionLabel(completion.outcome);
  }
  const outcome = outcomeLabels[feedback.outcome];
  if (feedback.affectedRows !== null && feedback.rowVerb) {
    return `${feedback.affectedRows} ${feedback.affectedRows === 1 ? 'row' : 'rows'} ${feedback.rowVerb} · ${outcome}`;
  }
  if (feedback.transactionId && feedback.outcome !== 'pending') {
    return transactionCompletionLabel(feedback.outcome);
  }
  return outcome.charAt(0).toUpperCase() + outcome.slice(1);
}
