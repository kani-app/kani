#!/usr/bin/env node

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { basename, extname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const MAX_COLUMNS = 100;
const MAX_PROSE_LINES = 3;
const EXTENSIONS = new Set([
  '.css', '.html', '.js', '.md', '.mjs', '.proto', '.py', '.rs', '.sh', '.sql', '.svg', '.toml',
  '.wit', '.yaml', '.yml',
]);
const BASENAMES = new Set([
  'Dockerfile', '.dockerignore', '.gitattributes', '.gitignore', '.markdownlintignore',
  'pre-commit', 'pre-push',
]);
const SKIP = /^(?:static\/js\/(?:dist|vendor)\/|static\/css\/main\.css$|wasm_sources\/|target\/|\.sqlx\/)|(?:Cargo\.lock|\.snap)$/;
const SKIP_DIRS = new Set(['.git', '.codegraph', 'node_modules', 'target', 'site']);
const DIRECTIVE = /(?:@ts-|audit-ignore|i18n-ignore|@generated|eslint-|noinspection|language=|rustfmt::skip)/;
const NEEDS_JUSTIFICATION = /(?:^|\s)(?:@ts-ignore|audit-ignore)(?!-file)\s*$/;
const DIRECTIVE_WITH_PROSE = /^@ts-check\s+\S/;
const DEBT = /\b(?:TODO|FIXME|HACK|XXX)\b/;
const ISSUE = /(?:#\d+|https:\/\/github\.com\/[^/]+\/[^/]+\/issues\/\d+)/;
const DIVIDER = /^(?:[-=─═]{3,}|[-=─═]{2,}\s.*\s[-=─═]{2,})$/;
const PLANNING_LABEL = /^(?:Groups? [A-Z](?:\d+)?\b|[A-Z]\d+(?:\/[A-Z]?\d+)*\s*[—-])|\b(?:[Ww]ave|[Pp]lan|[Pp]hase) \d+\b/;
const DURABLE_KNOWLEDGE = /\b(?:historically|before (?:this (?:change|fix|refactor|implementation)|the (?:fix|change|refactor))|bug this replaced|caught\s+\w+\s+times|benchmark(?:ed)?|p\d{2})\b|\bthe old (?:implementation|path|layout|behaviou?r|bridge|interpreter|workflow)\b|\bpreviously\s+(?:un\w+|only|learned|had|too|could|auto\w*)\b|\bused to\s+(?:be|abort|do|answer|grow|sit|only|treat|drop|read)\b/i;

function lineNumber(source, offset) {
  let line = 1;
  for (let i = 0; i < offset; i += 1) if (source[i] === '\n') line += 1;
  return line;
}

function commentLines(source, start, end, marker, doc) {
  const lines = source.split('\n');
  const first = lineNumber(source, start);
  const last = lineNumber(source, Math.max(start, end - 1));
  const fragments = source.slice(start, end).split('\n');
  const startColumn = start - (source.lastIndexOf('\n', start - 1) + 1);
  const lastLineStart = source.lastIndexOf('\n', Math.max(start, end - 1)) + 1;
  const endColumn = end - lastLineStart;
  const out = [];
  for (let number = first; number <= last; number += 1) {
    const raw = lines[number - 1] ?? '';
    const index = number - first;
    let text = fragments[index] ?? '';
    if (index === 0) text = text.slice(marker.length);
    if (index === fragments.length - 1) text = text.replace(/(?:\*\/|-->)$/, '');
    if (index > 0) text = text.replace(/^\s*\*?\s?/, '');
    const prefixClear = index > 0 || raw.slice(0, startColumn).trim() === '';
    const suffixClear = index < fragments.length - 1 || raw.slice(endColumn).trim() === '';
    out.push({ number, raw, text, doc, fullLine: prefixClear && suffixClear });
  }
  return out;
}

function scanCStyle(source, mode) {
  const comments = [];
  for (let i = 0; i < source.length;) {
    const c = source[i];
    const next = source[i + 1];
    if (mode === 'rust' && c === 'r') {
      const raw = source.slice(i).match(/^r(#+)?"/);
      if (raw) {
        const hashes = raw[1] ?? '';
        const close = `"${hashes}`;
        const at = source.indexOf(close, i + raw[0].length);
        i = at === -1 ? source.length : at + close.length;
        continue;
      }
    }
    const rustChar = mode === 'rust' && c === "'" && /^'(?:\\.|[^'\\])'/.test(source.slice(i));
    if (c === '"' || rustChar || (mode !== 'rust' && c === "'") || (mode === 'js' && c === '`')) {
      const quote = c;
      i += 1;
      while (i < source.length) {
        if (source[i] === '\\') i += 2;
        else if (source[i] === quote) { i += 1; break; }
        else i += 1;
      }
      continue;
    }
    if (mode === 'js' && c === '/' && next !== '/' && next !== '*') {
      const previous = source.slice(0, i).match(/\S(?=\s*$)/)?.[0];
      if (!previous || /[([{:;,=!?&|]/.test(previous)) {
        i += 1;
        while (i < source.length) {
          if (source[i] === '\\') i += 2;
          else if (source[i] === '/') {
            i += 1;
            while (/[a-z]/i.test(source[i] ?? '')) i += 1;
            break;
          } else i += 1;
        }
        continue;
      }
    }
    if (c === '/' && next === '/') {
      const start = i;
      const end = source.indexOf('\n', i + 2);
      const stop = end === -1 ? source.length : end;
      const marker = source.startsWith('///', i) || source.startsWith('//!', i)
        ? source.slice(i, i + 3)
        : '//';
      comments.push(commentLines(source, start, stop, marker, marker.length === 3));
      i = stop;
      continue;
    }
    if (c === '/' && next === '*') {
      const start = i;
      const at = source.indexOf('*/', i + 2);
      const stop = at === -1 ? source.length : at + 2;
      comments.push(commentLines(source, start, stop, '/*', source.startsWith('/**', i)));
      i = stop;
      continue;
    }
    i += 1;
  }
  return comments;
}

function scanSql(source) {
  const comments = [];
  for (let i = 0; i < source.length;) {
    const c = source[i];
    if (c === "'" || c === '"' || c === '[') {
      const close = c === '[' ? ']' : c;
      i += 1;
      while (i < source.length) {
        if (source[i] === close && source[i + 1] === close) i += 2;
        else if (source[i] === close) { i += 1; break; }
        else i += 1;
      }
      continue;
    }
    if (c === '-' && source[i + 1] === '-') {
      const end = source.indexOf('\n', i + 2);
      const stop = end === -1 ? source.length : end;
      comments.push(commentLines(source, i, stop, '--', false));
      i = stop;
      continue;
    }
    if (c === '/' && source[i + 1] === '*') {
      const at = source.indexOf('*/', i + 2);
      const stop = at === -1 ? source.length : at + 2;
      comments.push(commentLines(source, i, stop, '/*', false));
      i = stop;
      continue;
    }
    i += 1;
  }
  return comments;
}

function scanHash(source) {
  return source.split('\n').flatMap((raw, index) => {
    if (/^\s*#!/.test(raw)) return [];
    let quote = null;
    for (let i = 0; i < raw.length; i += 1) {
      if (quote) {
        if (raw[i] === '\\') i += 1;
        else if (raw[i] === quote) quote = null;
      } else if (raw[i] === '"' || raw[i] === "'") quote = raw[i];
      else if (raw[i] === '#') {
        return [[{ number: index + 1, raw, text: raw.slice(i + 1), doc: false, fullLine: /^\s*#/.test(raw) }]];
      }
    }
    return [];
  });
}

function scanHtml(source) {
  const comments = [];
  for (let i = 0; i < source.length;) {
    const start = source.indexOf('<!--', i);
    if (start === -1) break;
    const at = source.indexOf('-->', start + 4);
    const stop = at === -1 ? source.length : at + 3;
    comments.push(commentLines(source, start, stop, '<!--', false));
    i = stop;
  }
  return comments;
}

function groupsFor(path, source) {
  const ext = extname(path);
  let groups;
  if (ext === '.sql') groups = scanSql(source);
  if (ext === '.toml' || ext === '.yaml' || ext === '.yml' || ext === '.sh' || ext === '.py' || BASENAMES.has(basename(path))) {
    groups = scanHash(source);
  }
  if (ext === '.html' || ext === '.svg') groups = [...scanCStyle(source, 'js'), ...scanHtml(source)];
  if (ext === '.md') groups = scanHtml(source);
  const cStyleSource = ext === '.js' || ext === '.mjs' ? source.replaceAll('<//>', '<##>') : source;
  groups ??= scanCStyle(cStyleSource, ext === '.rs' || ext === '.wit' ? 'rust' : 'js');
  const merged = [];
  for (const group of groups.sort((a, b) => a[0].number - b[0].number)) {
    const previous = merged.at(-1);
    if (
      previous
      && previous.at(-1).number + 1 === group[0].number
      && previous.at(-1).fullLine
      && group[0].fullLine
      && previous[0].doc === group[0].doc
    ) previous.push(...group);
    else merged.push([...group]);
  }
  return merged;
}

export function checkSource(path, source) {
  if (source.slice(0, 512).includes('@generated')) return [];
  const violations = [];
  const groups = groupsFor(path, source);
  for (const group of groups) {
    const prose = group.filter((line) => line.fullLine && !line.doc && !DIRECTIVE.test(line.text));
    if (prose.length > MAX_PROSE_LINES) {
      violations.push({ line: prose[0].number, rule: 'prose-block', detail: `${prose.length} lines` });
    }
    for (const line of group) {
      const text = line.text.trim();
      if (DIVIDER.test(text)) violations.push({ line: line.number, rule: 'divider', detail: text });
      if (PLANNING_LABEL.test(text)) {
        violations.push({ line: line.number, rule: 'planning-label', detail: text });
      }
      if (DEBT.test(text) && !ISSUE.test(text)) {
        violations.push({ line: line.number, rule: 'debt-marker', detail: text });
      }
      if (NEEDS_JUSTIFICATION.test(text)) {
        violations.push({ line: line.number, rule: 'directive-justification', detail: text });
      }
      if (DIRECTIVE_WITH_PROSE.test(text)) {
        violations.push({ line: line.number, rule: 'directive-prose', detail: text });
      }
      if (DURABLE_KNOWLEDGE.test(text)) {
        violations.push({ line: line.number, rule: 'durable-knowledge', detail: text });
      }
      if (line.fullLine && !line.doc && line.raw.length > MAX_COLUMNS && !DIRECTIVE.test(text) && !/https?:\/\//.test(text)) {
        violations.push({ line: line.number, rule: 'line-length', detail: `${line.raw.length} columns` });
      }
    }
  }
  return violations;
}

function files(dir = ROOT, out = []) {
  for (const name of readdirSync(dir).sort()) {
    if (SKIP_DIRS.has(name)) continue;
    const full = join(dir, name);
    if (statSync(full).isDirectory()) files(full, out);
    else {
      const path = relative(ROOT, full).replaceAll('\\', '/');
      if (!SKIP.test(path) && (EXTENSIONS.has(extname(path)) || BASENAMES.has(basename(path)))) {
        out.push(path);
      }
    }
  }
  return out;
}

export function inventorySource(path, source) {
  if (source.slice(0, 512).includes('@generated')) return [];
  return groupsFor(path, source).flatMap((group) => group.map((line) => ({
    line: line.number,
    text: line.text.trim(),
    doc: line.doc,
    directive: DIRECTIVE.test(line.text),
    frozen: path.startsWith('migrations/'),
    fullLine: line.fullLine,
  })));
}

if (!process.env.NODE_TEST_CONTEXT && process.argv[1] === fileURLToPath(import.meta.url)) {
  if (process.argv.includes('--inventory')) {
    const totals = { files: 0, comments: 0, lines: 0, doc: 0, directive: 0, frozen: 0, inline: 0, ordinary: 0 };
    for (const path of files()) {
      const source = readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
      const inventory = inventorySource(path, source);
      if (!inventory.length) continue;
      totals.files += 1;
      totals.lines += inventory.length;
      totals.comments += groupsFor(path, source).length;
      for (const item of inventory) {
        if (item.frozen) totals.frozen += 1;
        else if (item.doc) totals.doc += 1;
        else if (item.directive) totals.directive += 1;
        else if (!item.fullLine) totals.inline += 1;
        else totals.ordinary += 1;
      }
    }
    console.log(JSON.stringify(totals, null, 2));
    process.exit(0);
  }
  if (process.argv.includes('--list-comments')) {
    for (const path of files()) {
      const source = readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
      for (const item of inventorySource(path, source)) {
        const category = item.frozen ? 'frozen' : item.doc ? 'doc' : item.directive ? 'directive' : item.fullLine ? 'ordinary' : 'inline';
        console.log(`${path}:${item.line}\t${category}\t${item.text}`);
      }
    }
    process.exit(0);
  }
  const violations = [];
  for (const path of files()) {
    const source = readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
    for (const violation of checkSource(path, source)) violations.push({ path, ...violation });
  }
  for (const item of violations) {
    console.error(`${item.path}:${item.line}: ${item.rule}: ${item.detail}`);
  }
  if (violations.length) {
    console.error(`\n${violations.length} comment-style violation(s)`);
    process.exit(1);
  }
  console.log('Comment style check passed.');
}
