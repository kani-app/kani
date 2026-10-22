
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const HERE = dirname(fileURLToPath(import.meta.url));
const SCRIPT = join(HERE, 'audit-tokens.mjs');
const FIXTURE = join(HERE, '..', 'kani-cli', 'tests', 'fixtures', 'audit-tokens');

/** Runs the scanner and returns { code, lines, literals }. */
function run(args) {
  try {
    const out = execFileSync(process.execPath, [SCRIPT, '--dir', FIXTURE, ...args], {
      encoding: 'utf8',
    });
    return { code: 0, out };
  } catch (e) {
    return { code: e.status, out: (e.stdout ?? '') + (e.stderr ?? '') };
  }
}

const literalsOf = (out) =>
  out
    .split('\n')
    .filter((l) => /^.+:\d+: /.test(l))
    .map((l) => l.replace(/^.+:\d+: /, ''));

test('finds hex, rgb and tailwind palette violations', () => {
  const { out } = run([]);
  const literals = literalsOf(out);
  assert.ok(literals.includes('#fff'), 'should flag 3-digit hex');
  assert.ok(literals.includes('#e8545a'), 'should flag 6-digit hex');
  assert.ok(literals.some((l) => l.startsWith('rgb')), 'should flag rgb() call');
  assert.ok(literals.includes('text-white'), 'should flag tailwind colour literal');
  assert.ok(literals.includes('bg-gray-800'), 'should flag tailwind palette class');
  assert.equal(literals.length, 5, 'expected exactly 5 violations');
});

test('semantic tokens are not violations', () => {
  const literals = literalsOf(run([]).out);
  assert.ok(!literals.includes('text-accent'), 'text-accent is a semantic token');
  assert.ok(!literals.some((l) => l.includes('bg-surface')), 'bg-surface is a semantic token');
});

test('the ratchet fails only above the baseline', () => {
  assert.equal(run(['--check', '--max', '4']).code, 1, '5 violations exceed a baseline of 4');
  assert.equal(run(['--check', '--max', '5']).code, 0, '5 violations do not exceed 5');
});

test('without --check it reports but does not fail', () => {
  const { code, out } = run(['--max', '0']);
  assert.equal(code, 0, 'reporting mode never fails the build');
  assert.match(out, /5 violation\(s\) found/);
});

test('a line-level audit-ignore opts that line out', () => {
  const literals = literalsOf(run([]).out);
  assert.equal(literals.length, 5);
});
