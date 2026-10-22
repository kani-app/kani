#!/usr/bin/env node
// @ts-check
/**
 * Finds endpoint tests that assert a success status and nothing else.
 *
 * A test that only checks `200 OK` proves the route is mounted and the auth guard
 * let the caller through. It cannot fail if the handler returns the wrong records,
 * an empty list, or someone else's data — so it reports green while the behaviour
 * it names is broken.
 *
 * Negative tests are exempt on purpose: `401 without auth` has no body worth
 * asserting, and the status *is* the behaviour.
 *
 * Baselined, so it fails on a new weak test and on a baseline entry that has since
 * been strengthened.
 */

import { readdirSync, readFileSync, existsSync } from 'node:fs';
import { join, dirname, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');
const BASELINE_FILE = join(__dirname, 'weak-test-baseline.txt');
const TEST_DIRS = ['kani-web/tests', 'kani-app/tests'];

/** Statuses that mean "the request was refused", where the status is the behaviour. */
const REFUSAL = /UNAUTHORIZED|FORBIDDEN|NOT_FOUND|BAD_REQUEST|CONFLICT|UNPROCESSABLE|TOO_MANY|METHOD_NOT_ALLOWED|PAYLOAD_TOO_LARGE|UNSUPPORTED|GONE|PRECONDITION|NOT_MODIFIED|INTERNAL|is_client_error|is_server_error/;
const SUCCESS = /\bOK\b|CREATED|ACCEPTED|NO_CONTENT|is_success/;
/** Evidence the test looked at something other than the status line. */
const INSPECTS_RESULT = /body_json|body_array|body_bytes|from_slice|sqlx::query|\.json\(\)|assert_json/;

/** @type {string[]} */
const findings = [];

for (const dir of TEST_DIRS) {
  const full = join(ROOT, dir);
  if (!existsSync(full)) continue;
  for (const entry of readdirSync(full)) {
    if (!entry.endsWith('.rs')) continue;
    const path = join(full, entry);
    const src = readFileSync(path, 'utf8');
    const blocks = src.split(/\n#\[tokio::test\]\n|\n#\[test\]\n|\n#\[sqlx::test[^\]]*\]\n/);
    for (const block of blocks.slice(1)) {
      const named = block.match(/^\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)/);
      if (!named) continue;
      const asserts = block.match(/assert\w*!\s*\(/g) ?? [];
      if (asserts.length === 0) continue;
      const statusAsserts = block.match(/assert(?:_eq|_ne)?!\([^;]*?\.status\(\)[^;]*?;/gs) ?? [];
      if (statusAsserts.length !== asserts.length) continue;
      if (INSPECTS_RESULT.test(block)) continue;
      const blob = statusAsserts.join(' ');
      if (!SUCCESS.test(blob) || REFUSAL.test(blob)) continue;
      findings.push(`${relative(ROOT, path)} ${named[1]}`);
    }
  }
}

findings.sort();

const baseline = new Set(
  existsSync(BASELINE_FILE)
    ? readFileSync(BASELINE_FILE, 'utf8')
        .split('\n')
        .map((l) => l.replace(/#.*$/, '').trim())
        .filter(Boolean)
    : [],
);

if (process.argv.includes('--write-baseline')) {
  console.log(findings.join('\n'));
  process.exit(0);
}

const added = findings.filter((f) => !baseline.has(f));
const stale = [...baseline].filter((f) => !findings.includes(f)).sort();
let failed = false;

if (added.length > 0) {
  failed = true;
  console.error(`${added.length} test(s) assert a success status and nothing else:`);
  for (const f of added) console.error(`  ${f}`);
  console.error('\nAssert something the handler actually produced — a field in the response');
  console.error('body, or the row it should have written. A status-only test cannot fail when');
  console.error('the endpoint returns the wrong data.');
}

if (stale.length > 0) {
  failed = true;
  console.error(`\n${stale.length} baseline entr(y/ies) now assert more — delete from`);
  console.error(`${relative(ROOT, BASELINE_FILE)}:`);
  for (const f of stale) console.error(`  ${f}`);
}

if (failed) process.exit(1);

console.log('No new status-only endpoint tests.');
console.log(`  ${baseline.size} known weak tests awaiting a real assertion.`);
