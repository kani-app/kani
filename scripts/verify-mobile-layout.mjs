// @ts-check
/**
 * Mobile layout rules, checked against a running instance.
 *
 * `docs/docs/developer/design-system.md` states what a narrow viewport must
 * honour — a touch-target floor, tables that scroll inside a wrapper rather
 * than widening the page, content that clears the bottom nav. Those are
 * rendered-geometry properties: they cannot be checked from source, and the
 * 2026-09 mobile audit found every one of them broken somewhere.
 *
 * As with the permission matrix, the expectations are not written down here.
 * The touch floor is read out of the design-system document and the route list
 * out of `static/js/router.js`, so a rule that changes changes what this
 * asserts, and a new route is covered the day it is added.
 *
 * Usage:
 *   node scripts/verify-mobile-layout.mjs <base-url> <admin-user> <admin-pass>
 *
 * Options:
 *   --widths=320,390       viewport widths to check (default 320,390,412)
 *   --only=library,jobs    limit to named routes
 *   --min-gutter=8         required gap between content and the bottom nav
 *   --verbose              list every offending element, not just the first few
 *
 * `--min-gutter` defaults to 0, which only catches content actually running
 * under the nav. The design system states no bottom gutter value, so a larger
 * figure is a judgement this cannot make for you; pass one once it is decided.
 *
 * Set `KANI_ROOT` if you run it from outside the repository (for instance from
 * a scratch directory that has Playwright installed).
 *
 * Needs Playwright (`npm i playwright` in a scratch directory is fine) and an
 * instance you may sign in to. It only reads, but it drives every route at
 * several widths, so point it at a throwaway instance and raise
 * `KANI_API_RATE_PER_SECOND` / `KANI_API_BURST_SIZE` to keep the run quick.
 *
 * Local only, like the permission matrix. It needs a live server and a
 * password, so it is not wired into CI.
 */

import { chromium, devices } from 'playwright';
import { readFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

const ROOT = process.env.KANI_ROOT ?? join(dirname(fileURLToPath(import.meta.url)), '..');
const [BASE = 'http://127.0.0.1:8299', ADMIN = 'admin', ADMIN_PW = ''] = process.argv
  .slice(2).filter((a) => !a.startsWith('--'));
const flag = (n) => (process.argv.find((a) => a.startsWith(`--${n}=`)) || '').split('=')[1];
const VERBOSE = process.argv.includes('--verbose');
const WIDTHS = (flag('widths') ?? '320,390,412').split(',').map(Number);
const ONLY = flag('only')?.split(',').filter(Boolean);
const MIN_GUTTER = Number(flag('min-gutter') ?? 0);

/** The touch floor, read from the design system rather than repeated here. */
function touchFloor() {
  const doc = readFileSync(join(ROOT, 'docs/docs/developer/design-system.md'), 'utf8');
  const m = doc.match(/interactive controls reach\s*[≥>=]+\s*(\d+)\s*px/i);
  if (!m) throw new Error('design-system.md no longer states a touch-target floor');
  return Number(m[1]);
}

/** Routes, straight out of the SPA router, with parameters left as markers. */
function routes() {
  const src = readFileSync(join(ROOT, 'static/js/router.js'), 'utf8');
  const out = [];
  for (const m of src.matchAll(/path:\s*'([^']+)'/g)) out.push(m[1]);
  if (!out.length) throw new Error('could not read routes out of router.js');
  return out;
}

/** Fill `:params` from real ids so parameterised routes are actually exercised. */
async function resolveRoutes(ctx) {
  const ids = { manga: null, source: null, chapter: null };
  const get = async (p) => {
    const r = await ctx.request.get(`${BASE}${p}`);
    return r.ok() ? r.json() : null;
  };
  const lib = await get('/rest/library?page=1');
  if (lib?.items?.length) ids.manga = lib.items[0].id;
  // A manga with no chapters resolves no chapter id, which skips /reader/:id
  // while the run still reports every check passed. Prefer one that has them.
  for (const item of (lib?.items ?? []).slice(0, 8)) {
    const c = await get(`/rest/manga/${item.id}/chapters?page=1`);
    const l = Array.isArray(c) ? c : c?.chapters ?? c?.items;
    if (l?.length) { ids.manga = item.id; ids.chapter = l[0].id; break; }
  }
  const srcs = await get('/rest/sources');
  if (Array.isArray(srcs) && srcs.length) ids.source = srcs[0].id;


  const skipped = [];
  const resolved = [];
  for (const path of routes()) {
    if (path === '/admin/ui-showcase') continue;
    const slug = path === '/' ? 'library' : path.replace(/^\//, '').replace(/[/:]/g, '-');
    if (ONLY && !ONLY.includes(slug)) continue;
    let url = path;
    if (path.includes(':')) {
      url = path
        .replace(/:db_id|:manga_id/g, String(ids.manga ?? ''))
        .replace(/:id/g, String(path.startsWith('/reader') ? ids.chapter ?? '' : ids.source ?? ''));
      if (url.includes('//') || /:/.test(url.slice(1)) || url.endsWith('/')) {
        skipped.push(`${slug} (no id available)`);
        continue;
      }
    }
    resolved.push({ slug, url });
  }
  return { resolved, skipped };
}

/**
 * Runs in the page. Settles every scroller, then reports the rendered facts the
 * rules are about. Deliberately returns measurements, not verdicts.
 */
/* eslint-disable no-undef */
async function measure(floor) {
  const settle = async () => {
    const scrollers = () => [...document.querySelectorAll('body *')].filter((e) => {
      const cs = getComputedStyle(e);
      return /(auto|scroll)/.test(cs.overflowY) && e.scrollHeight > e.clientHeight + 2;
    });
    let prev = '';
    for (let i = 0; i < 25; i++) {
      scrollers().forEach((s) => { s.scrollTop = s.scrollHeight; });
      await new Promise((r) => setTimeout(r, 350));
      const now = scrollers().map((s) => `${s.scrollHeight}:${Math.round(s.scrollTop)}`).join('|');
      if (now === prev && i > 2) break;
      prev = now;
    }
    await new Promise((r) => setTimeout(r, 300));
  };

  // Content in a closed <details>, a box clipped by an overflow:hidden
  // ancestor, and a drawer translated off-canvas all keep a live rect. The
  // reader's closed menu is the drawer case.
  const offCanvas = (el) => {
    for (let a = el; a && a !== document.documentElement; a = a.parentElement) {
      const t = getComputedStyle(a).transform;
      if (t && t !== 'none') {
        const m = new DOMMatrixReadOnly(t);
        if (Math.abs(m.m41) > 1 || Math.abs(m.m42) > 1) return true;
      }
    }
    return false;
  };
  const shown = (el) => {
    const cs = getComputedStyle(el);
    if (cs.visibility === 'hidden' || cs.display === 'none' || cs.opacity === '0') return false;
    if (el.closest('details:not([open])')) return false;
    if (offCanvas(el)) return false;
    const r = el.getBoundingClientRect();
    for (let a = el.parentElement; a && a !== document.documentElement; a = a.parentElement) {
      const acs = getComputedStyle(a);
      if (acs.overflow === 'hidden' || acs.overflowY === 'hidden') {
        if (r.bottom > a.getBoundingClientRect().bottom + 1) return false;
      }
    }
    return true;
  };
  const label = (el) => (el.getAttribute('aria-label') || el.textContent || el.tagName)
    .trim().replace(/\s+/g, ' ').slice(0, 30);
  const desc = (el) => `${el.tagName.toLowerCase()}${el.id ? `#${el.id}` : ''} ${JSON.stringify(label(el))}`;

  const nav = document.querySelector('#bottom-nav');
  const navShown = nav && getComputedStyle(nav).display !== 'none';
  const navTop = navShown ? nav.getBoundingClientRect().top : null;

  const CTRL = 'a[href],button,input,select,textarea,[role="button"],[role="tab"],[role="switch"]';
  // A link sitting inside a run of prose is not a tap target in the sense the
  // rule means; a standalone one is.
  const inlineInProse = (el) => el.tagName === 'A'
    && [...(el.parentElement?.childNodes ?? [])].some((n) => n.nodeType === 3 && n.textContent.trim());
  // Taken out of the tab order and wrapped in something that is itself the
  // control — a chapter link inside a role="option" row. The row is what the
  // finger hits, so measure that instead of the label it contains.
  const deferredToAncestor = (el) => el.getAttribute('tabindex') === '-1'
    && !!el.closest('[role="option"],[role="row"],[role="menuitem"],label');
  // A control may carry the floor on an overlay rather than its own box, so a
  // checkbox stays a 1rem mark while the finger gets 2.5rem. Measuring only
  // getBoundingClientRect would report every one of those as a failure.
  const hitBox = (el, r) => {
    const a = getComputedStyle(el, '::after');
    if (!a || a.content === 'none' || a.position !== 'absolute') return r;
    const w = parseFloat(a.width);
    const h = parseFloat(a.height);
    if (!Number.isFinite(w) || !Number.isFinite(h)) return r;
    return { width: Math.max(r.width, w), height: Math.max(r.height, h) };
  };

  const small = [];
  for (const el of document.querySelectorAll(CTRL)) {
    const r = el.getBoundingClientRect();
    if (r.width === 0 || r.height === 0) continue;
    // `shown` minus the off-canvas test: a closed drawer's controls are the
    // right size or the wrong size whether it is open or shut, and measuring
    // them only when it happens to be open is how the reader went unchecked.
    const cs = getComputedStyle(el);
    if (cs.visibility === 'hidden' || cs.display === 'none' || cs.opacity === '0') continue;
    if (el.closest('details:not([open])')) continue;
    if (inlineInProse(el) || deferredToAncestor(el)) continue;
    // A visually hidden input whose styled label is the real target.
    if (r.width <= 2 && r.height <= 2) continue;
    const hit = hitBox(el, r);
    if (hit.width >= floor && hit.height >= floor) continue;
    const via = hit.width !== r.width || hit.height !== r.height
      ? ` (hit ${Math.round(hit.width)}x${Math.round(hit.height)})` : '';
    small.push(`${desc(el)} ${Math.round(r.width)}x${Math.round(r.height)}${via}`);
  }

  const de = document.documentElement;
  const clipped = [];
  for (const el of document.querySelectorAll('body *')) {
    const r = el.getBoundingClientRect();
    if (r.width < 4 || r.height < 4 || !shown(el)) continue;
    if (r.right > de.clientWidth + 1 || r.left < -1) {
      let scrollable = false;
      for (let a = el.parentElement; a && a !== document.documentElement; a = a.parentElement) {
        const cs = getComputedStyle(a);
        if (/(auto|scroll)/.test(cs.overflowX) && a.scrollWidth > a.clientWidth + 2) { scrollable = true; break; }
      }
      if (!scrollable && el.matches(CTRL)) clipped.push(`${desc(el)} right=${Math.round(r.right)}`);
    }
  }

  const before = document.body.scrollLeft;
  document.body.scrollLeft = 9999;
  const bodyScrollsX = document.body.scrollLeft > before;
  document.body.scrollLeft = before;

  const tables = [...document.querySelectorAll('table')].filter(shown).map((t) => {
    const wrap = t.closest('.overflow-x-auto');
    const fits = t.getBoundingClientRect().width <= (wrap ?? t).clientWidth + 1;
    return { wrapped: !!wrap, fits, caption: label(t).slice(0, 24) };
  });

  await settle();

  let lowest = null;
  if (navShown) {
    for (const el of document.querySelectorAll('body *')) {
      if (nav.contains(el)) continue;
      if (getComputedStyle(el).position === 'fixed') continue;
      const r = el.getBoundingClientRect();
      if (r.width < 4 || r.height < 4 || r.top > innerHeight || r.bottom < 0) continue;
      if (!shown(el)) continue;
      const own = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
      if (!own && !/^(BUTTON|A|INPUT|SELECT|TEXTAREA|IMG|CANVAS)$/.test(el.tagName)) continue;
      if (!lowest || r.bottom > lowest.bottom) lowest = { bottom: r.bottom, what: desc(el) };
    }
  }

  const emptyStates = [...document.querySelectorAll('div')]
    .filter((e) => /py-16/.test(String(e.className)) && /text-center/.test(String(e.className)) && shown(e))
    .map((e) => {
      const ps = [...e.querySelectorAll('p')].map((p) => p.getBoundingClientRect());
      return {
        left: ps.length ? Math.round(Math.min(...ps.map((r) => r.left))) : null,
        right: ps.length ? Math.round(innerWidth - Math.max(...ps.map((r) => r.right))) : null,
      };
    });

  return {
    viewport: innerWidth,
    landedOn: location.pathname,
    smallTargets: small,
    clippedControls: clipped,
    bodyScrollsX,
    tables,
    navTop: navTop === null ? null : Math.round(navTop),
    bottomGap: lowest && navTop !== null ? Math.round(navTop - lowest.bottom) : null,
    bottomWhat: lowest?.what ?? null,
    emptyStates,
  };
}
/* eslint-enable no-undef */

let failures = 0;
const report = (ok, line) => {
  if (!ok) failures++;
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${line}`);
};
const list = (items) => (VERBOSE ? items : items.slice(0, 4))
  .map((i) => `\n        ${i}`).join('') + (!VERBOSE && items.length > 4 ? `\n        … ${items.length - 4} more` : '');

const FLOOR = touchFloor();
console.log(`touch floor ${FLOOR}px (from design-system.md), widths ${WIDTHS.join(', ')}\n`);

const browser = await chromium.launch({ headless: true });
const probe = await browser.newContext();
const auth = await probe.request.post(`${BASE}/rest/auth/login`, {
  data: { username: ADMIN, password: ADMIN_PW },
});
if (!auth.ok()) {
  console.error(`sign-in failed (${auth.status()}). Pass <base-url> <admin-user> <admin-pass>.`);
  process.exit(2);
}
const info = await probe.request.get(`${BASE}/rest/system/info`);
const VERSION = info.ok() ? (await info.json()).version : '';
const { resolved, skipped } = await resolveRoutes(probe);
await probe.close();
if (skipped.length) console.log(`skipped: ${skipped.join(', ')}\n`);

for (const width of WIDTHS) {
  const ctx = await browser.newContext({
    viewport: { width, height: 883 },
    deviceScaleFactor: 2,
    isMobile: true,
    hasTouch: true,
    userAgent: devices['Galaxy S9+'].userAgent,
  });
  await ctx.request.post(`${BASE}/rest/auth/login`, { data: { username: ADMIN, password: ADMIN_PW } });
  // The changelog modal opens once per version and would otherwise sit over
  // every page, so mark this version seen. It has to be the real version.
  await ctx.addInitScript((v) => {
    try { localStorage.setItem('kani_last_seen_version', v); } catch { /* private mode */ }
  }, VERSION);

  console.log(`── ${width}px ──`);
  for (const { slug, url } of resolved) {
    const page = await ctx.newPage();
    let m;
    try {
      await page.goto(BASE + url, { waitUntil: 'domcontentloaded', timeout: 25000 });
      await page.waitForTimeout(1500);
      m = await page.evaluate(measure, FLOOR);
    } catch (e) {
      report(false, `${slug} — did not load: ${e.message.slice(0, 60)}`);
      await page.close();
      continue;
    }

    // A first run that has not been completed redirects everything to the
    // wizard, which would otherwise be reported as twenty identical failures.
    const wanted = url.split('?')[0];
    if (m.landedOn !== wanted && wanted !== '/') {
      console.log(`SKIP  ${slug} — redirected to ${m.landedOn}`);
      await page.close();
      continue;
    }

    report(!m.smallTargets.length,
      `${slug} touch targets ≥ ${FLOOR}px${m.smallTargets.length ? list(m.smallTargets) : ''}`);
    report(!m.clippedControls.length,
      `${slug} controls within viewport${m.clippedControls.length ? list(m.clippedControls) : ''}`);
    report(!m.bodyScrollsX, `${slug} page body does not scroll horizontally`);

    const badTables = m.tables.filter((t) => !t.wrapped && !t.fits);
    report(!badTables.length,
      `${slug} tables scroll inside a wrapper${badTables.length ? list(badTables.map((t) => t.caption)) : ''}`);

    if (m.bottomGap !== null) {
      report(m.bottomGap > MIN_GUTTER,
        `${slug} content clears the bottom nav by > ${MIN_GUTTER}px `
        + `(gap ${m.bottomGap}px, ${m.bottomWhat})`);
    }
    for (const es of m.emptyStates) {
      if (es.left === null) continue;
      report(es.left > 0 && es.right > 0,
        `${slug} empty state has side padding (left ${es.left}px, right ${es.right}px)`);
    }
    await page.close();
  }
  await ctx.close();
  console.log('');
}

await browser.close();
console.log(failures ? `\n${failures} failing check(s)` : '\nall checks passed');
process.exit(failures ? 1 : 0);
