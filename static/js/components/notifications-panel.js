// @ts-check

import { h, render } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';
import htm from 'htm';
import { getState, subscribe, setState, updateState } from '../cache.js';
import { navigate } from '../router.js';
import { iconBell, iconX, iconDownload, iconCheck, iconArrowUp } from '../icons.js';
import { Icon } from './icon.js';
import { t } from '../i18n.js';
import { useOutsideClose } from './popover.js';
const html = htm.bind(h);

/** @typedef {import('../cache.js').ScanNotification} ScanNotification */
/** @typedef {import('../cache.js').ChapterProgress} ChapterProgress */

function NotificationsPanel() {
  const [open, setOpen] = useState(false);
  const [notifications, setNotifications] = useState(/** @type {ScanNotification[]} */ ([]));
  const [activeDownloads, setActiveDownloads] = useState(0);
  const [failedDownloads, setFailedDownloads] = useState(0);
  const [completedDownloads, setCompletedDownloads] = useState(/** @type {ChapterProgress[]} */ ([]));
  const [upgrades, setUpgrades] = useState(0);
  const wrapRef = useRef(/** @type {HTMLDivElement | null} */ (null));

  useEffect(() => {
    setNotifications(getState('scanNotifications'));
    return subscribe('scanNotifications', (notifs) => setNotifications([...notifs]));
  }, []);

  useEffect(() => {
    setUpgrades(Number(getState('upgradesPending')) || 0);
    return subscribe('upgradesPending', (n) => setUpgrades(Number(n) || 0));
  }, []);

  useEffect(() => {
    function syncDownloads() {
      /** @type {Map<number, ChapterProgress>} */
      const map = getState('chaptersProgress');
      let active = 0, failed = 0;
      /** @type {ChapterProgress[]} */
      const completed = [];
      for (const e of map.values()) {
        if (e.status === 'in_progress') active++;
        else if (e.status === 'failed') failed++;
        else if (e.status === 'completed') completed.push(e);
      }
      setActiveDownloads(active);
      setFailedDownloads(failed);
      setCompletedDownloads(completed);
    }
    syncDownloads();
    return subscribe('chaptersProgress', syncDownloads);
  }, []);

  useOutsideClose(open, [wrapRef], () => setOpen(false));

  const chapterCount = notifications.reduce((sum, n) => sum + n.count, 0);
  const badgeCount = chapterCount + failedDownloads + completedDownloads.length + upgrades;

  function dismissAll() {
    setState('scanNotifications', []);
    setState('upgradesPending', 0);
    updateState('chaptersProgress', (map) => {
      const next = new Map(map);
      for (const [k, v] of next) {
        if (v.status === 'completed') next.set(k, { ...v, status: 'completed_hidden' });
      }
      return next;
    });
    setOpen(false);
  }

  function dismiss(/** @type {number} */ mangaId) {
    setState('scanNotifications', notifications.filter(n => n.mangaId !== mangaId));
  }

  function dismissCompletedDownload(/** @type {number} */ chapterId) {
    updateState('chaptersProgress', (map) => {
      const next = new Map(map);
      const entry = next.get(chapterId);
      if (entry) next.set(chapterId, { ...entry, status: 'completed_hidden' });
      return next;
    });
  }

  const hasAnyDismissable = notifications.length > 0 || completedDownloads.length > 0;

  /** @type {Array<{ type: 'download', dl: ChapterProgress } | { type: 'scan', n: ScanNotification }>} */
  const feed = [
    ...completedDownloads.map(dl => ({ type: /** @type {'download'} */ ('download'), dl })),
    ...notifications.map(n => ({ type: /** @type {'scan'} */ ('scan'), n })),
  ];

  return html`
    <div class="relative" ref=${wrapRef}>
      <button
        type="button"
        class=${'touch-min relative inline-flex items-center justify-center w-9 h-9 rounded-full transition-colors focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-bg focus-visible:outline-none ' + (badgeCount === 0 && activeDownloads === 0 ? 'text-text-muted hover:bg-surface-2' : 'text-accent hover:bg-accent/10')}
        aria-label=${badgeCount > 0 ? t('notifications.btn.unread', { count: badgeCount }) : t('notifications.btn.label')}
        aria-expanded=${open}
        onClick=${() => setOpen(v => !v)}
      >
        <${Icon} svg=${iconBell} />
        ${badgeCount > 0 && html`
          <span class="absolute -top-1 -right-1 min-w-5 h-5 px-1 flex items-center justify-center text-2xs font-bold bg-accent text-on-accent rounded-full">
            ${badgeCount}
          </span>
        `}
        ${badgeCount === 0 && activeDownloads > 0 && html`
          <span class="absolute -top-0.5 -right-0.5 w-2.5 h-2.5 rounded-full bg-accent animate-pulse"></span>
        `}
      </button>

      ${open && html`
        <div class="absolute top-full right-0 mt-2 w-80 bg-surface border border-border rounded-xl shadow-lg z-50 overflow-hidden flex flex-col">

          <!-- Active downloads row -->
          ${activeDownloads > 0 && html`
            <a
              href="/downloads"
              class="flex items-center gap-3 px-4 py-3 border-b border-border hover:bg-surface-2 transition-colors shrink-0"
              onClick=${() => setOpen(false)}
            >
              <span class="shrink-0 icon-sm text-accent"><${Icon} svg=${iconDownload} /></span>
              <span class="flex-1 text-sm text-text">
                <strong>${activeDownloads}</strong> ${t('notifications.active.text', { s: activeDownloads !== 1 ? 's' : '' })}
              </span>
              <span class="text-xs text-accent shrink-0">${t('notifications.active.view')}</span>
            </a>
          `}

          <!-- Upgrades found by a scan -->
          ${upgrades > 0 && html`
            <a
              href="/upgrades"
              class="flex items-center gap-3 px-4 py-3 border-b border-border hover:bg-surface-2 transition-colors shrink-0"
              onClick=${() => { setState('upgradesPending', 0); setOpen(false); }}
            >
              <span class="shrink-0 icon-sm text-accent"><${Icon} svg=${iconArrowUp} /></span>
              <span class="flex-1 text-sm text-text">
                <strong>${upgrades}</strong> ${t('notifications.upgrades.text')}
              </span>
              <span class="text-xs text-accent shrink-0">${t('notifications.upgrades.view')}</span>
            </a>
          `}

          <!-- Failed downloads row -->
          ${failedDownloads > 0 && html`
            <a
              href="/downloads"
              class="flex items-center gap-3 px-4 py-3 border-b border-border hover:bg-surface-2 transition-colors shrink-0"
              onClick=${() => setOpen(false)}
            >
              <span class="shrink-0 icon-sm text-danger"><${Icon} svg=${iconDownload} /></span>
              <span class="flex-1 text-sm text-text">
                <strong class="text-danger">${failedDownloads}</strong> ${t('notifications.failed.text', { s: failedDownloads !== 1 ? 's' : '' })}
              </span>
              <span class="text-xs text-accent shrink-0">${t('notifications.active.view')}</span>
            </a>
          `}

          <!-- Merged notification feed -->
          ${feed.length === 0
            ? html`<p class="p-4 text-sm text-text-muted">${t('notifications.empty')}</p>`
            : html`
              <ul class="max-h-80 overflow-y-auto divide-y divide-border-subtle">
                ${feed.map(item => {
                  if (item.type === 'download') {
                    const dl = item.dl;
                    return html`
                      <li key=${'dl-' + dl.id} class="flex items-center gap-2 px-4 py-2.5">
                        <span class="shrink-0 text-success icon-xs"><${Icon} svg=${iconCheck} /></span>
                        <div class="flex-1 min-w-0">
                          <p class="text-2xs font-semibold uppercase tracking-wide text-text-muted mb-0.5">${t('notifications.chapter_downloaded.header')}</p>
                          ${dl.mangaId > 0 && html`
                            <a
                              href=${'/manga/' + dl.mangaId}
                              class="text-xs text-text-muted truncate block hover:text-accent transition-colors"
                              onClick=${(/** @type {MouseEvent} */ e) => { e.preventDefault(); setOpen(false); navigate('/manga/' + dl.mangaId); }}
                            >${dl.mangaTitle || t('notifications.manga_fallback')}</a>
                          `}
                          <a
                            href=${'/reader/' + dl.id}
                            class="text-sm text-text truncate block hover:text-accent transition-colors"
                            onClick=${(/** @type {MouseEvent} */ e) => { e.preventDefault(); setOpen(false); navigate('/reader/' + dl.id); }}
                          >${dl.name}</a>
                        </div>
                        <button
                          type="button"
                          class="btn-icon w-6 h-6 shrink-0"
                          aria-label=${t('notifications.dismiss')}
                          onClick=${() => dismissCompletedDownload(dl.id)}
                        ><${Icon} svg=${iconX} /></button>
                      </li>
                    `;
                  } else {
                    const n = item.n;
                    return html`
                      <li key=${'scan-' + n.mangaId} class="flex flex-col gap-1 px-4 py-3">
                        <div class="flex items-center gap-3">
                          <span class="shrink-0 text-accent icon-xs"><${Icon} svg=${iconBell} /></span>
                          <div class="flex-1 min-w-0">
                            <p class="text-2xs font-semibold uppercase tracking-wide text-text-muted mb-0.5">${t('notifications.new_chapters.header', { s: n.count !== 1 ? 's' : '' })}</p>
                            <a
                              href=${'/manga/' + n.mangaId}
                              class="text-sm font-medium text-text truncate block hover:text-accent transition-colors"
                              onClick=${() => setOpen(false)}
                            >${n.mangaName}</a>
                          </div>
                          <span class="text-xs text-text-muted whitespace-nowrap shrink-0">${t('notifications.new_chapters.count', { count: n.count })}</span>
                          <button type="button" class="btn-icon w-7 h-7 shrink-0" aria-label=${t('notifications.dismiss')} onClick=${() => dismiss(n.mangaId)}><${Icon} svg=${iconX} /></button>
                        </div>
                        ${n.chapterNames?.length > 0 && html`
                          <div class="flex flex-col gap-0.5 pl-6">
                            ${n.chapterNames.slice(0, 3).map(name => html`
                              <span class="text-xs text-text-muted truncate">${name}</span>
                            `)}
                            ${n.chapterNames.length > 3 && html`
                              <span class="text-xs text-text-faint">${t('notifications.more', { n: n.chapterNames.length - 3 })}</span>
                            `}
                          </div>
                        `}
                      </li>
                    `;
                  }
                })}
              </ul>
            `
          }

          <!-- Single dismiss-all button at bottom -->
          ${hasAnyDismissable && html`
            <div class="px-4 py-2.5 border-t border-border-subtle shrink-0">
              <button type="button" class="btn-ghost btn-sm w-full" onClick=${dismissAll}>
                ${t('notifications.dismiss_all')}
              </button>
            </div>
          `}
        </div>
      `}
    </div>
  `;
}

/**
 * Mount the notifications panel into a container.
 * @param {HTMLElement} container
 */
export function mountNotificationsPanel(container) {
  render(html`<${NotificationsPanel} />`, container);
}
