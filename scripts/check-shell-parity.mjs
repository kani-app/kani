#!/usr/bin/env node
// @ts-check
/**
 * Verifies the dev and production frontend shells agree on mount points and
 * document-level `<meta>` tags.
 *
 * Components portal into elements looked up by id, and every such lookup is
 * guarded (`if (root) render(...)`), so a shell missing one produces no error —
 * the feature is simply inert.
 *
 * The metas fail even more quietly: nothing reads them, so a shell that lacks
 * `viewport-fit=cover` or carries the wrong `theme-color` looks identical in a
 * desktop browser and only misbehaves on a phone, in the build users run.
 *
 * Exits with status 1 when the two shells disagree. Note what that does and does
 * not buy: this checks agreement, not correctness. Both shells carried the same
 * viewport tag, missing `viewport-fit=cover`, and a parity check can never see
 * that — only a rule about what the tag should say would.
 */

import { readFileSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const DEV_SHELL = 'static/index.html';
const PROD_SHELL = 'static/index.prod.html';

/**
 * Ids only the dev shell is expected to carry, mapped to the reason.
 * @type {Record<string, string>}
 */
const DEV_ONLY = {};

/**
 * @param {string} html
 * @returns {Set<string>}
 */
export function elementIds(html) {
  const withoutComments = html.replace(/<!--[\s\S]*?-->/g, '');
  const ids = new Set();
  for (const [, id] of withoutComments.matchAll(/\sid=["']([^"']+)["']/g)) {
    ids.add(id);
  }
  return ids;
}

/**
 * Named `<meta>` tags, keyed by name and any `media` qualifier.
 *
 * The qualifier is part of the key because `theme-color` legitimately appears
 * twice, once per colour scheme; keying on the name alone would silently keep
 * whichever came last.
 *
 * `charset`, `http-equiv` and `property` tags carry no `name` and are skipped —
 * they are not the class of thing that drifts between the two shells.
 *
 * @param {string} html
 * @returns {Map<string, string>}
 */
export function metaTags(html) {
  const withoutComments = html.replace(/<!--[\s\S]*?-->/g, '');
  const tags = new Map();
  for (const [, attrs] of withoutComments.matchAll(/<meta\s+([^>]*?)\/?>/gi)) {
    /** @param {string} name */
    const attr = (name) => attrs.match(new RegExp(`(?:^|\\s)${name}=["']([^"']*)["']`))?.[1];
    const name = attr('name');
    if (!name) continue;
    const media = attr('media');
    tags.set(media ? `${name}@${media}` : name, attr('content') ?? '');
  }
  return tags;
}

/**
 * @param {Map<string, string>} dev
 * @param {Map<string, string>} prod
 * @returns {{ missingFromProd: string[], missingFromDev: string[], differing: string[] }}
 */
export function compareMeta(dev, prod) {
  return {
    missingFromProd: [...dev.keys()].filter(k => !prod.has(k)).sort(),
    missingFromDev: [...prod.keys()].filter(k => !dev.has(k)).sort(),
    differing: [...dev.keys()].filter(k => prod.has(k) && prod.get(k) !== dev.get(k)).sort(),
  };
}

/**
 * @param {Set<string>} dev
 * @param {Set<string>} prod
 * @returns {{ missingFromProd: string[], missingFromDev: string[] }}
 */
export function compareShells(dev, prod) {
  return {
    missingFromProd: [...dev].filter(id => !prod.has(id) && !(id in DEV_ONLY)).sort(),
    missingFromDev: [...prod].filter(id => !dev.has(id)).sort(),
  };
}

function main() {
  const devHtml = readFileSync(join(ROOT, DEV_SHELL), 'utf8');
  const prodHtml = readFileSync(join(ROOT, PROD_SHELL), 'utf8');

  const dev = elementIds(devHtml);
  const prod = elementIds(prodHtml);
  const ids = compareShells(dev, prod);

  const devMeta = metaTags(devHtml);
  const prodMeta = metaTags(prodHtml);
  const meta = compareMeta(devMeta, prodMeta);

  const failed = ids.missingFromProd.length + ids.missingFromDev.length
    + meta.missingFromProd.length + meta.missingFromDev.length + meta.differing.length;

  if (failed === 0) {
    console.log(
      `Frontend shells agree. ${dev.size} mount point(s) and ${devMeta.size} meta tag(s) in both.`,
    );
    return 0;
  }

  for (const id of ids.missingFromProd) {
    console.error(`${PROD_SHELL} is missing id="${id}" (present in ${DEV_SHELL})`);
  }
  for (const id of ids.missingFromDev) {
    console.error(`${DEV_SHELL} is missing id="${id}" (present in ${PROD_SHELL})`);
  }
  if (ids.missingFromProd.length || ids.missingFromDev.length) {
    console.error(
      '\nA lookup against a missing mount point fails silently, so whatever portals\n'
      + 'into it stops working with nothing in the console. Add the element to the\n'
      + 'shell that lacks it, or record it in DEV_ONLY with a reason.',
    );
  }

  for (const key of meta.missingFromProd) {
    console.error(`${PROD_SHELL} is missing <meta name="${key}"> (present in ${DEV_SHELL})`);
  }
  for (const key of meta.missingFromDev) {
    console.error(`${DEV_SHELL} is missing <meta name="${key}"> (present in ${PROD_SHELL})`);
  }
  for (const key of meta.differing) {
    console.error(
      `<meta name="${key}"> disagrees: `
      + `${DEV_SHELL} has "${devMeta.get(key)}", ${PROD_SHELL} has "${prodMeta.get(key)}"`,
    );
  }
  if (meta.missingFromProd.length || meta.missingFromDev.length || meta.differing.length) {
    console.error(
      '\nNothing in the app reads these, so a shell that disagrees looks correct in a\n'
      + 'desktop browser and misbehaves only on a phone — in the production build.\n'
      + 'Make both shells state the same thing.',
    );
  }
  return 1;
}

if (!process.env.NODE_TEST_CONTEXT && process.argv[1] === fileURLToPath(import.meta.url)) {
  process.exit(main());
}
