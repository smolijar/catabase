import { Marked } from 'marked';
import DOMPurify from 'dompurify';

const markdown = new Marked({ gfm: true, renderer: { checkbox: ({ checked }) => checked ? '☑ ' : '☐ ' } });

export function renderReleaseNotes(source: string): string {
  return DOMPurify.sanitize(markdown.parse(source, { async: false }), {
    ALLOWED_TAGS: ['p', 'br', 'strong', 'em', 'del', 'code', 'pre', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'ul', 'ol', 'li', 'blockquote', 'hr', 'a', 'table', 'thead', 'tbody', 'tr', 'th', 'td'],
    ALLOWED_ATTR: ['href', 'title', 'start'],
    ALLOW_DATA_ATTR: false,
    ALLOW_ARIA_ATTR: false
  });
}

export function releaseLink(href: string): string | null {
  try {
    const url = new URL(href, 'https://github.com/smolijar/catabase/');
    return url.protocol === 'https:' && url.hostname === 'github.com' && !url.username && !url.password && !url.port
      && url.pathname.startsWith('/smolijar/catabase/') ? url.href : null;
  } catch { return null; }
}
