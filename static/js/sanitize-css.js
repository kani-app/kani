
/** At-rules a theme may keep. Everything else is dropped. */
const ALLOWED_AT_RULES = ['@media', '@supports', '@keyframes'];

/**
 * Fragments that make a declaration value unsafe regardless of context:
 * anything that can fetch, execute, or escape the declaration it sits in.
 */
const BANNED_VALUE_FRAGMENTS = [
  'url(',
  'image-set(',
  'expression(',
  'behavior',
  '-moz-binding',
  'javascript:',
  '</',
  '@import',
];

/** @param {string} input */
function stripComments(input) {
  let out = '';
  let i = 0;
  while (i < input.length) {
    if (input[i] === '/' && input[i + 1] === '*') {
      i += 2;
      while (i + 1 < input.length && !(input[i] === '*' && input[i + 1] === '/')) i += 1;
      i = Math.min(i + 2, input.length);
    } else {
      out += input[i];
      i += 1;
    }
  }
  return out;
}

/**
 * @typedef {{ kind: 'at', name: string, raw: string }
 *   | { kind: 'rule', selector: string, body: string }
 *   | { kind: 'junk', text: string }} Block
 */

/**
 * Split into top-level blocks, tracking brace depth so a nested rule (inside
 * `@media`) is not mistaken for a top-level one.
 * @param {string} input
 * @returns {Block[]}
 */
function splitTopLevel(input) {
  /** @type {Block[]} */
  const blocks = [];
  let head = '';
  let body = '';
  let depth = 0;

  for (let i = 0; i < input.length; i += 1) {
    const c = input[i];
    if (depth === 0) {
      if (c === '{') {
        depth = 1;
        body = '';
      } else if (c === ';') {
        // A statement at-rule (`@import url(x);`) has no block. Without this
        // case `head` keeps accumulating to the next `{`, so stripping the
        // at-rule also deletes the rule that followed it.
        const stmt = head.trim();
        if (stmt) {
          const name = stmt.split(/\s+/)[0] ?? '';
          if (name.startsWith('@')) blocks.push({ kind: 'at', name, raw: `${stmt};` });
          else blocks.push({ kind: 'junk', text: stmt });
        }
        head = '';
      } else {
        head += c;
      }
    } else {
      if (c === '{') depth += 1;
      else if (c === '}') {
        depth -= 1;
        if (depth === 0) {
          const selector = head.trim();
          const name = selector.split(/\s+/)[0] ?? '';
          if (name.startsWith('@')) {
            blocks.push({ kind: 'at', name, raw: `${selector} {${body}}` });
          } else if (!selector) {
            blocks.push({ kind: 'junk', text: body });
          } else {
            blocks.push({ kind: 'rule', selector, body });
          }
          head = '';
          body = '';
          continue;
        }
      }
      body += c;
    }
  }
  if (head.trim()) blocks.push({ kind: 'junk', text: head });
  return blocks;
}

/**
 * @param {string} body
 * @returns {{ css: string, stripped: string[] }}
 */
function sanitizeDeclarations(body) {
  let out = '';
  /** @type {string[]} */
  const stripped = [];
  for (const raw of body.split(';')) {
    const decl = raw.trim();
    if (!decl) continue;
    const idx = decl.indexOf(':');
    if (idx === -1) {
      stripped.push(`malformed declaration \`${decl}\``);
      continue;
    }
    const prop = decl.slice(0, idx).trim();
    const value = decl.slice(idx + 1).trim();
    const lower = value.toLowerCase();
    if (BANNED_VALUE_FRAGMENTS.some((f) => lower.includes(f))) {
      stripped.push(`declaration \`${prop}\``);
      continue;
    }
    out += `\n  ${prop}: ${value};`;
  }
  if (out) out += '\n';
  return { css: out, stripped };
}

/**
 * Confine every selector to the themed document state, so shared CSS cannot
 * restyle anything outside a page that has opted in.
 * @param {string} selector
 */
function scopeSelector(selector) {
  return selector
    .split(',')
    .map((s) => {
      const t = s.trim();
      return t.startsWith(':root')
        ? t.replace(':root', ':root[data-kani-theme]')
        : `[data-kani-theme] ${t}`;
    })
    .join(', ');
}

/**
 * Strip everything a theme has no business shipping, and scope what remains.
 * Mirrors the server; the server's result is what actually gets stored.
 * @param {string} input
 * @returns {{ css: string, stripped: string[] }}
 */
export function sanitizeCss(input) {
  /** @type {string[]} */
  const stripped = [];
  let out = '';

  for (const block of splitTopLevel(stripComments(input ?? ''))) {
    if (block.kind === 'at') {
      if (ALLOWED_AT_RULES.includes(block.name.toLowerCase())) {
        out += `${block.raw}\n`;
      } else {
        stripped.push(`at-rule ${block.name}`);
      }
    } else if (block.kind === 'rule') {
      const { css, stripped: removed } = sanitizeDeclarations(block.body);
      stripped.push(...removed);
      if (!css.trim()) continue;
      out += `${scopeSelector(block.selector)} {${css}}\n`;
    } else if (block.text.trim()) {
      stripped.push('unparseable input');
    }
  }

  return { css: out.trim(), stripped };
}
