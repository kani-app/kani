// @ts-check
// Data-driven reader side-panel accordions: dual-scanlator compare, bookmarks,
// and the per-chapter note. Each owns its own async loading + signal state and
// renders a PanelAccordion island into the panel scroller.

import { h, render } from 'preact';
import { signal, effect } from '@preact/signals';
import { t } from '../../i18n.js';
import * as api from '../../api.js';
import { debounce } from '../../utils.js';
import { PanelAccordion, NoteBody, BookmarksBody, ScanlatorBody } from '../../components/reader/panel-sections.js';

/**
 * @param {{
 *   panelScroll: HTMLElement,
 *   data: any,
 *   chapterId: number,
 *   mangaId: number|null,
 *   state: { pages: string[], currentPage: number },
 *   engine: { render: () => void },
 *   closePanel: () => void,
 *   panelOpenCallbacks: Array<() => void>,
 *   cleanup: Array<() => void>,
 * }} deps
 */
export function mountPanelData({ panelScroll, data, chapterId, mangaId, state, engine, closePanel, panelOpenCallbacks, cleanup }) {
  const alts = data?.scanlator_alternatives ?? [];
  if (alts.length > 0) {
    const _primaryPages = state.pages.slice();
    const _primaryChId  = chapterId;

    /** @typedef {{ chId: number, scanlator: string|null, volume: number|null }} AltEntry */
    /** @type {AltEntry[]} */
    const _allEntries = [
      { chId: _primaryChId, scanlator: data?.scanlator ?? null, volume: data?.chapter_number ?? null },
      ...alts.map((/** @type {any} */ a) => ({ chId: a.chapter_id, scanlator: a.scanlator ?? null, volume: a.volume ?? null })),
    ];

    /**
     * Human-readable, collision-free label. Duplicate scanlator strings
     * (incl. null→"Unknown") get a volume suffix, then a numeric ID as last resort.
     * @param {AltEntry} entry @returns {string}
     */
    function _scanlatorLabel(entry) {
      const base = entry.scanlator ?? t('reader.scanlator.unknown');
      const sameBase = _allEntries.filter(e => e.scanlator === entry.scanlator);
      if (sameBase.length === 1) return base;
      const sameVol = sameBase.filter(e => e.volume === entry.volume);
      if (entry.volume != null && sameVol.length === 1) return `${base} (Vol. ${entry.volume})`;
      return `${base} (#${entry.chId})`;
    }

    /** @type {Map<number, string[]>} */
    const _pageCache = new Map([[_primaryChId, _primaryPages]]);
    const _scanlatorOptions = _allEntries.map(entry => ({
      value: String(entry.chId),
      label: _scanlatorLabel(entry) + (entry.chId === _primaryChId ? ` (${t('reader.scanlator.current')})` : ''),
    }));
    const selectedSignal = signal(String(_primaryChId));
    const busySignal = signal(false);
    const _onScanlator = async (/** @type {string} */ val) => {
      busySignal.value = true;
      selectedSignal.value = val;
      const chId = Number(val);
      try {
        if (!_pageCache.has(chId)) {
          const d = await api.getChapterPages(chId);
          _pageCache.set(chId, (d?.pages ?? []).map((/** @type {any} */ p) => api.getChapterPageUrl(chId, p.index)));
        }
        state.pages = /** @type {string[]} */ (_pageCache.get(chId)).slice();
        state.currentPage = Math.min(state.currentPage, Math.max(0, state.pages.length - 1));
        engine.render();
      } catch {
        const active = [..._pageCache.entries()].find(([, v]) => v === state.pages)?.[0] ?? _primaryChId;
        selectedSignal.value = String(active);
      } finally {
        busySignal.value = false;
      }
    };

    const container = document.createElement('div');
    panelScroll.appendChild(container);
    cleanup.push(effect(() => render(h(PanelAccordion, { title: t('reader.panel.scanlators') },
      h(ScanlatorBody, { options: _scanlatorOptions, selected: selectedSignal.value, disabled: busySignal.value, onChange: _onScanlator })
    ), container)));
  }

  if (mangaId) {
    const bookmarksSignal = signal(/** @type {number[]} */ ([]));
    // Bumped on panel open so the Add/Remove label re-evaluates against the
    // current page (which may have changed while the panel was closed).
    const bumpSignal = signal(0);

    const _onToggle = async () => {
      try {
        const res = await api.toggleBookmark(chapterId, state.currentPage);
        const set = new Set(bookmarksSignal.value);
        if (res.bookmarked) set.add(state.currentPage); else set.delete(state.currentPage);
        bookmarksSignal.value = [...set].sort((a, b) => a - b);
      } catch { }
    };
    const _onJump = (/** @type {number} */ pg) => { state.currentPage = pg; engine.render(); closePanel(); };

    api.getBookmarks(chapterId).then((/** @type {number[]} */ pages) => {
      bookmarksSignal.value = [...pages].sort((a, b) => a - b);
    }).catch(() => {});

    panelOpenCallbacks.push(() => { bumpSignal.value++; });

    const container = document.createElement('div');
    panelScroll.appendChild(container);
    cleanup.push(effect(() => {
      void bumpSignal.value;
      const bms = bookmarksSignal.value;
      const addLabel = bms.includes(state.currentPage) ? t('reader.bookmark.remove') : t('reader.bookmark.add');
      render(h(PanelAccordion, { title: t('reader.panel.bookmarks') },
        h(BookmarksBody, { addLabel, onToggle: _onToggle, bookmarks: bms, onJump: _onJump })
      ), container);
    }));
  }

  {
    const noteSignal = signal('');
    const _saveNote = debounce(() => api.setChapterNote(chapterId, noteSignal.value).catch(() => {}), 1000);
    // Flush immediately on destroy so a note typed within the debounce window isn't lost.
    cleanup.push(() => {
      _saveNote.cancel();
      if (noteSignal.value) api.setChapterNote(chapterId, noteSignal.value).catch(() => {});
    });

    api.getChapterNote(chapterId).then((/** @type {any} */ res) => {
      if (res?.note) noteSignal.value = res.note;
    }).catch(() => {});

    const container = document.createElement('div');
    panelScroll.appendChild(container);
    cleanup.push(effect(() => render(h(PanelAccordion, { title: t('reader.panel.note') },
      h(NoteBody, { value: noteSignal.value, onInput: (/** @type {string} */ v) => { noteSignal.value = v; _saveNote(); } })
    ), container)));
  }
}
