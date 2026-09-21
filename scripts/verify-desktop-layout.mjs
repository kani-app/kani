// @ts-check
/**
 * Desktop layout rules, checked against a running instance.
 *
 * The counterpart to `verify-mobile-layout.mjs`. The 2026-09 mobile work was
 * measured at 320-430px and shipped two defects at widths it never looked at:
 * the sources view switcher spanning the content area instead of heading the
 * sidebar it switches (KANI-48), and the library filter panel forcing the
 * mobile sheet at every width, which on a desktop is a full-width strip welded
 * to the bottom of the window (KANI-49). Both are rendered geometry; neither is
 * visible from the source.
 *
 * As with the mobile harness and the permission matrix, expectations are not
 * written down here. The route list comes out of `static/js/router.js`, so a
 * new route is covered the day it is added.
 *
 * Usage:
 *   node scripts/verify-desktop-layout.mjs <base-url> <admin-user> <admin-pass>
 *
 * Options:
 *   --widths=1280,1920     viewport widths to check (default 1280,1440,1920)
 *   --only=library,sources limit to named routes
 *   --verbose              list every offending element, not just the first few
 *
 * Set `KANI_ROOT` if you run it from outside the repository (for instance from
 * a scratch directory that has Playwright installed).
 *
 * Needs Playwright and an instance you may sign in to. It only reads, but it
 * drives every route at several widths, so point it at a throwaway instance and
 * raise `KANI_API_RATE_PER_SECOND` / `KANI_API_BURST_SIZE` to keep the run quick.
 *
 * Local only, like the mobile harness. It needs a live server and a password,
 * so it is not wired into CI.
 *
 * Three behaviours make a naive version of this report defects that do not exist:
 *
 * 1. **A closed drawer is translated off-canvas and still measures.** The
 *    reader's menu sits at `translateX(-288px)`, which reads as four controls
 *    clipped off the left edge. Walk the ancestors for a translation.
 * 2. **The served bundle is not the source.** A release binary serves
 *    `static/js/dist`, which `kani-web/build.rs` regenerates. Run this against
 *    an unrebuilt binary and it reports the previous build's geometry. If a
 *    failure names a class the source emits, rebuild before filing anything.
 * 3. **A first-login dialog covers the page.** Dismiss whatever is open before
 *    measuring, or every route reports the same modal's controls.
 */

import { chromium } from 'playwright';
import { readFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

const ROOT = process.env.KANI_ROOT ?? join(dirname(fileURLToPath(import.meta.url)), '..');
const [BASE = 'http://127.0.0.1:8299', ADMIN = 'admin', ADMIN_PW = ''] = process.argv
  .slice(2).filter((a) => !a.startsWith('--'));
const flag = (n) => (process.argv.find((a) => a.startsWith(`--${n}=`)) || '').split('=')[1];
const VERBOSE = process.argv.includes('--verbose');
const WIDTHS = (flag('widths') ?? '1280,1440,1920').split(',').map(Number);
const ONLY = flag('only')?.split(',').filter(Boolean);

/** The breakpoint above which the app shows its desktop chrome, per the design system. */
function sheetBreakpoint() {
  const doc = readFileSync(join(ROOT, 'docs/docs/developer/design-system.md'), 'utf8');
  if (!/centred card from `?sm:/i.test(doc)) {
    throw new Error('design-system.md no longer states the modal presentation rule');
  }
  return 640; // Tailwind's sm:
}

/** Routes, straight out of the SPA router, with parameters left as markers. */
function routes() {
  const src = readFileSync(join(ROOT, 'static/js/router.js'), 'utf8');
  const out = [];
  for (const m of src.matchAll(/path:\s*'([^']+)'/g)) out.push(m[1]);
  if (!out.length) throw new Error('could not read routes out of router.js');
  return out;
}

const UNAUTHENTICATED = ['/login', '/register', '/setup', '/forgot-password',
  '/reset-password', '/verify-email', '/onboarding'];

async function resolveRoutes(ctx) {
  const get = async (p) => { const r = await ctx.request.get(`${BASE}${p}`); return r.ok() ? r.json() : null; };
  const ids = { manga: null, source: null, chapter: null };
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

  const resolved = [];
  const skipped = [];
  for (const path of routes()) {
    if (path === '/admin/ui-showcase' || UNAUTHENTICATED.includes(path)) continue;
    const slug = path === '/' ? 'library' : path.replace(/^\//, '').replace(/[/:]/g, '-');
    if (ONLY && !ONLY.includes(slug)) continue;
    let url = path;
    if (path.includes(':')) {
      url = path.replace(/:db_id|:manga_id/g, String(ids.manga ?? ''))
        .replace(/:id/g, String(path.startsWith('/reader') ? ids.chapter ?? '' : ids.source ?? ''));
      if (url.includes('//') || /:/.test(url.slice(1)) || url.endsWith('/')) {
        skipped.push(`${slug} (no id available)`); continue;
      }
    }
    resolved.push({ slug, url });
  }
  return { resolved, skipped };
}

/** Runs in the page. Returns measurements, not verdicts. */
/* eslint-disable no-undef */
function measure() {
  // A closed slide-out drawer is translated off-canvas and still reports a
  // rect. The reader's menu is the live example, at translateX(-288px).
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
    return cs.display !== 'none' && cs.visibility !== 'hidden' && cs.opacity !== '0'
      && !el.closest('details:not([open])') && !offCanvas(el);
  };
  const label = (el) => (el.getAttribute('aria-label') || el.textContent || el.tagName)
    .trim().replace(/\s+/g, ' ').slice(0, 30);

  const de = document.documentElement;
  const CTRL = 'a[href],button,input,select,textarea,[role="button"],[role="tab"],[role="switch"]';
  const clipped = [];
  for (const el of document.querySelectorAll(CTRL)) {
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2 || !shown(el)) continue;
    if (r.right > de.clientWidth + 1 || r.left < -1) {
      let scrollable = false;
      for (let a = el.parentElement; a && a !== de; a = a.parentElement) {
        const cs = getComputedStyle(a);
        if (/(auto|scroll)/.test(cs.overflowX) && a.scrollWidth > a.clientWidth + 2) { scrollable = true; break; }
      }
      if (!scrollable) clipped.push(`${el.tagName.toLowerCase()} "${label(el)}" right=${Math.round(r.right)}`);
    }
  }

  const bottomNav = document.querySelector('#bottom-nav');
  // A tab bar that heads a list pane must be sized to it, not to the page:
  // a bar spanning the content width appears to title the detail pane.
  const pane = document.querySelector('.js-sources-view aside, .master-detail__list, .list-pane-header');
  const bar = document.querySelector('[role="tablist"]');
  const paneBar = (pane && bar && getComputedStyle(pane).display !== 'none')
    ? (() => { const p = pane.getBoundingClientRect(), b = bar.getBoundingClientRect();
        return { paneLeft: Math.round(p.left), paneRight: Math.round(p.right),
                 barLeft: Math.round(b.left), barRight: Math.round(b.right) }; })()
    : null;

  return {
    overflowsX: de.scrollWidth > de.clientWidth + 1,
    scrollWidth: de.scrollWidth, clientWidth: de.clientWidth,
    clipped,
    bottomNavShown: !!(bottomNav && getComputedStyle(bottomNav).display !== 'none'),
    paneBar,
  };
}

/** Measures whatever dialog is open against the design system's §Modals rule. */
function measureDialog() {
  const d = document.querySelector('#modal-root [role="dialog"]');
  if (!d) return null;
  const r = d.getBoundingClientRect();
  return { width: Math.round(r.width), top: Math.round(r.top), bottom: Math.round(r.bottom),
           vw: innerWidth, vh: innerHeight };
}
/* eslint-enable no-undef */

let failures = 0;
const report = (ok, line) => { if (!ok) failures++; console.log(`${ok ? 'PASS' : 'FAIL'}  ${line}`); };
const list = (items) => (VERBOSE ? items : items.slice(0, 4))
  .map((i) => `\n        ${i}`).join('') + (!VERBOSE && items.length > 4 ? `\n        … ${items.length - 4} more` : '');

const SM = sheetBreakpoint();
console.log(`desktop layout, widths ${WIDTHS.join(', ')} (modal card rule applies from ${SM}px)\n`);

const browser = await chromium.launch({ headless: true });
const probe = await browser.newContext();
const auth = await probe.request.post(`${BASE}/rest/auth/login`, { data: { username: ADMIN, password: ADMIN_PW } });
if (!auth.ok()) {
  console.error(`sign-in failed (${auth.status()}). Pass <base-url> <admin-user> <admin-pass>.`);
  process.exit(2);
}
await probe.close();

for (const width of WIDTHS) {
  const ctx = await browser.newContext({ viewport: { width, height: 900 } });
  const page = await ctx.newPage();
  await page.goto(`${BASE}/login`);
  await page.fill('input[type="text"], input[name="username"]', ADMIN);
  await page.fill('input[type="password"]', ADMIN_PW);
  await page.click('button[type="submit"]');
  await page.waitForURL((u) => !u.pathname.startsWith('/login'), { timeout: 20000 });

  // A first-login "what's new" dialog otherwise covers every route measured.
  const dismiss = async () => {
    for (let i = 0; i < 4; i++) {
      if (!(await page.locator('#modal-root').locator('[role="dialog"]').count())) break;
      await page.keyboard.press('Escape');
      await page.waitForTimeout(350);
    }
  };

  const { resolved, skipped } = await resolveRoutes(ctx);
  console.log(`=== ${width}px ===`);
  if (skipped.length) console.log(`      skipped: ${skipped.join(', ')}`);

  for (const { slug, url } of resolved) {
    await page.goto(`${BASE}${url}`).catch(() => {});
    await page.waitForTimeout(1100);
    await dismiss();
    const m = await page.evaluate(measure);

    report(!m.overflowsX, `${slug} page body does not scroll horizontally (${m.scrollWidth} vs ${m.clientWidth})`);
    report(m.clipped.length === 0, `${slug} controls within viewport${m.clipped.length ? list(m.clipped) : ''}`);
    report(!m.bottomNavShown, `${slug} mobile bottom nav is hidden`);
    if (m.paneBar) {
      const { barLeft, barRight, paneLeft, paneRight } = m.paneBar;
      report(Math.abs(barRight - paneRight) <= 16 && Math.abs(barLeft - paneLeft) <= 16,
        `${slug} tab bar is sized to the pane it heads (bar ${barLeft}..${barRight}, pane ${paneLeft}..${paneRight})`);
    }
  }

  // §Modals: a bottom sheet on mobile, a centred card from sm: upward. Modals
  // need a trigger, so this drives a named one rather than pretending to be
  // generic — it is the rule KANI-49 broke, and the one worth a regression.
  await page.goto(`${BASE}/`);
  await page.waitForTimeout(1100);
  await dismiss();
  const trigger = page.locator('.js-filter-toggle, button:has-text("Filters")').first();
  if (await trigger.count()) {
    await trigger.click();
    await page.waitForTimeout(700);
    const d = await page.evaluate(measureDialog);
    if (d) {
      report(d.width < d.vw * 0.8, `library filters is a card, not a full-width sheet (${d.width} of ${d.vw})`);
      report(d.bottom < d.vh - 8, `library filters is not pinned to the window bottom (bottom ${d.bottom} of ${d.vh})`);
      report(Math.abs(d.top - (d.vh - d.bottom)) <= 8,
        `library filters is vertically centred (top ${d.top}, gap below ${d.vh - d.bottom})`);
    } else {
      report(false, 'library filters opened a dialog');
    }
  }
  console.log();
  await ctx.close();
}

await browser.close();
console.log(failures ? `${failures} check(s) failed` : 'all checks passed');
process.exit(failures ? 1 : 0);
