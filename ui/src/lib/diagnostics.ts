export interface DiagnosticReport {
  id: string | null;
  newCrash: boolean;
  title: string;
  body: string;
  query: { sql: string; phase: string; truncated: boolean; startedAt: string } | null;
  note: string;
}

export const ISSUE_URL = 'https://github.com/smolijar/catabase/issues/new';

export function diagnosticBody(report: DiagnosticReport, includeSql: boolean): string {
  if (!includeSql || !report.query) return report.body;
  const query = report.query;
  // SQL may contain Markdown fences. Keep it inside its own code block.
  const fence = '`'.repeat(Math.max(3, ...Array.from(query.sql.matchAll(/`+/g), match => match[0].length + 1)));
  return `${report.body}\n## Latest SQL${report.id ? ' in the crashed session' : ''}\n\nState: ${query.phase}${query.truncated ? ' (SQL truncated to 64,000 characters)' : ''}\n\n${fence}sql\n${query.sql}\n${fence}\n`;
}

export function issueDraft(title: string, body: string): { title: string; body: string; copyRequired: boolean } {
  const url = new URL(ISSUE_URL);
  url.searchParams.set('title', title);
  url.searchParams.set('body', body);
  const copyRequired = url.href.length > 7_000;
  return { title, body: copyRequired ? 'The full report was copied to the clipboard. Paste it here before submitting the issue.' : body, copyRequired };
}

export function copyableReport(title: string, body: string): string {
  return `# ${title}\n\n${body}`;
}
