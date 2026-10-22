// @ts-check

import { createCoverImage } from './cover-image.js';
import { getMangaCoverUrl } from '../api.js';
import { applyCoverQuality } from '../cover-quality.js';
import { iconEllipsisVertical, iconPlay, iconWarning } from '../icons.js';
import { t } from '../i18n.js';

/**
 * @typedef {{ chapter_id: number, chapter_number: number, last_page: number, page_count: number }} MangaCardResume
 * @typedef {{ id: number, title: string, source_id?: number | null, cover_image_url?: string | null, new_chapter_count?: number, is_orphaned?: boolean, import_link_status?: string | null, resume?: MangaCardResume | null }} MangaCardData
 */

/**
 * @param {{
 *   manga: MangaCardData,
 *   href: string,
 *   badge?: string | null,
 *   extraClass?: string,
 *   eager?: boolean,
 *   onCardClick?: ((manga: MangaCardData) => void) | null,
 *   onMenuClick?: ((manga: MangaCardData, btnEl: HTMLElement) => void) | null,
 *   onResumeClick?: ((manga: MangaCardData) => void) | null,
 * }} props
 * @returns {HTMLElement}
 */
export function createMangaCard({ manga, href, badge = null, extraClass = '', eager = false, onCardClick = null, onMenuClick = null, onResumeClick = null }) {
  const card = document.createElement('div');
  card.className = ['manga-card', extraClass].filter(Boolean).join(' ');
  card.dataset.mangaId = String(manga.id);

  const coverUrl = applyCoverQuality(manga.cover_image_url ?? (manga.source_id != null
    ? getMangaCoverUrl(manga.id, 'sm')
    : null));
  const link = document.createElement('a');
  link.href = href;

  const coverWrap = document.createElement('div');
  coverWrap.className = 'relative w-full overflow-hidden rounded-sm bg-surface-2';
  coverWrap.style.aspectRatio = '2/3';
  coverWrap.appendChild(createCoverImage({
    url: coverUrl,
    alt: manga.title,
    loading: eager ? 'eager' : 'lazy',
    fetchpriority: eager ? 'high' : 'auto',
  }));

  const titleEl = document.createElement('p');
  titleEl.className = 'title';
  const titleSpan = document.createElement('span');
  titleSpan.textContent = manga.title;
  titleEl.appendChild(titleSpan);

  if (manga.new_chapter_count && manga.new_chapter_count > 0) {
    const newBadge = document.createElement('span');
    newBadge.className = 'absolute top-1 left-1 bg-accent text-on-accent text-xs font-bold px-1.5 py-0.5 rounded-full leading-none z-10';
    newBadge.textContent = manga.new_chapter_count > 99 ? '99+' : String(manga.new_chapter_count);
    newBadge.setAttribute('aria-label', t('manga.badge.new_chapters', { count: manga.new_chapter_count, s: manga.new_chapter_count !== 1 ? 's' : '' }));
    coverWrap.appendChild(newBadge);
  }

  if (manga.is_orphaned) {
    const orphanBadge = document.createElement('span');
    orphanBadge.className = 'absolute top-1 right-1 bg-warn/20 text-warn text-xs font-semibold px-1.5 py-0.5 rounded leading-none z-10';
    orphanBadge.textContent = t('manga.badge.orphaned');
    orphanBadge.setAttribute('aria-label', t('manga.badge.orphaned.aria'));
    coverWrap.appendChild(orphanBadge);
  }

  setMangaCardImportLink(coverWrap, manga.import_link_status ?? null);

  coverWrap.appendChild(titleEl);
  link.appendChild(coverWrap);
  if (onCardClick) {
    link.addEventListener('click', (e) => { e.preventDefault(); onCardClick(manga); });
  }
  card.appendChild(link);

  if (onMenuClick) {
    const menuBtn = document.createElement('button');
    menuBtn.type = 'button';
    menuBtn.className = 'manga-card__menu-btn';
    menuBtn.setAttribute('aria-label', t('common.more_options'));
    menuBtn.innerHTML = iconEllipsisVertical;
    menuBtn.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();
      onMenuClick(manga, menuBtn);
    });
    coverWrap.appendChild(menuBtn);
  }

  if (manga.resume && onResumeClick) {
    const resumeBtn = document.createElement('button');
    resumeBtn.type = 'button';
    resumeBtn.className = 'manga-card__resume-btn';
    resumeBtn.setAttribute('aria-label', t('manga.resume.aria'));
    resumeBtn.innerHTML = iconPlay;
    resumeBtn.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();
      onResumeClick(manga);
    });
    coverWrap.appendChild(resumeBtn);
  }

  if (badge) {
    const badgeEl = document.createElement('span');
    badgeEl.className = 'manga-card__badge';
    badgeEl.textContent = badge;
    card.appendChild(badgeEl);
  }

  return card;
}

/**
 * Marks a cover whose manga was imported and cannot be addressed on its source
 * yet: a spinner while the queue still has it, a static note once it gave up.
 * @param {Element} coverWrap
 * @param {string | null} status
 */
function setMangaCardImportLink(coverWrap, status) {
  coverWrap.querySelector('.js-import-link')?.remove();
  if (status !== 'pending' && status !== 'unlinked') return;

  const overlay = document.createElement('div');
  overlay.className = 'js-import-link absolute inset-0 flex flex-col items-center justify-center gap-2 bg-black/45 rounded-sm z-10'; // audit-ignore: scrim over cover image, theme-independent
  overlay.setAttribute('role', 'status');
  overlay.setAttribute('aria-label', t(`manga.import_link.card.${status}.aria`));

  if (status === 'pending') {
    const spinner = document.createElement('div');
    spinner.className = 'w-10 h-10 border-[3px] border-white/30 border-t-white rounded-full animate-spin'; // audit-ignore: spinner on dark scrim, theme-independent
    overlay.appendChild(spinner);
  } else {
    const mark = document.createElement('span');
    mark.className = 'icon-md text-white/80'; // audit-ignore: mark on dark scrim, theme-independent
    mark.innerHTML = iconWarning;
    overlay.appendChild(mark);
  }

  const label = document.createElement('span');
  label.className = 'px-2 text-center text-2xs font-medium leading-tight text-white'; // audit-ignore: label on dark scrim, theme-independent
  label.textContent = t(`manga.import_link.card.${status}`);
  overlay.appendChild(label);

  coverWrap.appendChild(overlay);
}

/**
 * Adds or removes a scan-pending spinner overlay on a card within `root`.
 * @param {number} mangaId
 * @param {boolean} isScanning
 * @param {ParentNode} [root]
 */
export function setMangaCardScanning(mangaId, isScanning, root = document) {
  const card = root.querySelector(`[data-manga-id="${mangaId}"]`);
  if (!card) return;
  const coverWrap = card.querySelector('.relative');
  if (!coverWrap) return;
  const existing = coverWrap.querySelector('.js-scan-spinner');
  if (isScanning && !existing) {
    const overlay = document.createElement('div');
    overlay.className = 'js-scan-spinner absolute inset-0 flex items-center justify-center bg-black/40 rounded-md z-10'; // audit-ignore: scrim over cover image, theme-independent
    const spinner = document.createElement('div');
    spinner.className = 'w-10 h-10 border-[3px] border-white/30 border-t-white rounded-full animate-spin'; // audit-ignore: spinner on dark scrim, theme-independent
    overlay.appendChild(spinner);
    coverWrap.appendChild(overlay);
  } else if (!isScanning && existing) {
    existing.remove();
  }
}

/**
 * Sets or removes a "new chapters" badge count on a card.
 * Pass `count = 0` to remove the badge.
 * @param {number} mangaId
 * @param {number} count
 * @param {ParentNode} [root]
 */
export function setNewChapterCount(mangaId, count, root = document) {
  const card = root.querySelector(`[data-manga-id="${mangaId}"]`);
  if (!card) return;
  const coverWrap = card.querySelector('.relative');
  if (!coverWrap) return;
  let badge = /** @type {HTMLElement | null} */ (coverWrap.querySelector('.js-new-ch-badge'));
  if (count <= 0) {
    badge?.remove();
    return;
  }
  if (!badge) {
    badge = document.createElement('div');
    badge.className = 'js-new-ch-badge absolute top-1 left-1 z-10 min-w-[1.25rem] h-5 px-1 flex items-center justify-center rounded-full text-[10px] font-bold bg-accent text-on-accent leading-none';
    coverWrap.appendChild(badge);
  }
  badge.textContent = count > 99 ? '99+' : String(count);
}

/**
 * Updates the download progress bar overlay on a card within `root`.
 * Pass `pct = null` to remove the bar.
 * @param {number} mangaId
 * @param {number | null} pct  0–100
 * @param {ParentNode} [root]
 */
export function setMangaCardDownloadProgress(mangaId, pct, root = document) {
  const card = root.querySelector(`[data-manga-id="${mangaId}"]`);
  if (!card) return;
  const coverWrap = card.querySelector('.relative');
  if (!coverWrap) return;
  let bar = /** @type {HTMLElement | null} */ (coverWrap.querySelector('.js-dl-bar'));
  if (pct === null) {
    bar?.remove();
    return;
  }
  if (!bar) {
    bar = document.createElement('div');
    bar.className = 'js-dl-bar absolute bottom-0 left-0 right-0 h-1 bg-black/30 z-10'; // audit-ignore: track scrim over cover image
    const fill = document.createElement('div');
    fill.className = 'js-dl-bar-fill h-full bg-accent transition-[width] duration-300';
    bar.appendChild(fill);
    coverWrap.appendChild(bar);
  }
  const fill = /** @type {HTMLElement} */ (bar.querySelector('.js-dl-bar-fill'));
  fill.style.width = `${pct}%`;
}

/**
 * Renders a grid of manga cards into a container.
 * @param {HTMLElement} container
 * @param {{
 *   items: MangaCardData[],
 *   getHref: (manga: MangaCardData) => string,
 *   getBadge?: (manga: MangaCardData) => string | null,
 *   getCaption?: ((manga: MangaCardData) => string | null) | null,
 *   eagerCount?: number,
 *   onCardClick?: ((manga: MangaCardData) => void) | null,
 *   onMenuClick?: ((manga: MangaCardData, btnEl: HTMLElement) => void) | null,
 *   onResumeClick?: ((manga: MangaCardData) => void) | null,
 * }} props
 */
/**
 * Wraps a card so a caption can sit beneath it in the same grid cell. Returns
 * the card untouched when there is nothing to say.
 *
 * @param {HTMLElement} card
 * @param {string | null | undefined} caption
 * @returns {HTMLElement}
 */
export function mangaGridCell(card, caption) {
  if (!caption) return card;
  const cell = document.createElement('div');
  cell.className = 'flex flex-col gap-1 min-w-0';
  cell.appendChild(card);
  const note = document.createElement('p');
  note.className = 'text-2xs text-text-muted truncate';
  note.textContent = caption;
  note.title = caption;
  cell.appendChild(note);
  return cell;
}

export function renderMangaGrid(container, { items, getHref, getBadge, getCaption = null, onCardClick = null, onMenuClick = null, onResumeClick = null, eagerCount = 16 }) {
  const grid = document.createElement('div');
  grid.className = 'manga-grid';

  for (let i = 0; i < items.length; i++) {
    const manga = items[i];
    const card = createMangaCard({
      manga,
      href: getHref(manga),
      badge: getBadge ? getBadge(manga) : null,
      eager: i < eagerCount,
      onCardClick: onCardClick ? () => onCardClick(manga) : null,
      onMenuClick: onMenuClick ? (m, btn) => onMenuClick(m, btn) : null,
      onResumeClick: onResumeClick ? () => onResumeClick(manga) : null,
    });
    grid.appendChild(mangaGridCell(card, getCaption ? getCaption(manga) : null));
  }

  container.appendChild(grid);
  return grid;
}
