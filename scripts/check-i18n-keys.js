#!/usr/bin/env node
// @ts-check
/**
 * Verifies the catalog and its call sites agree in both directions: every static
 * `t()` key exists in the base English catalog, and every catalog key is reachable.
 * Exits with status 1 on a referenced-but-undefined key, an orphaned key, or a
 * duplicate definition.
 */

import { readFileSync, readdirSync, statSync } from 'fs';
import { join, extname } from 'path';
import { fileURLToPath, pathToFileURL } from 'url';
import { dirname } from 'path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const ROOT = join(__dirname, '..');
const JS_DIR = join(ROOT, 'static', 'js');
const LOCALE_FILE = join(ROOT, 'static', 'locales', 'en.js');
const BASELINE_FILE = join(__dirname, 'i18n-orphan-baseline.txt');

/**
 * @param {string} dir
 */
const SKIP_DIRS = new Set(['dist', 'vendor']);

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

const { default: catalog } = await import(pathToFileURL(LOCALE_FILE).href);
const catalogKeys = new Set(Object.keys(catalog));

const T_RE = /\bt\(\s*['"]([^'"]+)['"]/g;
// Keys also travel as bare literals the receiving component resolves, such as
// `titleKey="diag.storage.title"` and entries in config tables.
const LITERAL_KEY_RE = /['"`]([a-z][a-z0-9_]*(?:\.[a-z0-9_]+)+)['"`]/gi;
// Keys assembled at runtime. The prefix is all a scanner can see, so every
// catalog key beneath one counts as reachable.
const DIRECT_TEMPLATE_RE = /\bt\(\s*`([^`$]*)\$\{/g;
const INDIRECT_TEMPLATE_RE = /`([a-z][a-z0-9_.]*\.)\$\{/gi;

const allFiles = walk(JS_DIR);

/** @type {Map<string, string[]>} key → [file, ...] */
const missing = new Map();
/** @type {Set<string>} */
const referenced = new Set();
/** @type {Map<string, Set<string>>} prefix → files */
const dynamicPrefixes = new Map();

for (const file of allFiles) {
  const src = readFileSync(file, 'utf8');
  const rel = file.replace(ROOT + '/', '');
  for (const m of src.matchAll(T_RE)) {
    const key = m[1];
    referenced.add(key);
    if (!catalogKeys.has(key)) {
      if (!missing.has(key)) missing.set(key, []);
      missing.get(key)?.push(rel);
    }
  }
  for (const m of src.matchAll(LITERAL_KEY_RE)) referenced.add(m[1]);
  for (const re of [DIRECT_TEMPLATE_RE, INDIRECT_TEMPLATE_RE]) {
    for (const m of src.matchAll(re)) {
      if (!m[1]) continue;
      if (!dynamicPrefixes.has(m[1])) dynamicPrefixes.set(m[1], new Set());
      dynamicPrefixes.get(m[1])?.add(rel);
    }
  }
}

{
  const idx = join(ROOT, 'static/js/pages/settings/index.js');
  const src = readFileSync(idx, 'utf8');
  const ids = new Set([...src.matchAll(/\{\s*id:\s*'([a-z0-9-]+)'/g)].map((m) => m[1]));
  for (const id of ids) {
    for (const suffix of ['label', 'desc']) {
      const key = `settings.section.${id.replace(/-/g, '_')}.${suffix}`;
      referenced.add(key);
      if (!catalogKeys.has(key)) {
        if (!missing.has(key)) missing.set(key, []);
        missing.get(key)?.push('static/js/pages/settings/index.js (settings section)');
      }
    }
  }
}

const rawCatalog = readFileSync(LOCALE_FILE, 'utf8');
/** @type {Map<string, number>} */
const firstLine = new Map();
/** @type {{ key: string, first: number, second: number }[]} */
const duplicates = [];
for (const m of rawCatalog.matchAll(/^\s*'([^']+)'\s*:/gm)) {
  const line = rawCatalog.slice(0, m.index).split('\n').length;
  if (firstLine.has(m[1])) {
    duplicates.push({ key: m[1], first: firstLine.get(m[1]) ?? 0, second: line });
  } else {
    firstLine.set(m[1], line);
  }
}

const prefixes = [...dynamicPrefixes.keys()];
const orphaned = [...catalogKeys]
  .filter((key) => !referenced.has(key))
  .filter((key) => !prefixes.some((prefix) => key.startsWith(prefix)))
  .sort();

const baseline = new Set(
  readFileSync(BASELINE_FILE, 'utf8')
    .split('\n')
    .map((line) => line.replace(/#.*$/, '').trim())
    .filter(Boolean),
);
const newOrphans = orphaned.filter((key) => !baseline.has(key));
const staleBaseline = [...baseline].filter((key) => !orphaned.includes(key)).sort();

let failed = false;

if (missing.size > 0) {
  failed = true;
  console.error(`${missing.size} i18n key(s) referenced but not in catalog:`);
  for (const [key, files] of missing) {
    console.error(`  "${key}"  ->  ${[...new Set(files)].join(', ')}`);
  }
}

if (duplicates.length > 0) {
  failed = true;
  console.error(`\n${duplicates.length} duplicate catalog key(s) — the later one silently wins:`);
  for (const d of duplicates) {
    console.error(`  "${d.key}"  ->  defined at lines ${d.first} and ${d.second}`);
  }
}

if (newOrphans.length > 0) {
  failed = true;
  console.error(`\n${newOrphans.length} catalog key(s) nothing references:`);
  for (const key of newOrphans) console.error(`  "${key}"`);
  console.error('\nRemove the key, reference it, or if it is built at runtime from a prefix');
  console.error('this scanner cannot see, add that call site to DYNAMIC_PREFIX_NOTES.');
}

if (staleBaseline.length > 0) {
  failed = true;
  console.error(`\n${staleBaseline.length} baseline entr(y/ies) no longer orphaned — delete from`);
  console.error(`${BASELINE_FILE.replace(ROOT + '/', '')}:`);
  for (const key of staleBaseline) console.error(`  "${key}"`);
}

if (failed) process.exit(1);

console.log(`i18n catalog and call sites agree.`);
console.log(`  ${catalogKeys.size} keys, ${prefixes.length} runtime-built prefixes,`);
console.log(`  ${baseline.size} known orphans awaiting removal.`);
