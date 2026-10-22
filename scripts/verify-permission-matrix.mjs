// @ts-check
/**
 * Per-permission UI matrix.
 *
 * The frontend hides whole surfaces behind `hasPermission(...)`, which makes the
 * number of possible layouts combinatorial. Testing every combination is not
 * worth it; testing every *permission* is, and it is linear: for each one, a
 * user who holds it and a user who does not, asserting the surfaces it gates
 * appear in the first case and not the second.
 *
 * The expectations are not written down here. They are read out of the two
 * tables that define them — the sidebar in `static/js/app.js` and the section
 * list in `static/js/pages/settings/index.js` — so this cannot drift from the
 * app the way a hand-maintained list would. A surface that changes its
 * permission changes what this asserts.
 *
 * Usage:
 *   node scripts/verify-permission-matrix.mjs <base-url> <admin-user> <admin-pass>
 *
 * Set `KANI_ROOT` if you run it from outside the repository (for instance from a
 * scratch directory that has Playwright installed).
 *
 * Needs Playwright (`npm i playwright` in a scratch directory is fine) and an
 * instance you may create users on. It creates roles and accounts named
 * `permmatrix-*`; delete them afterwards, or point it at a throwaway instance.
 *
 * It signs in once per permission and loads the whole settings tree each time,
 * which is enough traffic to drain the API rate limiter; it backs off and
 * retries, but starting the server with a raised `KANI_API_RATE_PER_SECOND` /
 * `KANI_API_BURST_SIZE` makes the run much faster.
 */

import { chromium } from 'playwright';
import { readFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

// Normally the repo this script lives in; `KANI_ROOT` lets it run from a
// directory where Playwright happens to be installed.
const ROOT = process.env.KANI_ROOT ?? join(dirname(fileURLToPath(import.meta.url)), '..');
const [BASE = 'http://127.0.0.1:8299', ADMIN = 'admin', ADMIN_PW = ''] = process.argv.slice(2);

/** Nav entries, straight out of the sidebar definition. */
function navSurfaces() {
  const src = readFileSync(join(ROOT, 'static/js/app.js'), 'utf8');
  const table = src.slice(src.indexOf("{ href: '/sources'"), src.indexOf("perm: 'admin:manage', matchPrefix: '/admin/ui-showcase' }") + 80);
  const out = [];
  for (const m of table.matchAll(/\{\s*href: '([^']+)',[^}]*?label: (?:'([^']*)'|t\('([^']+)'\))[^}]*?perm: '([^']+)'/g)) {
    out.push({ kind: 'nav', href: m[1], label: m[2] ?? m[3], perm: m[4] });
  }
  return out;
}

/**
 * Settings sections, straight out of `buildSections()`, labelled from the
 * catalogue the sidebar itself renders through — a section's id is not its
 * label (`server` shows as "Lifecycle"), so matching on the id silently misses.
 */
function settingsSurfaces() {
  const src = readFileSync(join(ROOT, 'static/js/pages/settings/index.js'), 'utf8');
  const locale = readFileSync(join(ROOT, 'static/locales/en.js'), 'utf8');
  const out = [];
  for (const m of src.matchAll(/id: '([a-z-]+)',\s*(?:\/\/[^\n]*\n\s*)*perm: '([^']+)'/g)) {
    const id = m[1];
    const key = `settings.section.${id.replace(/-/g, '_')}.label`;
    const label = locale.match(new RegExp(`'${key.replace(/\./g, '\\.')}':\\s*'([^']+)'`))?.[1];
    if (!label) throw new Error(`no ${key} in static/locales/en.js — the section list and the catalogue disagree`);
    out.push({ kind: 'settings', id, label, perm: m[2] });
  }
  return out;
}

const surfaces = [...navSurfaces(), ...settingsSurfaces()];
const permissions = [...new Set(surfaces.map((s) => s.perm))].sort();

if (!permissions.length) {
  console.error('No permission-gated surfaces found — the source tables moved.');
  process.exit(2);
}

/** Admin session, used to mint the test roles and users. */
async function adminFetch(cookie, path, init = {}) {
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    headers: { 'Content-Type': 'application/json', cookie, ...(init.headers ?? {}) },
  });
  return res;
}

async function login(username, password) {
  let res;
  for (let attempt = 0; attempt < 5; attempt++) {
    res = await fetch(`${BASE}/rest/auth/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ username, password }),
    });
    if (res.status !== 429) break;
    await new Promise((r) => setTimeout(r, 10_000 * (attempt + 1)));
  }
  if (!res.ok) throw new Error(`login failed for ${username}: ${res.status}`);
  return (res.headers.getSetCookie?.() ?? [res.headers.get('set-cookie')])
    .filter(Boolean)
    .map((c) => String(c).split(';')[0])
    .join('; ');
}

const TEST_PW = 'PermMatrixPassword123!';

async function ensureUserWith(cookie, slug, perms) {
  const role = `permmatrix-${slug}`;
  const username = `permmatrix-${slug}`;
  // Reused roles must exactly match the permission set under test.
  const roleRes = await adminFetch(cookie, '/rest/admin/roles', {
    method: 'POST',
    body: JSON.stringify({ slug: role, description: 'per-permission UI matrix', permissions: perms }),
  });
  if (!roleRes.ok) {
    const patched = await adminFetch(cookie, `/rest/admin/roles/${role}`, {
      method: 'PATCH',
      body: JSON.stringify({ description: 'per-permission UI matrix', permissions: perms }),
    });
    if (!patched.ok) {
      throw new Error(`could not create or update role ${role}: ${roleRes.status}/${patched.status}`);
    }
  }
  const created = await adminFetch(cookie, '/rest/admin/users', {
    method: 'POST',
    body: JSON.stringify({ username, email: `${username}@test.invalid`, password: TEST_PW }),
  });
  let id;
  if (created.ok) {
    id = (await created.json())?.id;
  } else {
    const list = await (await adminFetch(cookie, '/rest/admin/users?page=1&page_size=200')).json();
    id = (list.users ?? list.items ?? list).find?.((u) => u.username === username)?.id;
  }
  if (id == null) throw new Error(`could not create or find ${username}`);
  // Exactly the role under test: the default `user` role would grant extras.
  await adminFetch(cookie, `/rest/admin/users/${id}/roles/user`, { method: 'DELETE' });
  await adminFetch(cookie, `/rest/admin/users/${id}/roles`, {
    method: 'POST',
    body: JSON.stringify({ role_slug: role }),
  });
  return username;
}

/** What this account can actually see. */
async function visibleSurfaces(browser, username) {
  const ctx = await browser.newContext({ viewport: { width: 1400, height: 1000 } });
  const page = await ctx.newPage();
  // The sweep is heavy enough to drain the API rate limiter, and a throttled
  // sign-in leaves the browser sitting on /login with an empty sidebar — which
  // would otherwise be reported as a permission hiding every surface.
  let signedIn = false;
  for (let attempt = 0; attempt < 5 && !signedIn; attempt++) {
    await page.goto(`${BASE}/login`, { waitUntil: 'networkidle' });
    await page.fill('#login-username', username);
    await page.fill('#login-password', TEST_PW);
    await page.click('button[type="submit"]');
    await page.waitForTimeout(2500);
    signedIn = !new URL(page.url()).pathname.startsWith('/login');
    if (!signedIn) await page.waitForTimeout(5000 * (attempt + 1));
  }
  if (!signedIn) throw new Error(`${username} could not sign in — rate limited, or the account is missing`);

  const navHrefs = await page
    .locator('nav a, aside a')
    .evaluateAll((els) => els.map((e) => e.getAttribute('href')).filter(Boolean));

  let sectionIds = [];
  if (navHrefs.includes('/settings')) {
    await page.goto(`${BASE}/settings`, { waitUntil: 'networkidle' });
    await page.waitForTimeout(1500);
    sectionIds = await page
      .locator('main button, nav button')
      .evaluateAll((els) => els.map((e) => (e.textContent || '').trim().toLowerCase()).filter(Boolean));
  }
  await ctx.close();
  return { navHrefs: new Set(navHrefs), sectionText: sectionIds.join(' | ') };
}

const browser = await chromium.launch();
const cookie = await login(ADMIN, ADMIN_PW);

console.log(`Checking ${permissions.length} permissions over ${surfaces.length} gated surfaces\n`);

// A settings section is only reachable by an account that can open Settings at
// all, so `settings:view` is part of the baseline rather than a variable — with
// it missing, every section reads as "hidden" and the run says nothing.
const BASELINE_PERMS = ['settings:view'];

// Use the baseline for most negative cases and no permissions when testing the baseline itself.
const baseline = await visibleSurfaces(browser, await ensureUserWith(cookie, 'baseline', BASELINE_PERMS));
const nothing = await visibleSurfaces(browser, await ensureUserWith(cookie, 'nothing', []));

let failures = 0;
for (const perm of permissions) {
  const gated = surfaces.filter((s) => s.perm === perm);
  const slug = perm.replace(/[^a-z]/g, '-');
  const user = await ensureUserWith(cookie, slug, [...new Set([...BASELINE_PERMS, perm])]);
  const seen = await visibleSurfaces(browser, user);
  // Comparing a baseline permission against the baseline would prove nothing.
  const without = BASELINE_PERMS.includes(perm) ? nothing : baseline;

  for (const surface of gated) {
    const present = surface.kind === 'nav'
      ? seen.navHrefs.has(surface.href)
      : seen.sectionText.includes(surface.label.toLowerCase());
    const presentWithout = surface.kind === 'nav'
      ? without.navHrefs.has(surface.href)
      : without.sectionText.includes(surface.label.toLowerCase());

    const name = surface.kind === 'nav' ? surface.href : `settings/${surface.id} ("${surface.label}")`;
    if (!present) {
      console.log(`FAIL  ${perm} → ${name} is hidden from an account that holds it`);
      failures++;
    } else if (presentWithout) {
      console.log(`FAIL  ${perm} → ${name} is visible to an account that does not`);
      failures++;
    } else {
      console.log(`PASS  ${perm} → ${name}`);
    }
  }
}

await browser.close();
console.log(`\n${failures === 0 ? 'All gated surfaces behave' : `${failures} mismatch(es)`}`);
process.exit(failures ? 1 : 0);
