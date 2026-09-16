// @ts-check
// Manga details — hero (cover + meta + description) + CTA button group.
// Handles responsive layout swap between mobile and desktop.

import * as api from '../../api.js';
import { t } from '../../i18n.js';
import { hasPermission } from '../../session.js';
import { getState, subscribe } from '../../cache.js';
import { navigate } from '../../router.js';
import { getLocal, setLocal, escapeHtml, formatRelativeTime } from '../../utils.js';
import { createCoverImage } from '../cover-image.js';
import { showToast, showApiError } from '../toast.js';
import { iconSpinner } from '../../icons.js';
import { subscribeJob } from '../../sse.js';

/** Tears down an open description panel when the hero is rebuilt. */
let _destroyDesc = /** @type {(() => void)|null} */ (null);


/** @type {Map<string|number, Promise<any[]>>} */
const _sourceFilterDefsCache = new Map();

/**
 * @param {string|number} sid
 * @param {string} name
 * @param {'Author'|'Artist'|'Tag'} semantic
 * @returns {Promise<string>}
 */
export async function buildSourceMetaUrl(sid, name, semantic) {
  if (!_sourceFilterDefsCache.has(sid)) {
    _sourceFilterDefsCache.set(sid, api.getSourceFilters(sid)
      .then(fl => Array.isArray(fl?.filters) ? fl.filters : [])
      .catch(() => []));
  }
  const defs = await _sourceFilterDefsCache.get(sid);
  const match = defs.find(f => f.semantic === semantic);
  if (match) {
    return `/source/${sid}?filter_name=${encodeURIComponent(match.name)}&filter_value=${encodeURIComponent(name)}`;
  }
  return `/source/${sid}?q=${encodeURIComponent(name)}`;
}


/** @param {string} url */
function _showExternalLinkDialog(url) {
  const overlay = document.createElement('div');
  overlay.className = 'fixed inset-0 bg-scrim z-modal flex items-center justify-center p-4';

  const dialog = document.createElement('div');
  dialog.className = 'bg-surface rounded-xl p-6 max-w-sm w-full shadow-xl flex flex-col gap-4';
  dialog.innerHTML = `
    <div class="flex flex-col gap-1">
      <h3 class="text-base font-semibold text-text">${t('manga.header.external_link.title')}</h3>
      <p class="text-sm text-text-muted">${t('manga.header.external_link.body')}</p>
      <p class="text-sm text-accent break-all">${escapeHtml(url)}</p>
    </div>
    <label class="flex items-center gap-2 text-sm text-text-muted cursor-pointer select-none">
      <input type="checkbox" class="js-dont-ask accent-accent" />
      ${t('manga.header.external_link.dont_ask')}
    </label>
    <div class="flex gap-2 justify-end">
      <button type="button" class="btn-ghost btn-sm js-cancel">${t('common.cancel')}</button>
      <button type="button" class="btn-primary btn-sm js-continue">${t('manga.header.external_link.open')}</button>
    </div>
  `;
  overlay.appendChild(dialog);

  const close = () => overlay.remove();
  overlay.addEventListener('click', (e) => { if (e.target === overlay) close(); });
  dialog.querySelector('.js-cancel')?.addEventListener('click', close);
  dialog.querySelector('.js-continue')?.addEventListener('click', () => {
    if (/** @type {HTMLInputElement} */ (dialog.querySelector('.js-dont-ask')).checked) {
      setLocal('kani_skip_external_warning', 'true');
    }
    window.open(url, '_blank', 'noopener,noreferrer');
    close();
  });

  document.body.appendChild(overlay);
  /** @type {HTMLButtonElement|null} */ (dialog.querySelector('.js-continue'))?.focus();
}


/**
 * @typedef {{
 *   isLocal: boolean,
 *   dbId: number,
 *   sid: number,
 *   mangaId: string,
 *   existingDbId: () => number|null,
 *   addedDbId: () => number|null,
 *   findNextPreferredChapter: () => any|null,
 *   getChapters: () => any[],
 *   onDownloadAll: () => Promise<{ jobId: string | null }>,
 *   onCancelAll: () => Promise<void>,
 *   onScan: () => Promise<{new_chapters?: number}>,
 *   onAddedToLibrary: (newDbId: number) => void,
 *   onSwitchToChapters: () => void,
 * }} HeroCtx
 */

/**
 * Mounts the hero (cover + meta + description) and CTA button group into leftCol.
 * Returns { destroy, coverEl }.
 *
 * @param {HTMLElement} leftCol
 * @param {any} info        manga metadata
 * @param {any} source      source metadata (may be null for remote views)
 * @param {HeroCtx} ctx
 * @returns {{ destroy: () => void, coverEl: HTMLElement }}
 */
export function mountMangaHeader(leftCol, info, source, ctx) {
  const { isLocal, dbId, sid, mangaId } = ctx;

  const coverUrl = info?.cover_url
    ?? info?.cover_image_url
    ?? (isLocal ? api.getMangaCoverUrl(dbId, 'lg') : null);

  // The rail activates only when both it and the chapter column retain usable width.
  const isDesktop = () => window.innerWidth >= 1024;

  const coverInner = document.createElement('div');
  coverInner.className = 'rail-cover-slot rounded-xl overflow-hidden bg-surface-2 cursor-pointer shadow-card';
  coverInner.appendChild(createCoverImage({ url: coverUrl, alt: info?.title ?? '' }));

  // The frame the cover is cropped into. Its height is what changes with the
  // viewport — in steps, see .rail-cover-box — while the width stays the
  // rail's, so the jacket's edges keep meeting the controls below it.
  const coverBox = document.createElement('div');
  coverBox.className = 'rail-cover-box';
  coverBox.appendChild(coverInner);

  coverInner.addEventListener('click', () => {
    if (!coverUrl) return;
    const rect = coverInner.getBoundingClientRect();
    const overlay = document.createElement('div');
    overlay.className = 'fixed inset-0 z-top flex items-center justify-center';
    overlay.style.cssText = 'background:rgba(0,0,0,0);transition:background var(--motion-slow) ease'; // audit-ignore: cover lightbox backdrop (animated, theme-independent)

    const img = document.createElement('img');
    img.src = coverUrl;
    img.alt = info?.title ?? '';
    img.className = 'shadow-2xl object-contain';
    img.style.cssText = `position:fixed;top:${rect.top}px;left:${rect.left}px;width:${rect.width}px;height:${rect.height}px;border-radius:0.75rem;object-fit:contain;`;
    overlay.appendChild(img);
    document.body.appendChild(overlay);

    img.getBoundingClientRect();
    img.style.transition = `top 280ms var(--motion-ease), left 280ms var(--motion-ease), width 280ms var(--motion-ease), height 280ms var(--motion-ease), border-radius 280ms ease`;
    overlay.style.background = 'rgba(0,0,0,0.6)'; // audit-ignore: cover lightbox scrim

    const vw = window.innerWidth, vh = window.innerHeight;
    const maxW = vw * 0.9, maxH = vh * 0.9;
    const scale = Math.min(maxW / rect.width, maxH / rect.height, 3);
    const newW = rect.width * scale, newH = rect.height * scale;
    img.style.top = ((vh - newH) / 2) + 'px';
    img.style.left = ((vw - newW) / 2) + 'px';
    img.style.width = newW + 'px';
    img.style.height = newH + 'px';
    img.style.borderRadius = '1rem';

    const close = () => {
      overlay.style.background = 'rgba(0,0,0,0)'; /* audit-ignore: animated cover backdrop */
      img.style.top = rect.top + 'px'; img.style.left = rect.left + 'px';
      img.style.width = rect.width + 'px'; img.style.height = rect.height + 'px';
      setTimeout(() => overlay.remove(), 280);
    };
    overlay.addEventListener('click', close);
  });

  const meta = document.createElement('div');
  meta.className = 'rail-meta flex flex-col gap-3';

  const META_LINK_CLS = 'touch-min-h inline-flex items-center text-text hover:text-accent hover:underline underline-offset-4 transition-colors focus-visible:outline-none focus-visible:text-accent';

  /**
   * Splits the two credit lists into the rows a cover would print.
   *
   * Whoever is both author and artist takes the byline; whoever is left on
   * each side follows. Rows come out most-responsible first and empty ones are
   * dropped, so a name is never printed twice.
   *
   * @param {Array<{ id?: number, name: string }>} authors
   * @param {Array<{ id?: number, name: string }>} artists
   * @returns {Array<{ roleKey: string, people: Array<{ id?: number, name: string }>, kind: 'Author'|'Artist', param: string }>}
   */
  function splitCredits(authors, artists) {
    const artistNames = new Set(artists.map(a => a.name));
    const authorNames = new Set(authors.map(a => a.name));
    const both = authors.filter(a => artistNames.has(a.name));
    const storyOnly = authors.filter(a => !artistNames.has(a.name));
    const artOnly = artists.filter(a => !authorNames.has(a.name));
    const rows = [];
    if (both.length) rows.push({ roleKey: 'manga.header.role.story_art', people: both, kind: /** @type {'Author'} */ ('Author'), param: 'author_id' });
    if (storyOnly.length) rows.push({ roleKey: 'manga.header.role.story', people: storyOnly, kind: /** @type {'Author'} */ ('Author'), param: 'author_id' });
    if (artOnly.length) rows.push({ roleKey: 'manga.header.role.art', people: artOnly, kind: /** @type {'Artist'} */ ('Artist'), param: 'artist_id' });
    return rows;
  }

  const credits = splitCredits(info?.authors ?? [], info?.artists ?? []);
  if (credits.length) {
    const block = document.createElement('div');
    block.className = 'flex flex-col gap-3';
    credits.forEach((row, idx) => {
      const line = document.createElement('div');
      line.className = 'flex flex-col gap-0.5 min-w-0';
      const names = document.createElement('p');
      names.className = idx === 0 ? 'rail-credit-name' : 'rail-credit-name rail-credit-name--secondary';
      names.innerHTML = row.people.map((a, i) => isLocal
        ? `<a class="${META_LINK_CLS}" href="/?${row.param}=${a.id}" data-idx="${i}">${escapeHtml(a.name)}</a>`
        : `<a class="${META_LINK_CLS}" href="/source/${sid}?q=${encodeURIComponent(a.name)}" data-idx="${i}">${escapeHtml(a.name)}</a>`
      ).join(', ');
      names.querySelectorAll('a').forEach(el => {
        const person = row.people[Number(/** @type {HTMLElement} */(el).dataset.idx)];
        el.addEventListener('click', e => {
          e.preventDefault();
          if (isLocal) navigate(`/?${row.param}=${person.id}`);
          else buildSourceMetaUrl(sid, person.name, row.kind).then(url => navigate(url));
        });
      });
      const role = document.createElement('p');
      role.className = 'rail-credit-role';
      role.textContent = t(row.roleKey);
      line.appendChild(names);
      line.appendChild(role);
      block.appendChild(line);
    });
    meta.appendChild(block);
  }

  const facts = document.createElement('dl');
  facts.className = 'rail-facts';
  let factCount = 0;

  /**
   * @param {string} label
   * @param {string} valueHtml
   * @param {string} [sealCls] colour class for the status seal, when this row has one
   * @returns {HTMLElement} the value element, for wiring links
   */
  function mkFact(label, valueHtml, sealCls) {
    const cell = document.createElement('div');
    cell.className = 'flex flex-col gap-0.5 min-w-0';
    const k = document.createElement('dt');
    k.className = 'rail-fact-k';
    k.textContent = label;
    const v = document.createElement('dd');
    v.className = 'rail-fact-v m-0 flex items-center gap-1.5 min-w-0';
    if (sealCls) {
      const seal = document.createElement('span');
      seal.className = `rail-seal ${sealCls}`;
      seal.setAttribute('aria-hidden', 'true');
      v.appendChild(seal);
    }
    const text = document.createElement('span');
    text.className = 'truncate';
    text.innerHTML = valueHtml;
    v.appendChild(text);
    cell.appendChild(k);
    cell.appendChild(v);
    facts.appendChild(cell);
    factCount++;
    return text;
  }

  if (isLocal && (source || info?.source_id || sid)) {
    const sname = escapeHtml(source?.name || info?.source_name || 'Source');
    const srcId = source?.id || info?.source_id || sid;
    const val = mkFact(t('manga.header.source'), `<a href="/source/${srcId}" class="${META_LINK_CLS}">${sname}</a>`);
    val.querySelector('a')?.addEventListener('click', e => { e.preventDefault(); navigate(`/source/${srcId}`); });
  }

  if (info?.status && info.status.toLowerCase() !== 'unknown') {
    const statusVal = info.status.toLowerCase();
    const statusDisplay = info.status.charAt(0).toUpperCase() + info.status.slice(1);
    const sealCls = {
      ongoing: 'bg-accent', publishing: 'bg-accent', releasing: 'bg-accent',
      completed: 'bg-text-faint', finished: 'bg-text-faint',
      hiatus: 'bg-warn', 'on hiatus': 'bg-warn',
      cancelled: 'bg-danger', dropped: 'bg-danger',
    }[statusVal] ?? 'bg-text-faint';
    const val = mkFact(t('manga.header.status'),
      `<a href="/?status=${statusVal}" class="${META_LINK_CLS}">${escapeHtml(statusDisplay)}</a>`, sealCls);
    val.querySelector('a')?.addEventListener('click', e => { e.preventDefault(); navigate(`/?status=${statusVal}`); });
  }

  if (isLocal && Number.isFinite(Number(info?.chapter_count))) {
    mkFact(t('manga.header.chapters'), String(Number(info.chapter_count)));
  }

  if (isLocal && info?.added_at) {
    const when = new Date(info.added_at);
    if (!Number.isNaN(when.getTime())) {
      mkFact(t('manga.header.added'), escapeHtml(formatRelativeTime(info.added_at)));
    }
  }

  if (factCount) {
    if (credits.length) {
      const rule = document.createElement('div');
      rule.className = 'h-px bg-border-subtle';
      meta.appendChild(rule);
    }
    meta.appendChild(facts);
  }

  const btnGroupEl = document.createElement('div');
  btnGroupEl.className = 'flex flex-col gap-2';
  _renderBtnGroup(btnGroupEl, info, source, ctx);

  /** @type {HTMLElement|null} */ let descWrap = null;
  /** @type {HTMLElement|null} */ let desc = null;
  let expanded = false;

  if (info?.description_html || info?.description) {
    descWrap = document.createElement('div');
    descWrap.className = 'flex flex-col gap-1.5 min-h-0 page-fill md:pb-1';
    desc = document.createElement('div');
    desc.className = 'text-sm text-text-muted leading-relaxed rail-desc';
    // Deliberately not role="button": a synopsis can contain links, and a control containing
    // links is nested interactive content — invalid, and unusable by keyboard. The button below
    // owns the interaction; clicking the text is a mouse convenience on top of it.
    desc.id = 'manga-description';
    desc.innerHTML = info.description_html ?? `<p>${escapeHtml(info.description)}</p>`;

    desc.querySelectorAll('a[href]').forEach(link => {
      link.classList.add('text-accent', 'underline', 'decoration-accent/50', 'hover:decoration-accent');
      link.setAttribute('target', '_blank');
      link.setAttribute('rel', 'noopener noreferrer');
      link.addEventListener('click', (e) => {
        e.stopPropagation();
        if (getLocal('kani_skip_external_warning') === 'true') return;
        e.preventDefault();
        _showExternalLinkDialog(/** @type {HTMLAnchorElement} */(link).href);
      });
    });

    const toggle = document.createElement('button');
    toggle.type = 'button';
    toggle.className = 'touch-min-h inline-flex items-center justify-center text-xs text-text-muted underline underline-offset-2 decoration-border hover:text-accent text-center self-center shrink-0 py-1';
    toggle.textContent = t('manga.header.show_more');
    toggle.setAttribute('aria-expanded', 'false');
    toggle.setAttribute('aria-controls', 'manga-description');
    toggle.hidden = true;

    descWrap.appendChild(desc);
    descWrap.appendChild(toggle);

    /** The rail root: ancestor of both the cover and the facts, since reading
     *  state restyles both. */
    const railRoot = () => desc?.closest('.manga-rail') ?? null;

    const setReading = (/** @type {boolean} */ on) => {
      if (!desc) return;
      expanded = on;
      railRoot()?.classList.toggle('rail--reading', on);
      // Tailwind's own utility rather than a descendant rule: the utilities
      // layer wins on order, so `.rail--reading .rail-meta { display:none }`
      // never beat the `flex` class already on this element.
      meta.classList.toggle('hidden', on);
      toggle.textContent = on ? t('manga.header.show_less') : t('manga.header.show_more');
      toggle.setAttribute('aria-expanded', String(on));

      // Without the fit-to-viewport shell the synopsis sits under a tall hero,
      // so expanding it pushed the chapter list off the bottom. Bring the
      // synopsis to the top instead: both it and the list stay in view.
      if (on && !getComputedStyle(document.documentElement).getPropertyValue('--grid-fit').trim()) {
        requestAnimationFrame(() => desc.scrollIntoView({ block: 'start', behavior: 'smooth' }));
      }
    };

    // Only offer it when there is more than the three lines already showing.
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (desc && desc.scrollHeight <= desc.clientHeight + 2) {
          desc.classList.remove('rail-desc');
        } else {
          toggle.hidden = false;
        }
      });
    });

    desc.addEventListener('click', () => { if (!expanded && desc?.classList.contains('rail-desc')) setReading(true); });
    toggle.addEventListener('click', () => setReading(!expanded));
    _destroyDesc = () => setReading(false);
  }

  const titleMetaCard = document.createElement('div');
  titleMetaCard.className = 'flex flex-col gap-2 min-w-0';
  titleMetaCard.appendChild(meta);

  const contentCard = document.createElement('div');
  contentCard.className = 'flex flex-col gap-3 min-w-0 rail-content';
  contentCard.style.position = 'relative';
  contentCard.style.zIndex = '1';

  // The rail holds credits, facts and the description. No surrounding surface:
  // the hairline between credits and facts carries the structure, and a border
  // around content that is already the only thing in the column adds nothing.
  const metaPanel = document.createElement('div');
  // The scroll boundary sits below the actions: the cover and Read/Download/Scan
  // hold their place, and only the credits, facts and description move. Putting
  // it any higher scrolled the primary actions out of reach.
  metaPanel.className = 'rail-metapanel flex flex-col gap-3 min-w-0 pt-1 rail-meta-panel';

  const heroRow = document.createElement('div');
  leftCol.appendChild(heroRow);

  function applyHeroLayout() {
    if (!isDesktop()) {
      leftCol.style.maxWidth = '';
      heroRow.style.cssText = 'display:flex;flex-direction:row;align-items:flex-start;gap:0.75rem';
      coverInner.style.width = '35%';
      coverInner.style.marginLeft = '';
      coverInner.style.marginRight = '';
      // Pull meta/description out of the desktop panel back to their mobile homes.
      if (!titleMetaCard.contains(meta)) titleMetaCard.appendChild(meta);
      if (!heroRow.contains(coverBox)) heroRow.insertBefore(coverBox, heroRow.firstChild);
      if (!heroRow.contains(titleMetaCard)) heroRow.appendChild(titleMetaCard);
      if (heroRow.contains(contentCard)) heroRow.removeChild(contentCard);
      titleMetaCard.style.flex = '1 1 0%';
      btnGroupEl.style.paddingTop = '0.5rem';
      btnGroupEl.style.paddingBottom = '0.5rem';
      if (!leftCol.contains(btnGroupEl)) leftCol.appendChild(btnGroupEl);
      if (descWrap && !leftCol.contains(descWrap)) leftCol.appendChild(descWrap);
    } else {
      // Cap the column so the cover and panel don't sprawl across a quarter of
      // an ultra-wide viewport.
      leftCol.style.maxWidth = '';
      heroRow.style.cssText = 'display:flex;flex-direction:column;gap:1rem;flex:1 1 auto;min-height:0';
      btnGroupEl.style.paddingTop = '';
      btnGroupEl.style.paddingBottom = '';
      if (!heroRow.contains(coverBox)) heroRow.insertBefore(coverBox, heroRow.firstChild);
      if (!heroRow.contains(contentCard)) heroRow.appendChild(contentCard);
      if (heroRow.contains(titleMetaCard)) heroRow.removeChild(titleMetaCard);
      // Order: actions (free) then the reference panel (metadata + description).
      if (contentCard.firstChild !== btnGroupEl) contentCard.insertBefore(btnGroupEl, contentCard.firstChild);
      if (!metaPanel.contains(meta)) metaPanel.insertBefore(meta, metaPanel.firstChild);
      if (descWrap && !metaPanel.contains(descWrap)) metaPanel.appendChild(descWrap);
      if (!contentCard.contains(metaPanel)) contentCard.appendChild(metaPanel);
      // Width comes from the 2:3 ratio against the box's flexed height, so it
      // must not be pinned here — an inline width beats the stylesheet and was
      // what kept the jacket 280 px wide while its height collapsed.
      coverInner.style.width = '';
      coverInner.style.marginLeft = '';
      coverInner.style.marginRight = '';
    }
  }

  applyHeroLayout();
  window.addEventListener('resize', applyHeroLayout);

  return {
    destroy() {
      window.removeEventListener('resize', applyHeroLayout);
      _destroyDesc?.();
      _destroyDesc = null;
    },
    coverEl: coverInner,
  };
}


/**
 * @param {HTMLElement} btnGroupEl
 * @param {any} info
 * @param {any} source
 * @param {HeroCtx} ctx
 */
function _renderBtnGroup(btnGroupEl, info, source, ctx) {
  btnGroupEl.innerHTML = '';
  const { isLocal, dbId, sid, mangaId, onDownloadAll, onCancelAll, onScan, onAddedToLibrary, onSwitchToChapters, findNextPreferredChapter } = ctx;

  if (isLocal) {
    const readBtn = document.createElement('button');
    readBtn.type = 'button';
    readBtn.className = 'btn-primary w-full';
    readBtn.textContent = t('manga.header.read');
    readBtn.addEventListener('click', async () => {
      if (readBtn.disabled) return;
      readBtn.disabled = true;
      try {
        const info = await api.getContinueReading(dbId);
        if (info) {
          const href = info.last_page > 0 ? `/reader/${info.chapter_id}?page=${info.last_page}` : `/reader/${info.chapter_id}`;
          navigate(href);
          return;
        }
        const nextUnread = findNextPreferredChapter();
        if (!nextUnread) {
          readBtn.disabled = false;
          const hasAnyUnread = ctx.getChapters().some(ch => !ch.read);
          if (hasAnyUnread) showToast(t('manga.header.no_pref_chapters'), { type: 'warning' });
          else showToast(t('manga.header.all_read'));
          return;
        }
        const originalText = readBtn.textContent;
        readBtn.innerHTML = `<span class="inline-block animate-spin icon-sm">${iconSpinner}</span> ${t('manga.header.downloading')}`;
        try {
          await api.downloadChapter(nextUnread.id);
          await new Promise((resolve, reject) => {
            let timeout;
            if (getState('chaptersProgress').get(nextUnread.id)?.status === 'completed') { resolve(undefined); return; }
            const unsub = subscribe('chaptersProgress', () => {
              if (getState('chaptersProgress').get(nextUnread.id)?.status === 'completed') {
                clearTimeout(timeout); unsub(); resolve(undefined);
              }
            });
            timeout = setTimeout(() => { unsub(); reject(new Error('timeout')); }, 5 * 60 * 1000);
          });
          navigate(`/reader/${nextUnread.id}`);
        } catch {
          showToast(t('manga.header.download_failed'));
          readBtn.textContent = originalText;
          readBtn.disabled = false;
        }
      } catch { readBtn.disabled = false; }
    });
    api.getMangaTracking(dbId).then(t => {
      if (t && t.chapters_read > 0) readBtn.textContent = t('manga.header.continue_reading');
      else readBtn.textContent = t('manga.header.start_reading');
    }).catch(() => {});
    btnGroupEl.appendChild(readBtn);

    const actionRow = document.createElement('div');
    actionRow.className = 'flex gap-2';

    if (hasPermission('chapter:download')) {
      const dlBtn = document.createElement('button');
      dlBtn.type = 'button';
      dlBtn.className = 'btn-ghost btn-sm flex-1';
      dlBtn.textContent = t('manga.header.download_all');

      const cancelBtn = document.createElement('button');
      cancelBtn.type = 'button';
      cancelBtn.className = 'btn-ghost btn-sm flex-1';
      cancelBtn.textContent = t('manga.header.cancel_all');
      cancelBtn.style.display = 'none';

      const _dlBtnOriginalText = dlBtn.textContent;
      dlBtn.addEventListener('click', async () => {
        dlBtn.disabled = true;
        try {
          const { jobId } = await onDownloadAll();
          dlBtn.style.display = 'none';
          cancelBtn.style.display = '';
          if (jobId) {
            subscribeJob(jobId, {
              onProgress: (e) => {
                const cur = e.current ?? 0, tot = e.total ?? 0;
                dlBtn.textContent = tot > 0 ? `${cur}/${tot}` : _dlBtnOriginalText;
              },
              onComplete: () => {
                dlBtn.style.display = '';
                cancelBtn.style.display = 'none';
                dlBtn.textContent = _dlBtnOriginalText;
              },
              onFailed: () => {
                dlBtn.style.display = '';
                cancelBtn.style.display = 'none';
                dlBtn.textContent = _dlBtnOriginalText;
              },
              onCancelled: () => {
                dlBtn.style.display = '';
                cancelBtn.style.display = 'none';
                dlBtn.textContent = _dlBtnOriginalText;
              },
            });
          }
        } finally { dlBtn.disabled = false; }
      });
      cancelBtn.addEventListener('click', async () => {
        cancelBtn.disabled = true;
        try {
          await onCancelAll();
        } finally {
          cancelBtn.disabled = false;
          cancelBtn.style.display = 'none';
          dlBtn.style.display = '';
        }
      });

      actionRow.appendChild(dlBtn);
      actionRow.appendChild(cancelBtn);
    }

    if (hasPermission('library:refresh')) {
      const scanBtn = document.createElement('button');
      scanBtn.type = 'button';
      scanBtn.className = 'btn-ghost btn-sm flex-1';
      scanBtn.textContent = t('manga.header.scan');
      scanBtn.addEventListener('click', async () => {
        scanBtn.disabled = true;
        try {
          const res = await onScan();
          const count = res?.new_chapters ?? 0;
          showToast(
            count > 0 ? t('manga.header.scan.found', { count, s: count !== 1 ? 's' : '' }) : t('manga.header.scan.none'),
            { type: count > 0 ? 'success' : 'info' },
          );
        } catch (e) {
          showApiError(e);
        } finally { scanBtn.disabled = false; }
      });
      actionRow.appendChild(scanBtn);
    }

    if (actionRow.children.length > 0) btnGroupEl.appendChild(actionRow);
  } else {
    const inLibrary = !!(ctx.existingDbId() || ctx.addedDbId());
    if (inLibrary) {
      const existId = ctx.existingDbId() ?? ctx.addedDbId();
      const goBtn = document.createElement('a');
      goBtn.className = 'btn-primary w-full text-center';
      goBtn.textContent = t('manga.header.go_to_entry');
      goBtn.href = `/manga/${existId}`;
      goBtn.addEventListener('click', e => { e.preventDefault(); navigate(`/manga/${existId}`); });
      btnGroupEl.appendChild(goBtn);
    } else if (hasPermission('library:add')) {
      const addBtn = document.createElement('button');
      addBtn.type = 'button';
      addBtn.className = 'btn-primary w-full';
      addBtn.textContent = t('manga.header.add_to_library');
      addBtn.addEventListener('click', async () => {
        addBtn.disabled = true;
        try {
          const res = await api.saveToLibrary(sid, mangaId);
          const newDbId = res?.db_id ?? res?.id ?? null;
          if (newDbId) {
            onAddedToLibrary(newDbId);
            _renderBtnGroup(btnGroupEl, info, source, ctx);
          }
        } catch (err) {
          if (err?.status === 409 && err?.suggestions) {
            _showDuplicateModal(err.suggestions, sid, mangaId, onAddedToLibrary, btnGroupEl, info, source, ctx);
          } else {
            addBtn.disabled = false;
          }
        }
      });
      btnGroupEl.appendChild(addBtn);
    }
  }
}

/**
 * Show an inline duplicate-warning dialog below the add button.
 * @param {Array<{id: number, name: string, similarity: number, author_match: boolean}>} suggestions
 */
function _showDuplicateModal(suggestions, sid, mangaId, onAddedToLibrary, btnGroupEl, info, source, ctx) {
  const overlay = document.createElement('div');
  overlay.className = 'mt-3 p-3 rounded-lg bg-surface-2 border border-border-subtle text-sm flex flex-col gap-2';

  const top = document.createElement('div');
  top.className = 'font-medium text-warn';
  top.textContent = t('manga.header.duplicate.title');
  overlay.appendChild(top);

  for (const s of suggestions.slice(0, 3)) {
    const row = document.createElement('div');
    row.className = 'flex items-center justify-between gap-2';
    const nameLink = document.createElement('a');
    nameLink.href = `/manga/${s.id}`;
    nameLink.className = 'text-text-muted underline underline-offset-2 decoration-border hover:text-accent truncate';
    nameLink.textContent = s.name;
    nameLink.addEventListener('click', e => { e.preventDefault(); navigate(`/manga/${s.id}`); });
    const meta = document.createElement('span');
    meta.className = 'text-text-muted text-xs shrink-0';
    meta.textContent = `${Math.round(s.similarity * 100)}% match${s.author_match ? ' · author' : ''}`;
    row.appendChild(nameLink);
    row.appendChild(meta);
    overlay.appendChild(row);
  }

  const actions = document.createElement('div');
  actions.className = 'flex gap-2 pt-1';

  const forceBtn = document.createElement('button');
  forceBtn.type = 'button';
  forceBtn.className = 'btn-primary btn-sm';
  forceBtn.textContent = t('manga.header.duplicate.add_anyway');
  forceBtn.addEventListener('click', async () => {
    forceBtn.disabled = true;
    try {
      const res = await api.saveToLibrary(sid, mangaId, true);
      const newDbId = res?.db_id ?? res?.id ?? null;
      overlay.remove();
      if (newDbId) { onAddedToLibrary(newDbId); _renderBtnGroup(btnGroupEl, info, source, ctx); }
    } catch { forceBtn.disabled = false; }
  });

  const cancelBtn = document.createElement('button');
  cancelBtn.type = 'button';
  cancelBtn.className = 'btn-secondary btn-sm';
  cancelBtn.textContent = t('common.cancel');
  cancelBtn.addEventListener('click', () => overlay.remove());

  actions.appendChild(forceBtn);
  actions.appendChild(cancelBtn);
  overlay.appendChild(actions);
  btnGroupEl.appendChild(overlay);
}
