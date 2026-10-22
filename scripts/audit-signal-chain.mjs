// @ts-check
/**
 * Finds values that are written but cannot be read back.
 *
 * Caller counts cannot detect a value discarded between persistence and the
 * UI. This script inspects the chain:
 *
 *     written -> selected -> serialised -> rendered
 *
 * and reports the hop where a column falls off. It is a lead generator, not a
 * verdict: read the code before believing any row.
 *
 * Known blind spots:
 *
 *  - `SELECT *` hides every column, so a column read that way looks unselected.
 *  - Values combined outside SQL require manual verification.
 *  - Explicit SELECT lists extending beyond the scan window look unselected.
 *  - Filter-only columns are legitimate even though they are never selected.
 *  - `DELETE ... RETURNING`, trait parameters, and other indirect reads are invisible.
 *
 * The signal worth chasing is a column written with a meaningful value whose
 * only reads exclude that value.
 *
 * Usage: node scripts/audit-signal-chain.mjs [--all]
 */

import { readFileSync, readdirSync, statSync } from 'fs';
import { join, extname } from 'path';

const ROOT = process.env.KANI_ROOT ?? process.cwd();
const SHOW_ALL = process.argv.includes('--all');
const SELECT_LOOKAHEAD_CHARS = 2400;

function walk(dir, out = [], skip = ['target', 'node_modules', 'dist', 'vendor', '.git', 'site']) {
  for (const entry of readdirSync(dir)) {
    if (skip.includes(entry)) continue;
    const p = join(dir, entry);
    const st = statSync(p);
    if (st.isDirectory()) walk(p, out, skip);
    else out.push(p);
  }
  return out;
}

const files = walk(ROOT);
const rustSrc = files.filter((f) => extname(f) === '.rs' && !f.includes('/tests/') && !f.includes('/migrations/'));
const jsSrc = files.filter((f) => extname(f) === '.js' && f.includes('/static/js/'));
const migrations = files.filter((f) => f.startsWith(join(ROOT, 'migrations')) && f.endsWith('.sql'));

const rustText = rustSrc.map((f) => readFileSync(f, 'utf8')).join('\n');
const jsText = jsSrc.map((f) => readFileSync(f, 'utf8')).join('\n');
const sqlText = rustText;

/** Columns per table, from the migrations. */
function schema() {
  const tables = new Map();
  for (const f of migrations) {
    const sql = readFileSync(f, 'utf8');
    for (const m of sql.matchAll(/CREATE TABLE (?:IF NOT EXISTS )?[`"]?(\w+)[`"]?\s*\(([\s\S]*?)\n\s*\);/gi)) {
      const table = m[1];
      const cols = tables.get(table) ?? new Set();
      for (const line of m[2].split('\n')) {
        const c = line.trim().match(/^[`"]?(\w+)[`"]?\s+(INTEGER|TEXT|REAL|BLOB|BOOLEAN|NUMERIC|TIMESTAMP)/i);
        if (c && !/^(PRIMARY|FOREIGN|UNIQUE|CHECK|CONSTRAINT)$/i.test(c[1])) cols.add(c[1]);
      }
      tables.set(table, cols);
    }
    for (const m of sql.matchAll(/ALTER TABLE [`"]?(\w+)[`"]?\s+ADD COLUMN [`"]?(\w+)[`"]?/gi)) {
      const cols = tables.get(m[1]) ?? new Set();
      cols.add(m[2]);
      tables.set(m[1], cols);
    }
  }
  return tables;
}

const NOISE = new Set(['id', 'created_at', 'updated_at', 'user_id', 'manga_id', 'chapter_id', 'source_id']);

const rows = [];
for (const [table, cols] of schema()) {
  for (const col of cols) {
    if (NOISE.has(col)) continue;
    const word = new RegExp(`\\b${col}\\b`, 'g');

    const written = (sqlText.match(new RegExp(`(INSERT INTO[\\s\\S]{0,400}?\\b${col}\\b|SET[^;"]{0,200}\\b${col}\\b\\s*=)`, 'g')) ?? []).length;
    const mentions = (rustText.match(word) ?? []).length;
    // A SELECT that only ever appears inside a filter is not a read.
    const selected = (sqlText.match(new RegExp(`SELECT[\\s\\S]{0,${SELECT_LOOKAHEAD_CHARS}}?\\b${col}\\b`, 'g')) ?? []).length;
    const filtered = (sqlText.match(new RegExp(`(WHERE|AND|OR)[^;"']{0,80}\\b${col}\\b\\s*(=|IS|!=|<>)`, 'g')) ?? []).length;
    const inJs = (jsText.match(word) ?? []).length;

    rows.push({ table, col, written, mentions, selected, filtered, inJs });
  }
}

const suspects = rows.filter((r) => {
  const unusedSchemaColumn = r.written === 0 && r.mentions === 0;
  const reachesClient = r.inJs > 0 && r.selected > r.filtered;
  const neverSelected = r.written > 0 && r.selected === 0;
  const usedOnlyAsFilter = r.written > 0 && r.filtered > 0 && r.selected <= r.filtered;
  if (unusedSchemaColumn || reachesClient) return false;
  return neverSelected || usedOnlyAsFilter;
});

const shown = SHOW_ALL ? rows : suspects;
shown.sort((a, b) => (a.table + a.col).localeCompare(b.table + b.col));

console.log(`Scanned ${rows.length} columns across ${schema().size} tables\n`);
console.log('table.column'.padEnd(46), 'writes  selects  filters  js   verdict');
for (const r of shown) {
  let verdict = 'check';
  if (r.written > 0 && r.selected === 0) verdict = 'written, never selected';
  else if (r.filtered > 0 && r.selected <= r.filtered) verdict = 'only ever a filter';
  if (r.inJs === 0 && verdict !== 'check') verdict += '; absent from the UI';
  console.log(
    `${r.table}.${r.col}`.padEnd(46),
    String(r.written).padStart(5),
    String(r.selected).padStart(8),
    String(r.filtered).padStart(8),
    String(r.inJs).padStart(4),
    ' ' + verdict,
  );
}
console.log(`\n${shown.length} lead(s). Each is a lead, not a verdict — read the code.`);
