#!/usr/bin/env node
// @ts-check
/**
 * Reports frontend modules unreachable from the entry points and exported symbols
 * no other module imports. Baselined: the check fails on new findings and on
 * baseline entries that are no longer dead, so the file drains and cannot go stale.
 */

import { readFileSync, readdirSync, statSync, existsSync } from 'fs';
import { join, extname, dirname, resolve, relative } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');
const JS_DIR = join(ROOT, 'static', 'js');
const BASELINE_FILE = join(__dirname, 'dead-js-baseline.txt');
const SKIP_DIRS = new Set(['dist', 'vendor']);

// Entry points esbuild bundles from, plus the router that lazy-imports pages.
const ENTRIES = ['app.js', 'router.js'];
// router.js calls these on the page namespace it dynamically imports.
const PAGE_CONTRACT = new Set(['init', 'initSetup', 'destroy']);
// Loaded by the importmap in index.html rather than by an import statement.
const EXTERNALLY_LOADED = new Set(['dev-debug.js']);

/** @param {string} dir @returns {string[]} */
function walk(dir) {
  /** @type {string[]} */
  const files = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (SKIP_DIRS.has(entry)) continue;
      files.push(...walk(full));
    } else if (extname(full) === '.js') {
      files.push(full);
    }
  }
  return files;
}

const IMPORT_PATTERNS = [
  /\bimport\s+[^'"]*?\bfrom\s*['"]([^'"]+)['"]/g,
  /\bimport\s*['"]([^'"]+)['"]/g,
  /\bimport\s*\(\s*['"]([^'"]+)['"]\s*\)/g,
  /\bexport\s+[^'"]*?\bfrom\s*['"]([^'"]+)['"]/g,
];

/** @param {string} file @returns {string[]} */
function importsOf(file) {
  const src = readFileSync(file, 'utf8');
  /** @type {string[]} */
  const out = [];
  for (const re of IMPORT_PATTERNS) {
    for (const m of src.matchAll(re)) out.push(m[1]);
  }
  return out;
}

/** @param {string} fromFile @param {string} spec @returns {string | null} */
function resolveSpec(fromFile, spec) {
  if (!spec.startsWith('.')) return null;
  const base = resolve(dirname(fromFile), spec);
  if (existsSync(base) && statSync(base).isFile()) return base;
  if (existsSync(base + '.js')) return base + '.js';
  const index = join(base, 'index.js');
  if (existsSync(index)) return index;
  return null;
}

const allFiles = walk(JS_DIR);
const rel = (/** @type {string} */ f) => relative(ROOT, f);

const reached = new Set();
const queue = ENTRIES.map((e) => join(JS_DIR, e)).filter(existsSync);
while (queue.length > 0) {
  const file = queue.pop();
  if (!file || reached.has(file)) continue;
  reached.add(file);
  for (const spec of importsOf(file)) {
    const target = resolveSpec(file, spec);
    if (target && !reached.has(target)) queue.push(target);
  }
}

const importedNames = new Set();
const namespaceImported = new Set();
for (const file of allFiles) {
  const src = readFileSync(file, 'utf8');
  for (const m of src.matchAll(/\bimport\s*\{([^}]+)\}\s*from/g)) {
    for (const part of m[1].split(',')) {
      const name = part.trim().split(/\s+as\s+/)[0].trim();
      if (name) importedNames.add(name);
    }
  }
  for (const m of src.matchAll(/\bimport\s*\*\s*as\s+\w+\s*from\s*['"]([^'"]+)['"]/g)) {
    const target = resolveSpec(file, m[1]);
    if (target) namespaceImported.add(target);
  }
  // A dynamic import hands the module to a callback, which either destructures
  // the export or reaches it as a property. Both are real uses.
  for (const m of src.matchAll(/\.then\(\s*\(?\s*\{([^}]+)\}\s*\)?\s*=>/g)) {
    for (const part of m[1].split(',')) {
      const name = part.trim().split(':').pop()?.trim();
      if (name) importedNames.add(name);
    }
  }
  for (const m of src.matchAll(
    /\bimport\s*\([^)]*\)\s*\.then\(\s*\(?\s*(\w+)\s*\)?\s*=>\s*\1\.(\w+)/g,
  )) {
    importedNames.add(m[2]);
  }
}

/** @type {string[]} */
const findings = [];

for (const file of allFiles) {
  if (reached.has(file)) continue;
  if (file.endsWith('.test.js')) continue;
  if (EXTERNALLY_LOADED.has(relative(JS_DIR, file))) continue;
  findings.push(`module ${rel(file)}`);
}

for (const file of allFiles) {
  // A namespace import makes every export reachable as a property.
  if (namespaceImported.has(file)) continue;
  const isPage = relative(JS_DIR, file).startsWith('pages/');
  const src = readFileSync(file, 'utf8');
  const names = new Set();
  for (const m of src.matchAll(
    /^\s*export\s+(?:async\s+)?(?:function|const|let|var|class)\s+([A-Za-z_$][\w$]*)/gm,
  )) {
    names.add(m[1]);
  }
  for (const m of src.matchAll(/^\s*export\s*\{([^}]+)\}/gm)) {
    for (const part of m[1].split(',')) {
      const name = part.trim().split(/\s+as\s+/).pop()?.trim();
      if (name) names.add(name);
    }
  }
  for (const name of names) {
    if (isPage && PAGE_CONTRACT.has(name)) continue;
    if (importedNames.has(name)) continue;
    findings.push(`export ${rel(file)} ${name}`);
  }
}

findings.sort();

const baseline = new Set(
  existsSync(BASELINE_FILE)
    ? readFileSync(BASELINE_FILE, 'utf8')
        .split('\n')
        .map((line) => line.replace(/#.*$/, '').trim())
        .filter(Boolean)
    : [],
);

const added = findings.filter((f) => !baseline.has(f));
const stale = [...baseline].filter((f) => !findings.includes(f)).sort();

if (process.argv.includes('--write-baseline')) {
  console.log(findings.join('\n'));
  process.exit(0);
}

let failed = false;

if (added.length > 0) {
  failed = true;
  console.error(`${added.length} newly dead frontend symbol(s) or module(s):`);
  for (const f of added) console.error(`  ${f}`);
  console.error('\nDelete it, import it, or if it is reached in a way this scanner cannot');
  console.error('see (importmap, string dispatch), add it to EXTERNALLY_LOADED with a reason.');
}

if (stale.length > 0) {
  failed = true;
  console.error(`\n${stale.length} baseline entr(y/ies) no longer dead — delete from`);
  console.error(`${relative(ROOT, BASELINE_FILE)}:`);
  for (const f of stale) console.error(`  ${f}`);
}

if (failed) process.exit(1);

console.log('No new dead frontend modules or exports.');
console.log(`  ${allFiles.length} modules, ${reached.size} reachable, ${baseline.size} known dead.`);
