import assert from 'node:assert/strict';
import test from 'node:test';
import { releaseLink } from '../src/lib/releaseNotes.ts';

test('release links stay on the repository and resolve relative commit/release links', () => {
  assert.equal(releaseLink('commit/abc'), 'https://github.com/smolijar/catabase/commit/abc');
  assert.equal(releaseLink('https://github.com/smolijar/catabase/compare/v0.1.3...v0.1.4'), 'https://github.com/smolijar/catabase/compare/v0.1.3...v0.1.4');
  for (const url of ['javascript:alert(1)', 'data:text/html,boom', 'file:///private/data', 'https://github.com.evil.test/smolijar/catabase/', 'https://evil.test/', 'https://github.com/smolijar/catabase-evil/commit/x', 'https://user:secret@github.com/smolijar/catabase/', 'https://github.com:1234/smolijar/catabase/', '//evil.test/']) {
    assert.equal(releaseLink(url), null, url);
  }
});
