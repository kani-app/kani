#!/usr/bin/env node

import { sanitizeCss } from '../static/js/sanitize-css.js';

/** @type {[string, string, string, string][]} name, input, expected css, expected stripped */
const CASES = [
  [
    'import',
    '@import url(evil.css); .btn { color: red }',
    '[data-kani-theme] .btn {\n  color: red;\n}',
    'at-rule @import',
  ],
  [
    'url_decl',
    '.a { color: red; background: url(x.png); font-size: 12px }',
    '[data-kani-theme] .a {\n  color: red;\n  font-size: 12px;\n}',
    'declaration `background`',
  ],
  ['scope', '.btn { color: red }', '[data-kani-theme] .btn {\n  color: red;\n}', ''],
  ['root', ':root { --x: 1 }', ':root[data-kani-theme] {\n  --x: 1;\n}', ''],
  [
    'media',
    '@media (min-width: 40rem) { .a { color: red } }',
    '@media (min-width: 40rem) { .a { color: red } }',
    '',
  ],
  ['fontface', '@font-face { src: local(x) }', '', 'at-rule @font-face'],
  [
    'comment',
    '/* } body { background: url(x) */ .a { color: red }',
    '[data-kani-theme] .a {\n  color: red;\n}',
    '',
  ],
  ['unbalanced', '.a { color: red', '', 'unparseable input'],
];

let failures = 0;
for (const [name, input, wantCss, wantStripped] of CASES) {
  const got = sanitizeCss(input);
  if (got.css !== wantCss) {
    failures += 1;
    console.error(
      `✘ ${name}: css mismatch\n  want: ${JSON.stringify(wantCss)}\n  got:  ${JSON.stringify(got.css)}`,
    );
  }
  const gotStripped = got.stripped.join('|');
  if (gotStripped !== wantStripped) {
    failures += 1;
    console.error(
      `✘ ${name}: stripped mismatch\n  want: ${JSON.stringify(wantStripped)}\n  got:  ${JSON.stringify(gotStripped)}`,
    );
  }
}

{
  const once = sanitizeCss('.btn { color: red }').css;
  const twice = sanitizeCss(once).css;
  if (once === twice) {
    failures += 1;
    console.error(
      '✘ scoping became idempotent — applyCustomCss can now drop its `raw` option ' +
        '(and this check should be deleted along with it)',
    );
  } else if (!twice.includes('[data-kani-theme] [data-kani-theme]')) {
    failures += 1;
    console.error(
      `✘ double-scoping changed shape; re-check applyCustomCss.\n  got: ${JSON.stringify(twice)}`,
    );
  }
}

if (failures > 0) {
  console.error(
    `\n${failures} sanitiser parity failure(s). The client mirror and the Rust ` +
      `sanitiser must agree — update both, or the theme editor will misreport ` +
      `what gets saved.`,
  );
  process.exit(1);
}

console.log(`Sanitiser parity OK (${CASES.length} fixtures match the Rust implementation).`);
