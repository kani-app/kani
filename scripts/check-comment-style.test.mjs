import assert from 'node:assert/strict';
import { test } from 'node:test';

import { checkSource } from './check-comment-style.mjs';

const rules = (path, source) => checkSource(path, source).map((item) => item.rule);

test('rejects decorative dividers and untracked debt', () => {
  const found = rules('sample.rs', '// ── Section ──\n// TODO: later\n');
  assert.deepEqual(found, ['divider', 'debt-marker']);
});

test('rejects project-plan labels in source comments', () => {
  const source = '//! Group C — transport checks.\n// Wave 10 reader state.\n';
  assert.deepEqual(rules('sample.rs', source), ['planning-label', 'planning-label']);
});

test('accepts issue-linked debt and machine directives', () => {
  const found = rules(
    'sample.js',
    '// TODO(#123): replace this\nconst color = "#fff"; // audit-ignore: token source\n',
  );
  assert.deepEqual(found, []);
});

test('rejects oversized ordinary prose but permits contract docs', () => {
  const prose = '// one\n// two\n// three\n// four\n';
  const docs = '/// one\n/// two\n/// three\n/// four\n';
  assert.deepEqual(rules('sample.rs', prose), ['prose-block']);
  assert.deepEqual(rules('sample.rs', docs), []);
});

test('does not parse comment markers inside strings', () => {
  const source = 'const a = "// ─────";\nconst b = `/* TODO */`;\n';
  assert.deepEqual(rules('sample.js', source), []);
});

test('does not parse SQL comment markers inside quoted values', () => {
  const source = "INSERT INTO notes(value) VALUES ('-- TODO');\n";
  assert.deepEqual(rules('sample.sql', source), []);
});

test('skips generated files', () => {
  const source = '// @generated\n// ─────\n';
  assert.deepEqual(rules('sample.rs', source), []);
});

test('enforces comment line length', () => {
  const source = `// ${'x'.repeat(101)}\n`;
  assert.deepEqual(rules('sample.rs', source), ['line-length']);
});

test('moves historical debugging evidence out of source comments', () => {
  const source = [
    '// Before the fix this path never applied the limiter.',
    '// Historically each backend had a separate implementation.',
    '// The old layout duplicated every author.',
  ].join('\n');
  assert.deepEqual(rules('sample.rs', source), [
    'durable-knowledge',
    'durable-knowledge',
    'durable-knowledge',
  ]);
});

test('permits present compatibility and sequencing constraints', () => {
  const source = '// Legacy values remain accepted.\n// Validate the host before dialing.\n';
  assert.deepEqual(rules('sample.rs', source), []);
});

test('permits current constraints and statistical terminology', () => {
  const source = '/// Median quality across sampled pages.\n// Redirects must pass the hop policy.\n';
  assert.deepEqual(rules('sample.rs', source), []);
});

test('requires suppression directives to explain their exception', () => {
  assert.deepEqual(rules('sample.js', '// @ts-ignore\n'), ['directive-justification']);
  assert.deepEqual(rules('sample.js', '// @ts-ignore: dynamic key\n'), []);
});

test('keeps prose separate from type-check directives', () => {
  assert.deepEqual(rules('sample.js', '// @ts-check component notes\n'), ['directive-prose']);
  assert.deepEqual(rules('sample.js', '// @ts-check\n'), []);
});
