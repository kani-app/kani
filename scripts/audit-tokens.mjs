#!/usr/bin/env node

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, extname } from 'node:path';

const argv = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = argv.indexOf(name);
  return i === -1 ? fallback : argv[i + 1];
};
const DIR = flag('--dir', 'static/js');
const CHECK = argv.includes('--check');
const MAX = Number(flag('--max', '0'));

const PALETTE = [
  'gray', 'zinc', 'slate', 'neutral', 'stone', 'red', 'orange', 'amber', 'yellow',
  'lime', 'green', 'emerald', 'teal', 'cyan', 'sky', 'blue', 'indigo', 'violet',
  'purple', 'fuchsia', 'pink', 'rose', 'white', 'black',
].join('|');

// `\b` after the hex body matches Rust's regex crate: a 3, 6 or 8 digit run
// followed by a word boundary, so `#abcd` is not reported as `#abc`.
const HEX = /#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3})\b/g;
const FUNC = /rgb[a]?\s*\(|hsl[a]?\s*\(/g;
const TAILWIND = new RegExp(
  String.raw`\b(?:text|bg|border|ring|fill|stroke|from|to|via|shadow|accent)-(?:${PALETTE})(?:-\d+)?`,
  'g',
);

const SKIP_DIRS = new Set(['dist', 'vendor', 'node_modules']);

/** @param {string} line */
function literalsIn(line) {
  const found = [];
  for (const m of line.matchAll(HEX)) found.push(m[0]);
  // Reported without the trailing paren/space, matching the Rust output.
  for (const m of line.matchAll(FUNC)) found.push(m[0].replace(/[(\s]+$/, ''));
  for (const m of line.matchAll(TAILWIND)) found.push(m[0]);
  return found;
}

/** @param {string} dir @param {{file:string,line:number,literal:string}[]} out */
function walk(dir, out) {
  let entries;
  try {
    entries = readdirSync(dir).sort();
  } catch (e) {
    console.error(`cannot read ${dir}: ${e.message}`);
    process.exit(2);
  }
  for (const name of entries) {
    if (SKIP_DIRS.has(name)) continue;
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, out);
    else if (extname(path) === '.js') scanFile(path, out);
  }
}

/** @param {string} path @param {{file:string,line:number,literal:string}[]} out */
function scanFile(path, out) {
  const content = readFileSync(path, 'utf8');
  if (content.includes('audit-ignore-file')) return;
  content.split('\n').forEach((line, i) => {
    if (line.includes('audit-ignore')) return;
    for (const literal of literalsIn(line)) {
      out.push({ file: path, line: i + 1, literal });
    }
  });
}

if (argv.includes('--unused-css')) {
  auditUnusedSelectors();
}

/**
 * Reports bespoke class selectors in app.css that no markup or script names.
 * Shares this file with the colour audit because both parse the same stylesheet.
 */
function auditUnusedSelectors() {
  const cssPath = 'static/css/app.css';
  const css = readFileSync(cssPath, 'utf8');
  const sources = [];
  walkAll('static/js', sources);
  walkAll('static', sources, '.html');
  const haystack = sources.map((f) => readFileSync(f, 'utf8')).join('\n');

  const declared = [...new Set([...css.matchAll(/^\s*\.([a-z][a-z0-9-]*)\s*[,{]/gim)].map((m) => m[1]))];
  const unused = declared.filter((name) => !haystack.includes(name));

  const baselineFile = 'scripts/unused-css-baseline.txt';
  const baseline = new Set(
    existsSync(baselineFile)
      ? readFileSync(baselineFile, 'utf8')
        .split('\n')
        .map((l) => l.replace(/#.*$/, '').trim())
        .filter(Boolean)
      : [],
  );

  if (argv.includes('--write-baseline')) {
    console.log(unused.sort().join('\n'));
    process.exit(0);
  }

  const added = unused.filter((n) => !baseline.has(n)).sort();
  const stale = [...baseline].filter((n) => !unused.includes(n)).sort();

  if (added.length > 0) {
    console.error(`error: ${added.length} unreferenced class selector(s) in ${cssPath}:`);
    for (const n of added) console.error(`  .${n}`);
  }
  if (stale.length > 0) {
    console.error(`error: ${stale.length} baseline entr(y/ies) now referenced — remove from ${baselineFile}:`);
    for (const n of stale) console.error(`  .${n}`);
  }
  if (added.length > 0 || stale.length > 0) process.exit(1);

  console.log(`No new unreferenced selectors in ${cssPath}.`);
  console.log(`  ${declared.length} bespoke classes, ${baseline.size} known unused.`);
  process.exit(0);
}

function walkAll(dir, out, ext = '.js') {
  for (const entry of readdirSync(dir)) {
    const full = `${dir}/${entry}`;
    if (statSync(full).isDirectory()) {
      if (SKIP_DIRS.has(entry)) continue;
      walkAll(full, out, ext);
    } else if (full.endsWith(ext)) {
      out.push(full);
    }
  }
}

const violations = [];
walk(DIR, violations);
for (const v of violations) console.log(`${v.file}:${v.line}: ${v.literal}`);

if (violations.length === 0) {
  console.log(`No hard-coded colour literals found in ${DIR}.`);
  process.exit(0);
}

console.log(`\n${violations.length} violation(s) found (baseline max: ${MAX}).`);
if (CHECK && violations.length > MAX) {
  // `error:` prefix matches what cargo printed, so CI logs read the same.
  console.error(
    `error: ${violations.length} hard-coded colour violation(s) exceeds baseline of ${MAX} — ` +
    'migrate new literals to design tokens (or lower the baseline as you clean up)',
  );
  process.exit(1);
}
