#!/usr/bin/env node

import { readFileSync, readdirSync, statSync } from 'fs';
import { join, extname, relative } from 'path';

const ROOT = join(import.meta.dirname, '..');
const JS_DIR = join(ROOT, 'static', 'js');

const EXEMPT_FILES = new Set([
  'pages/admin/ui-showcase.js',
]);

/** @param {string} dir @returns {string[]} */
function walk(dir) {
  /** @type {string[]} */
  const files = [];
  for (const entry of readdirSync(dir)) {
    if (entry === 'dist' || entry === 'vendor') continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      files.push(...walk(full));
    } else if (extname(full) === '.js') {
      files.push(full);
    }
  }
  return files;
}

/**
 * A string is "suspicious" if, once trimmed, it contains a letter and isn't
 * one of the deliberately-exempt shapes: pure punctuation/symbols, a single
 * short token that reads as a technical label (all-caps acronym, no spaces),
 * or something that looks like a CSS class list / URL / template
 * placeholder rather than prose.
 * @param {string} text
 */
function isSuspicious(text) {
  const trimmed = text.trim();
  if (!trimmed) return false;
  if (!/[a-zA-Z]/.test(trimmed)) return false;
  if (/^__EXPR\d+__$/.test(trimmed)) return false;
  // Bare technical tokens: single all-caps word (DEBUG, OK-as-acronym-ish),
  // or a single word with no spaces that's 3 chars or fewer (icons, units).
  if (/^[A-Z0-9_-]{2,12}$/.test(trimmed) && !trimmed.includes(' ')) return false;
  // Single-character initials (avatar/logo placeholders, aria-hidden) — can't be wrapped in t()
  // without becoming a translated single letter, and can't carry a trailing `// i18n-ignore`
  // since they live inside a multi-line HTML template literal where a `//` would render as text.
  if (trimmed.length === 1) return false;
  // A lowercase-`v` version prefix, e.g. `v${version}` — formatting
  // convention, not prose.
  if (/^v$/.test(trimmed)) return false;
  // Product name — deliberately untranslated everywhere it appears.
  if (trimmed === 'Kani') return false;
  // Looks like a URL, path, or CSS custom property / class token.
  if (/^[./#-]|^https?:|^var\(--/.test(trimmed)) return false;
  // htm doesn't support JSX-style `{/* comment */}` children, but the
  // convention shows up anyway as literal text — not user-visible copy.
  if (/^\{\/\*.*\*\/\}$/.test(trimmed)) return false;
  return true;
}

/**
 * Extracts the raw contents of every `html\`...\`` tagged template literal
 * and every `.innerHTML = \`...\`` template-literal assignment (the vanilla-
 * DOM equivalent — same markup-in-a-backtick shape), tracking `${...}`
 * nesting depth so a literal backtick inside an expression doesn't
 * prematurely close the outer template.
 * @param {string} src
 * @returns {string[]}
 */
function extractHtmTemplates(src) {
  /** @type {string[]} */
  const templates = [];
  const startRe = /\bhtml`|\.innerHTML\s*=\s*`/g;
  let m;
  while ((m = startRe.exec(src))) {
    let i = m.index + m[0].length;
    let braceDepth = 0;
    let out = '';
    while (i < src.length) {
      const ch = src[i];
      if (ch === '\\') { out += ch + (src[i + 1] ?? ''); i += 2; continue; }
      if (ch === '$' && src[i + 1] === '{') { braceDepth++; out += '__EXPR__'; i += 2; continue; }
      if (braceDepth > 0) {
        if (ch === '{') braceDepth++;
        else if (ch === '}') braceDepth--;
        i++;
        continue;
      }
      if (ch === '`') { i++; break; }
      out += ch;
      i++;
    }
    templates.push(out);
  }
  return templates;
}

/**
 * Finds text nodes (content between `>` and the next `<` or end-of-string)
 * in a stripped-down htm template body.
 * @param {string} body
 * @returns {string[]}
 */
function findTextNodes(body) {
  const nodes = [];
  // Only look at text that follows a tag close `>` and precedes the next
  // `<` — this skips attribute values, which sit between `="` and `"`.
  const re = />([^<>]+)(?=<|$)/g;
  let m;
  while ((m = re.exec(body))) {
    nodes.push(m[1].replace(/__EXPR__/g, ' '));
  }
  return nodes;
}

const T_CALL_RE = /\bt\(/;

/** @type {Array<{ file: string, line: number, snippet: string }>} */
const findings = [];

for (const file of walk(JS_DIR)) {
  const relPath = relative(JS_DIR, file);
  if (EXEMPT_FILES.has(relPath)) continue;
  const src = readFileSync(file, 'utf8');
  const lines = src.split('\n');

  // Raw .textContent = 'literal' / "literal" assignments (not t(...) calls).
  const textContentRe = /\.textContent\s*=\s*(['"])((?:(?!\1)[^\\]|\\.)*)\1/g;
  let tm;
  while ((tm = textContentRe.exec(src))) {
    const value = tm[2];
    if (!isSuspicious(value)) continue;
    const upTo = src.slice(0, tm.index);
    const lineNo = upTo.split('\n').length;
    if ((lines[lineNo - 1] ?? '').includes('i18n-ignore')) continue;
    findings.push({ file: relPath, line: lineNo, snippet: `.textContent = '${value}'` });
  }

  // Bare text nodes inside htm`` templates.
  let searchCursor = 0;
  for (const template of extractHtmTemplates(src)) {
    for (const node of findTextNodes(template)) {
      if (!isSuspicious(node)) continue;
      if (T_CALL_RE.test(node)) continue;
      // Locate an approximate line by searching forward from the last match,
      // so repeated identical text nodes get distinct line numbers.
      const needle = node.trim().slice(0, 40);
      const idx = needle ? src.indexOf(needle, searchCursor) : -1;
      const lineNo = idx >= 0 ? src.slice(0, idx).split('\n').length : 0;
      if (idx >= 0) searchCursor = idx + needle.length;
      if (lineNo && (lines[lineNo - 1] ?? '').includes('i18n-ignore')) continue;
      findings.push({ file: relPath, line: lineNo, snippet: node.trim().slice(0, 60) });
    }
  }
}

if (findings.length === 0) {
  console.log('No untranslated string literals found.');
  process.exit(0);
} else {
  console.error(`${findings.length} possibly-untranslated string(s):`);
  for (const f of findings) {
    console.error(`  ${f.file}:${f.line}  ${JSON.stringify(f.snippet)}`);
  }
  console.error('\nWrap in t("key") or add `// i18n-ignore` to the line if deliberate.');
  process.exit(1);
}
