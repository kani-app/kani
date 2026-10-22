
import * as api from '../../api.js';
import { t } from '../../i18n.js';
import { setLocalJson } from '../../utils.js';
import { createEmptyState } from '../../components/empty-state.js';
import { iconChevronLeft, iconChevronRight } from '../../icons.js';
import { zoomStep, clampPan, ZOOM_MIN, ZOOM_MAX } from './zoom.js';
import { isWideImage, spreadPairVerdict, edgeMatchResult } from './spread-detect.js';
import { cropStyles, croppedWidth, croppedHeight, cropSourceRect } from './crop.js';
import { pagesPerMinute, minutesRemaining, adaptivePreloadCount, preloadThreshold } from './preload.js';
import { finishChapter } from './chapter-end.js';

/**
 * @typedef {import('../../reader-prefs.js').ReaderPrefs} ReaderPrefs
 * @typedef {{
 *   pages: string[], currentPage: number,
 *   mode: import('../../reader-prefs.js').ReadingMode,
 *   smoothScroll: boolean, doublePage: boolean,
 *   direction: 'rtl'|'ltr', fit: 'both'|'width'|'height',
 *   autoSpread: boolean,
 *   imgDims: Map<number, {w: number, h: number, edgeMatch?: boolean|null}>,
 *   lastLayoutPage: number, hasServerAnalysis: boolean,
 *   serverDoublePages: Set<number>,
 *   chapterInfo: { prev_chapter_id?: number|null, next_chapter_id?: number|null },
 *   scrollObs: IntersectionObserver|null,
 *   loaded: Set<number>, failed: Set<number>,
 *   cpTrack: HTMLElement|null, preloadDone: boolean,
 *   cachedPreloadN: number|null,
 * }} ReaderState
 * @typedef {{
 *   pagesEl: HTMLElement,
 *   canvasEl: HTMLElement,
 *   tintOverlay: HTMLElement,
 *   readerRoot: HTMLElement,
 *   pageNumOverlay: HTMLElement,
 *   state: ReaderState,
 *   chapterId: number,
 *   getPrefs: () => ReaderPrefs | null,
 *   reportProgress: () => void,
 *   navigateChapter: (chId: number, targetPage?: number) => void,
 *   navigateToManga: () => void,
 * }} ReaderEngineDeps
 */

export function createReaderEngine(/** @type {ReaderEngineDeps} */ deps) {
  const {
    pagesEl, canvasEl, tintOverlay, readerRoot, pageNumOverlay,
    state, chapterId, getPrefs, reportProgress, navigateChapter, navigateToManga,
  } = deps;

  /** @type {Array<() => void>} */
  const _cleanup = [];

  // audit-ignore-file: reader presentation palette source.
  const BG_MAP = /** @type {Record<string,string>} */ ({ black: '#000', white: '#fff', sepia: '#f5e6c8' }); // audit-ignore: reader palette source


  /** Apply CSS filter + background colour from prefs to the page container. */
  function applyPresentation() {
    const prefs = getPrefs();
    if (!prefs) return;
    const { brightness: br, contrast: co, saturation: sa, grayscale: gs, invert: inv, bg, bgTintPage } = prefs;
    const needsFilter = br !== 100 || co !== 100 || sa !== 100 || gs || inv;
    pagesEl.style.filter = needsFilter
      ? [`brightness(${br}%)`, `contrast(${co}%)`, `saturate(${sa}%)`,
         gs ? 'grayscale(1)' : '', inv ? 'invert(1)' : ''].filter(Boolean).join(' ')
      : '';
    const bgColor = BG_MAP[bg] ?? '#000'; // audit-ignore: reader background fallback
    readerRoot.style.backgroundColor = bgColor;
    // The isolated page blend context needs an opaque backdrop for tint multiplication.
    canvasEl.style.backgroundColor = bgColor;
    pagesEl.style.mixBlendMode = (bgTintPage && bg !== 'black') ? 'multiply' : '';
  }

  /** Apply a semi-transparent colour overlay with the chosen blend mode to the page area. */
  function applyTint() {
    const prefs = getPrefs();
    if (!prefs) return;
    const { tintOpacity: op, tintColor: col, tintBlend: blend } = prefs;
    if (!op) { tintOverlay.style.display = 'none'; return; }
    const r = parseInt(col.slice(1, 3), 16);
    const g = parseInt(col.slice(3, 5), 16);
    const b = parseInt(col.slice(5, 7), 16);
    tintOverlay.style.display         = '';
    tintOverlay.style.backgroundColor = `rgba(${r},${g},${b},${op / 100})`; // audit-ignore: built from user-chosen tint colour
    tintOverlay.style.mixBlendMode    = blend;
  }

  /** Show/update the floating page-number overlay from prefs + current page. */
  function updatePageOverlay() {
    const prefs = getPrefs();
    const on = prefs?.pageOverlay ?? false;
    if (!on || state.pages.length === 0) { pageNumOverlay.style.display = 'none'; return; }
    pageNumOverlay.style.display = '';
    const span = pageNumOverlay.querySelector('span');
    if (span) span.textContent = `${state.currentPage + 1} / ${state.pages.length}`;
  }


  let _dimSaveTimer = /** @type {ReturnType<typeof setTimeout>|null} */ (null);

  function _saveDims() {
    if (_dimSaveTimer) clearTimeout(_dimSaveTimer);
    _dimSaveTimer = setTimeout(() => {
      const entries = [];
      for (const [i, d] of state.imgDims) entries.push([i, d.w, d.h]);
      setLocalJson(`kani_dims_${chapterId}`, entries);
    }, 500);
  }
  _cleanup.push(() => { if (_dimSaveTimer) clearTimeout(_dimSaveTimer); });

  function _setDims(idx, w, h) {
    const prev = state.imgDims.get(idx);
    state.imgDims.set(idx, { w, h, edgeMatch: prev?.edgeMatch });
    _saveDims();
  }


  /**
   * Apply percentage-based clip-path to remove border strips from an image.
   * Crop values are 0–50 (percent of the element's own dimension per side).
   *
   * clip-path inset() % are relative to the element's own box: correct.
   * margin % is always relative to the containing block's WIDTH (even for top/bottom),
   * which equals the image width in our layouts.  For left/right that's correct.
   * For top/bottom it's an approximation: on a 2:3 manga page 1% width ≈ 0.67% height,
   * so we scale by the natural aspect ratio (h/w) when dimensions are known.
   */
  function _applyCropToImg(/** @type {HTMLElement} */ el) {
    const imgEl = /** @type {HTMLImageElement} */ (el);
    const nw = imgEl.naturalWidth  || 0;
    const nh = imgEl.naturalHeight || 0;
    const ratio = (nw > 0 && nh > 0) ? nh / nw : 1.5;
    const styles = cropStyles(getPrefs() ?? /** @type {any} */ ({}), ratio);
    if (!styles) {
      el.style.clipPath    = '';
      el.style.marginTop   = el.style.marginBottom = '';
      el.style.marginLeft  = el.style.marginRight  = '';
      return;
    }
    el.style.clipPath     = styles.clipPath;
    el.style.marginTop    = styles.marginTop;
    el.style.marginBottom = styles.marginBottom;
    el.style.marginLeft   = styles.marginLeft;
    el.style.marginRight  = styles.marginRight;
  }

  /**
   * Re-apply crop styles to all currently visible images in-place.
   * Used by crop sliders to avoid a full DOM re-render (which causes flicker).
   * Canvas composites (spread pages) are NOT updated here — those still need render().
   */
  function _applyCropToAllImages() {
    pagesEl.querySelectorAll('img').forEach(img => _applyCropToImg(/** @type {HTMLImageElement} */ (img)));
  }

  /** Cropped natural width of an image element after applying percentage prefs. */
  const _cW = (/** @type {HTMLImageElement} */ img) =>
    croppedWidth(img.naturalWidth, getPrefs()?.cropLeft ?? 0, getPrefs()?.cropRight ?? 0);
  /** Cropped natural height of an image element after applying percentage prefs. */
  const _cH = (/** @type {HTMLImageElement} */ img) =>
    croppedHeight(img.naturalHeight, getPrefs()?.cropTop ?? 0, getPrefs()?.cropBottom ?? 0);

  /**
   * Draw a page image onto a canvas at (dx, dy), honouring crop prefs
   * via the drawImage source-rect so the canvas bitmap is trimmed correctly.
   * @param {CanvasRenderingContext2D} ctx
   * @param {HTMLImageElement} img
   * @param {number} dx
   * @param {number} dy
   */
  function _drawPage(ctx, img, dx, dy) {
    const { sx, sy, sw, sh } = cropSourceRect(img.naturalWidth, img.naturalHeight, getPrefs() ?? /** @type {any} */ ({}));
    ctx.drawImage(img, sx, sy, sw, sh, dx, dy, sw, sh);
  }


  function _maybePreloadNext() {
    if (state.preloadDone || !state.chapterInfo.next_chapter_id) return;
    const threshold = preloadThreshold(state.mode, state.pages.length);
    if (state.currentPage < threshold) return;
    state.preloadDone = true;
    const nextId = state.chapterInfo.next_chapter_id;
    api.getChapterPages(nextId).then((data) => {
      if (!Array.isArray(data?.pages)) return;
      data.pages.slice(0, 3).forEach((p) => {
        const img = new Image();
        img.src = api.getChapterPageUrl(nextId, p.index);
      });
    }).catch(() => {});
  }


  /**
   * Returns true if pages `idxA` and `idxA+1` are a split double-page spread
   * (i.e. a single wide scan split into two portrait-oriented files). Both
   * image dimensions must already be known via `state.imgDims`.
   * @param {number} idxA
   */
  function _isSpreadPair(idxA) {
    if (!state.autoSpread) return false;
    const idxB = idxA + 1;
    if (idxB >= state.pages.length) return false;

    const a = state.imgDims.get(idxA);
    const b = state.imgDims.get(idxB);
    const verdict = spreadPairVerdict(a, b, {
      hasServerAnalysis: state.hasServerAnalysis,
      isServerDoubleA: state.serverDoublePages.has(idxA),
      edgeMatch: a ? a.edgeMatch : undefined,
    });
    if (verdict === 'pair') return true;
    if (verdict === 'needs-edge-check') {
      if (a) a.edgeMatch = null;
      _checkEdgeMatch(idxA);
    }
    return false;
  }

  /**
   * Samples a narrow vertical strip of pixels from the right edge of page `idxA`
   * and the left edge of page `idxA + 1`. If they're similar (low average diff),
   * the pages are halves of the same original scan and should be composited.
   * @param {number} idxA
   */
  async function _checkEdgeMatch(idxA) {
    const STRIP_W  = 32;
    const SAMPLE_H = 64;
    try {
      const [bmpA, bmpB] = await Promise.all([
        fetch(state.pages[idxA]).then(r => r.blob()).then(b => createImageBitmap(b)),
        fetch(state.pages[idxA + 1]).then(r => r.blob()).then(b => createImageBitmap(b)),
      ]);
      const [leftBmp, rightBmp] = state.direction === 'rtl' ? [bmpB, bmpA] : [bmpA, bmpB];
      const oc  = new OffscreenCanvas(STRIP_W * 2, SAMPLE_H);
      const ctx = /** @type {OffscreenCanvasRenderingContext2D} */ (oc.getContext('2d'));
      ctx.drawImage(leftBmp,  leftBmp.width  - STRIP_W, 0, STRIP_W, leftBmp.height,  0,       0, STRIP_W, SAMPLE_H);
      ctx.drawImage(rightBmp, 0,                         0, STRIP_W, rightBmp.height, STRIP_W, 0, STRIP_W, SAMPLE_H);
      const pxA = ctx.getImageData(0,       0, STRIP_W, SAMPLE_H).data;
      const pxB = ctx.getImageData(STRIP_W, 0, STRIP_W, SAMPLE_H).data;

      const current = state.imgDims.get(idxA);
      if (!current) return;
      const result = edgeMatchResult(pxA, pxB, { stripW: STRIP_W, sampleH: SAMPLE_H });
      current.edgeMatch = result.isMatch;
      if (result.flat) return;

      if (current.edgeMatch && state.mode === 'paged' && state.lastLayoutPage !== idxA) {
        if (state.currentPage === idxA) {
          state.lastLayoutPage = idxA;
          _renderPages();
        } else if (state.currentPage === idxA + 1) {
          state.currentPage    = idxA;
          state.lastLayoutPage = idxA;
          _renderPages();
        }
      }
    } catch {
      const current = state.imgDims.get(idxA);
      if (current) current.edgeMatch = false;
    }
  }

  /**
   * Returns true if page `idx` is a pre-combined wide spread (landscape orientation).
   * In double-page mode such pages are displayed alone rather than paired.
   * @param {number} idx
   */
  function _isWideImage(idx) {
    return isWideImage(state.imgDims.get(idx), {
      hasServerAnalysis: state.hasServerAnalysis,
      isServerDouble: state.serverDoublePages.has(idx),
    });
  }


  /**
   * Returns the page index of the next stop after `from` in paged mode.
   * Accounts for: first-page-alone, wide images, spread pairs.
   * @param {number} from
   */
  function _nextStop(from) {
    if (state.doublePage) {
      const spreadOffset = getPrefs()?.spreadOffset ?? false;
      if (!spreadOffset && from === 0) return 1;
      if (_isWideImage(from) || (from + 1 < state.pages.length && _isWideImage(from + 1)))
        return from + 1;
      return from + 2;
    }
    if (state.autoSpread && _isSpreadPair(from)) return from + 2;
    return from + 1;
  }

  /**
   * Returns the page index of the previous stop before `from` in paged mode.
   * Reconstructs the backward path by inverting `_nextStop`.
   * @param {number} from
   */
  function _prevStop(from) {
    for (let offset = 1; offset <= 2; offset++) {
      const c = from - offset;
      if (c >= 0 && _nextStop(c) === from) return c;
    }
    return from - 1;
  }


  const FETCH_WINDOW = 8;
  /** @type {number[]} */
  const _fetchMsLog = [];

  function _recordFetchMs(ms) {
    _fetchMsLog.push(ms);
    if (_fetchMsLog.length > FETCH_WINDOW) _fetchMsLog.shift();
    state.cachedPreloadN = null;
  }

  function _prefetch(pageIndex) {
    if (state.mode !== 'paged' && state.mode !== 'continuous-paged') return;
    const preloadN = _adaptivePreload();
    for (let i = 1; i <= preloadN; i++) {
      const prefIdx = pageIndex + i;
      const url = state.pages[prefIdx];
      if (url && !state.loaded.has(prefIdx) && !state.failed.has(prefIdx)) {
        const img = new Image();
        const fetchStart = performance.now();
        img.addEventListener('load', () => {
          _recordFetchMs(performance.now() - fetchStart);
          state.loaded.add(prefIdx); state.loadVersion++;
          if (img.naturalWidth > 0) {
            _setDims(prefIdx, img.naturalWidth, img.naturalHeight);
            if (state.autoSpread && state.mode === 'paged' && !state.doublePage &&
                prefIdx === state.currentPage + 1 && _isSpreadPair(state.currentPage) &&
                state.lastLayoutPage !== state.currentPage) {
              state.lastLayoutPage = state.currentPage;
              _renderPages();
            }
          }
        });
        img.addEventListener('error', () => { state.failed.add(prefIdx); state.loadVersion++; });
        img.src = url;
      }
    }
  }


  const PACE_WINDOW = 10;
  /** @type {{ time: number }[]} */
  const _paceLog = [];

  function _recordPace() {
    _paceLog.push({ time: Date.now() });
    if (_paceLog.length > PACE_WINDOW + 1) _paceLog.shift();
    state.cachedPreloadN = null;
  }

  /** Returns pages-per-minute rolling average, or null if not enough data. */
  function _ppm() {
    return pagesPerMinute(_paceLog.map(e => e.time));
  }

  function etaText() {
    if (state.pages.length === 0) return null;
    const mins = minutesRemaining(_ppm(), state.pages.length - 1 - state.currentPage);
    if (mins === null) return null;
    if (mins === 0) return '0 min';
    return mins < 1 ? '<1 min' : `~${Math.round(mins)} min`;
  }

  /**
   * Smart preload count: how many images can fully load within one
   * average page-read interval, clamped to [1, user max].
   * Falls back to the configured max when there isn't enough data yet.
   */
  function _adaptivePreload() {
    if (state.cachedPreloadN !== null) return state.cachedPreloadN;
    const max = getPrefs()?.preloadCount ?? 2;
    const ppm = _ppm();
    if (_fetchMsLog.length < 3 || !ppm) return max;
    state.cachedPreloadN = adaptivePreloadCount({ max, fetchMsLog: _fetchMsLog, ppm });
    return state.cachedPreloadN;
  }


  let _zoomScale = 1;
  let _zoomTx    = 0;
  let _zoomTy    = 0;

  /** Returns the element to zoom in paged mode; null otherwise (no-op).
   * continuous-paged is excluded because the track also owns the translateY snap —
   * both writes target style.transform and would race each other. */
  function _zoomTarget() {
    return state.mode === 'paged'
      ? /** @type {HTMLElement|null} */ (pagesEl.firstElementChild)
      : null;
  }

  function _applyZoom() {
    const t = _zoomTarget();
    if (!t) return;
    if (_zoomScale <= 1) {
      t.style.transform = '';
      t.style.transformOrigin = '';
      t.style.cursor = '';
    } else {
      t.style.transformOrigin = '0 0';
      t.style.transform = `translate(${_zoomTx}px,${_zoomTy}px) scale(${_zoomScale})`;
      t.style.cursor = 'grab';
    }
  }

  function resetZoom() {
    const t = _zoomTarget();
    _zoomScale = 1; _zoomTx = 0; _zoomTy = 0;
    if (t) { t.style.transform = ''; t.style.transformOrigin = ''; t.style.cursor = ''; }
  }

  function _clampPan() {
    const rect = pagesEl.getBoundingClientRect();
    const c = clampPan(rect.width, rect.height, _zoomScale, _zoomTx, _zoomTy);
    _zoomTx = c.tx;
    _zoomTy = c.ty;
  }

  function zoomAt(factor, clientX, clientY) {
    const t = _zoomTarget();
    let cx, cy;
    if (t) {
      const r = t.getBoundingClientRect();
      cx = clientX - (r.left - _zoomTx);
      cy = clientY - (r.top  - _zoomTy);
    } else {
      const r = pagesEl.getBoundingClientRect();
      cx = clientX - r.left;
      cy = clientY - r.top;
    }
    const rect = pagesEl.getBoundingClientRect();
    const next = zoomStep(
      { scale: _zoomScale, tx: _zoomTx, ty: _zoomTy },
      factor, cx, cy,
      { min: ZOOM_MIN, max: ZOOM_MAX, viewportW: rect.width, viewportH: rect.height },
    );
    _zoomScale = next.scale;
    _zoomTx = next.tx;
    _zoomTy = next.ty;
    _applyZoom();
  }

  const isZoomed = () => _zoomScale > 1;


  let _touchStartX = 0;
  let _touchStartY = 0;
  let _pinchDist   = 0;
  let _panActive   = false;
  let _panStartTx  = 0;
  let _panStartTy  = 0;

  pagesEl.addEventListener('touchstart', (e) => {
    if (e.touches.length === 2) {
      e.preventDefault();
      const dx = e.touches[0].clientX - e.touches[1].clientX;
      const dy = e.touches[0].clientY - e.touches[1].clientY;
      _pinchDist = Math.hypot(dx, dy);
      return;
    }
    _touchStartX = e.touches[0].clientX;
    _touchStartY = e.touches[0].clientY;
    if (_zoomScale > 1) {
      e.preventDefault();
      _panActive  = true;
      _panStartTx = _zoomTx;
      _panStartTy = _zoomTy;
    }
  }, { passive: false });

  pagesEl.addEventListener('touchmove', (e) => {
    if (e.touches.length === 2) {
      e.preventDefault();
      const dx = e.touches[0].clientX - e.touches[1].clientX;
      const dy = e.touches[0].clientY - e.touches[1].clientY;
      const newDist = Math.hypot(dx, dy);
      if (_pinchDist > 0 && newDist > 0) {
        const midX = (e.touches[0].clientX + e.touches[1].clientX) / 2;
        const midY = (e.touches[0].clientY + e.touches[1].clientY) / 2;
        zoomAt(newDist / _pinchDist, midX, midY);
      }
      _pinchDist = newDist;
      return;
    }
    if (_panActive && e.touches.length === 1) {
      e.preventDefault();
      _zoomTx = _panStartTx + (e.touches[0].clientX - _touchStartX);
      _zoomTy = _panStartTy + (e.touches[0].clientY - _touchStartY);
      _clampPan();
      _applyZoom();
    }
  }, { passive: false });

  pagesEl.addEventListener('touchend', (e) => {
    _pinchDist = 0;
    _panActive = false;
    if (_zoomScale > 1) return;

    const dx = e.changedTouches[0].clientX - _touchStartX;
    const dy = e.changedTouches[0].clientY - _touchStartY;

    if (Math.abs(dx) > 50 && Math.abs(dx) > Math.abs(dy)) _goPage(dx < 0 ? 1 : -1);
  });

  // Paged + fit=both: plain wheel zooms. Paged + fit=width/height: plain wheel scrolls the
  // overflow axis; ctrl+wheel zooms. Scroll/webtoon: wheel always scrolls; ctrl+wheel zooms.
  // When already zoomed: wheel always zooms regardless of mode/fit.

  pagesEl.addEventListener('wheel', (e) => {
    const isPaged = state.mode === 'paged' || state.mode === 'continuous-paged';
    const alreadyZoomed = _zoomScale > 1;

    if (alreadyZoomed) {
      e.preventDefault();
      zoomAt(e.deltaY < 0 ? 1.1 : 0.9, e.clientX, e.clientY);
      return;
    }

    // ctrl+wheel always zooms (browser pinch-to-zoom emulation sends ctrlKey=true).
    if (e.ctrlKey) {
      e.preventDefault();
      zoomAt(e.deltaY < 0 ? 1.1 : 0.9, e.clientX, e.clientY);
      return;
    }

    // Plain wheel in paged+both: zoom in/out (no native scroll at zoom=1 in this mode).
    if (isPaged && state.fit === 'both') {
      e.preventDefault();
      zoomAt(e.deltaY < 0 ? 1.1 : 0.9, e.clientX, e.clientY);
      return;
    }
  }, { passive: false });


  let _mousePanActive = false;
  let _mousePanStartX = 0;
  let _mousePanStartY = 0;
  let _mousePanTx0    = 0;
  let _mousePanTy0    = 0;

  pagesEl.addEventListener('mousedown', (e) => {
    if (_zoomScale <= 1 || e.button !== 0) return;
    const target = /** @type {HTMLElement} */ (e.target);
    if (target.closest('button') || target.closest('a')) return;
    _mousePanActive = true;
    _mousePanStartX = e.clientX;
    _mousePanStartY = e.clientY;
    _mousePanTx0    = _zoomTx;
    _mousePanTy0    = _zoomTy;
    pagesEl.style.cursor = 'grabbing';
    e.preventDefault();
  });
  const _onMouseMove = (/** @type {MouseEvent} */ e) => {
    if (!_mousePanActive) return;
    _zoomTx = _mousePanTx0 + (e.clientX - _mousePanStartX);
    _zoomTy = _mousePanTy0 + (e.clientY - _mousePanStartY);
    _clampPan();
    _applyZoom();
  };
  const _onMouseUp = () => {
    if (!_mousePanActive) return;
    _mousePanActive = false;
    pagesEl.style.cursor = '';
    _applyZoom();
  };
  document.addEventListener('mousemove', _onMouseMove);
  document.addEventListener('mouseup',   _onMouseUp);
  _cleanup.push(() => {
    document.removeEventListener('mousemove', _onMouseMove);
    document.removeEventListener('mouseup',   _onMouseUp);
  });


  /** Returns CSS classes for page images given current fit mode and layout context. */
  function _imgClass(ctx = 'scroll') {
    if (ctx === 'scroll') {
      if (state.fit === 'height') return 'max-h-screen w-auto';
      if (state.fit === 'width')  return 'max-w-full h-auto';
      return 'max-w-full max-h-screen object-contain';
    }
    if (ctx === 'paged-single') {
      if (state.fit === 'height') return 'max-h-full w-auto';
      if (state.fit === 'width')  return 'max-w-full h-auto';
      return 'max-w-full max-h-full object-contain';
    }
    if (state.fit === 'height') return 'max-h-full w-auto';
    if (state.fit === 'width')  return 'max-w-[50vw] max-h-full';
    return 'max-w-[50vw] max-h-full object-contain';
  }

  /** CSS classes for a spread canvas (already a bitmap — no object-contain). */
  function _spreadClass() {
    if (state.fit === 'height') return 'max-h-full w-auto';
    if (state.fit === 'width')  return 'max-w-full h-auto';
    return 'max-w-full max-h-full';
  }

  /**
   * CSS classes for the paged/continuous-paged outer container.
   * Enables native scroll on the non-constrained axis in fit-width/height modes.
   */
  function _pagedContainerClass() {
    const overflow = state.fit === 'width'  ? 'overflow-y-auto overflow-x-hidden'
                   : state.fit === 'height' ? 'overflow-x-auto overflow-y-hidden'
                   : 'overflow-hidden';
    // fit-width: align to top so the image starts at scrollTop=0 (items-center would push
    // the top of a tall image above the scroll origin, making it unreachable).
    const align = state.fit === 'width' ? 'items-start' : 'items-center';
    return `flex-1 ${overflow} ${align} relative flex justify-center`;
  }

  /**
   * Build the end-of-chapter card. Shared by scroll mode (appended after the
   * last page) and paged/continuous modes (shown as a final screen when the
   * endCardInPaged pref is on). @param {boolean} inPaged
   */
  function _buildEndCard(inPaged) {
    const card = document.createElement('div');
    card.className = inPaged
      ? 'flex-1 min-h-full w-full flex flex-col items-center justify-center gap-5 py-16'
      : 'w-full shrink-0 flex flex-col items-center justify-center gap-5 py-16';
    const hasNext = !!state.chapterInfo.next_chapter_id;
    const hasPrev = !!state.chapterInfo.prev_chapter_id;
    card.innerHTML = `
      <div class="flex flex-col items-center gap-1">
        <p class="text-text text-base font-medium">${t('reader.end.chapter')}</p>
        <p class="text-muted text-xs">${t('reader.end.finished')}</p>
      </div>
      <div class="flex flex-col items-stretch gap-2 w-full max-w-xs px-6">
        ${hasNext ? `<button data-act="next" class="btn-primary flex items-center justify-center gap-1">${t('reader.end.next_chapter')} ${iconChevronRight}</button>` : ''}
        ${hasPrev ? `<button data-act="prev" class="btn-secondary flex items-center justify-center gap-1">${iconChevronLeft} ${t('reader.end.prev_chapter')}</button>` : ''}
        <button data-act="manga" class="btn-secondary flex items-center justify-center gap-1">${t('reader.back_to_manga')}</button>
        ${inPaged ? `<button data-act="reread" class="btn-secondary text-sm flex items-center justify-center gap-1">${iconChevronLeft} ${t('reader.end.last_page')}</button>` : ''}
      </div>`;
    card.querySelector('[data-act="next"]')?.addEventListener('click', () => { if (state.chapterInfo.next_chapter_id) navigateChapter(state.chapterInfo.next_chapter_id); });
    card.querySelector('[data-act="prev"]')?.addEventListener('click', () => { if (state.chapterInfo.prev_chapter_id) navigateChapter(state.chapterInfo.prev_chapter_id); });
    card.querySelector('[data-act="manga"]')?.addEventListener('click', () => navigateToManga());
    card.querySelector('[data-act="reread"]')?.addEventListener('click', () => { state.currentPage = state.pages.length - 1; _renderPages(); });
    return card;
  }

  /** Replace the page surface with the end card (paged/continuous end-of-chapter). */
  function _showPagedEndCard() {
    resetZoom();
    pagesEl.innerHTML = '';
    pagesEl.style.transform = '';
    pagesEl.appendChild(_buildEndCard(true));
    state.loadVersion++;
  }


  function _renderContinuousPaged() {
    const preload = _adaptivePreload();
    const windowStart = Math.max(0, state.currentPage - preload);
    const windowEnd   = Math.min(state.pages.length - 1, state.currentPage + preload);

    pagesEl.className = _pagedContainerClass();

    const track = document.createElement('div');
    track.className = 'absolute inset-0 flex flex-col items-center reader-cp-track';
    track.style.willChange = 'transform';
    state.cpTrack = track;

    for (let i = windowStart; i <= windowEnd; i++) {
      const slot = document.createElement('div');
      slot.className = 'w-full flex-shrink-0 flex items-center justify-center';
      slot.style.height = '100%';

      const img = document.createElement('img');
      img.src           = state.pages[i] ?? '';
      img.className     = _imgClass('paged-single');
      img.style.aspectRatio = '2/3';
      img.alt           = `Page ${i + 1}`;
      img.dataset.index = String(i);
      const _i = i;
      img.addEventListener('load', () => {
        img.style.aspectRatio = '';
        _applyCropToImg(img);
        state.loaded.add(_i); state.failed.delete(_i);
        if (img.naturalWidth > 0) _setDims(_i, img.naturalWidth, img.naturalHeight);
        state.loadVersion++;
      });
      img.addEventListener('error', () => { state.failed.add(_i); state.loaded.delete(_i); state.loadVersion++; });
      if (img.complete && img.naturalWidth) {
        img.style.aspectRatio = '';
        state.loaded.add(i); _setDims(i, img.naturalWidth, img.naturalHeight);
      }
      _applyCropToImg(img);
      slot.appendChild(img);
      track.appendChild(slot);
    }

    pagesEl.appendChild(track);
    _cpSnapToPage(state.currentPage, windowStart);
  }

  function _cpSnapToPage(pageIdx, windowStart) {
    if (!state.cpTrack) return;
    const offset = pageIdx - (windowStart ?? state.currentPage - (getPrefs()?.preloadCount ?? 2));
    const h = pagesEl.getBoundingClientRect().height || window.innerHeight;
    state.cpTrack.style.transform = `translateY(${-offset * h}px)`;
  }

  function _renderPages() {
    state.cpTrack = null;
    if (state.scrollObs) { state.scrollObs.disconnect(); state.scrollObs = null; }
    resetZoom();
    pagesEl.innerHTML = '';

    if (state.pages.length === 0) {
      pagesEl.className = 'flex-1 overflow-y-auto overflow-x-hidden flex flex-col items-center';
      const emptyWrap = document.createElement('div');
      emptyWrap.className = 'flex items-center justify-center min-h-full';
      emptyWrap.appendChild(createEmptyState({ title: t('reader.empty.title'), subtitle: t('reader.empty.subtitle') }));
      pagesEl.appendChild(emptyWrap);
      state.loadVersion++;
      return;
    }

    const _isScrollLike = state.mode === 'scroll' || state.mode === 'webtoon';

    if (_isScrollLike) {
      pagesEl.className = state.mode === 'webtoon'
        ? 'flex-1 overflow-y-auto overflow-x-hidden flex flex-col items-center'
        : 'flex-1 overflow-y-auto overflow-x-hidden flex flex-col items-center gap-1 py-2';

      /** @type {Map<number, HTMLImageElement>} */
      const _scrollImgs = new Map();

      /**
       * If pages idxA and idxA+1 are a spread pair and both are already loaded,
       * replace both img elements with a single composited canvas in-place.
       * @param {number} idxA
       */
      const _maybeComposite = (idxA) => {
        if (!_isSpreadPair(idxA)) return;
        const imgA = _scrollImgs.get(idxA);
        const imgB = _scrollImgs.get(idxA + 1);
        if (!imgA || !imgB || !imgA.parentElement) return;

        const [leftImg, rightImg] = state.direction === 'rtl' ? [imgB, imgA] : [imgA, imgB];
        const W = _cW(leftImg) + _cW(rightImg);
        const H = Math.max(_cH(leftImg), _cH(rightImg));
        const canvas = document.createElement('canvas');
        canvas.className   = _spreadClass();
        canvas.dataset.index = String(idxA);
        canvas.width  = W;
        canvas.height = H;
        const ctx = canvas.getContext('2d');
        if (ctx) {
          _drawPage(ctx, leftImg,  0,           (H - _cH(leftImg))  / 2);
          _drawPage(ctx, rightImg, _cW(leftImg), (H - _cH(rightImg)) / 2);
        }
        state.loaded.add(idxA); state.loaded.add(idxA + 1);

        state.scrollObs?.unobserve(imgA);
        state.scrollObs?.unobserve(imgB);
        imgA.replaceWith(canvas);
        imgB.remove();
        _scrollImgs.delete(idxA);
        _scrollImgs.delete(idxA + 1);
        state.scrollObs?.observe(canvas);
        state.loadVersion++;
      };

      for (let i = 0; i < state.pages.length; i++) {
        const img         = document.createElement('img');
        img.src           = state.pages[i];
        img.className     = _imgClass('scroll');
        img.alt           = '';
        img.loading       = 'lazy';
        img.style.aspectRatio = '2/3';
        img.dataset.index = String(i);
        const _i = i;
        img.addEventListener('load', () => {
          img.style.aspectRatio = '';
          _applyCropToImg(img);
          state.loaded.add(_i); state.failed.delete(_i);
          if (img.naturalWidth > 0) {
            _setDims(_i, img.naturalWidth, img.naturalHeight);
            if (state.autoSpread) {
              _maybeComposite(_i);
              if (_i > 0) _maybeComposite(_i - 1);
            }
          }
          state.loadVersion++;
        });
        img.addEventListener('error', () => { state.failed.add(_i); state.loaded.delete(_i); state.loadVersion++; });
        if (img.complete) {
          if (img.naturalWidth) { state.loaded.add(i); _setDims(i, img.naturalWidth, img.naturalHeight); }
          else state.failed.add(i);
        }
        _applyCropToImg(img);
        _scrollImgs.set(i, img);
        pagesEl.appendChild(img);
      }

      pagesEl.appendChild(_buildEndCard(false));

      /** @type {Set<number>} */
      const visible = new Set();
      state.scrollObs = new IntersectionObserver((entries) => {
        for (const e of entries) {
          const idx = Number(/** @type {HTMLElement} */ (e.target).dataset.index);
          if (!isNaN(idx)) {
            if (e.isIntersecting) visible.add(idx);
            else visible.delete(idx);
          }
        }
        if (visible.size > 0) {
          state.currentPage = Math.min(...visible);
          state.loadVersion++;
          reportProgress();
          _maybePreloadNext();
        }
      }, { root: pagesEl, threshold: state.mode === 'webtoon' ? 0.5 : 0.1 });
      pagesEl.querySelectorAll('[data-index]').forEach(el => state.scrollObs?.observe(el));

    } else {
      state.currentPage = Math.max(0, Math.min(state.pages.length - 1, state.currentPage));

      if (state.mode === 'continuous-paged') {
        _renderContinuousPaged();
        state.loadVersion++;
        return;
      }

      pagesEl.className = _pagedContainerClass();

      /** @param {number} pageIdx @param {string} altText @returns {HTMLImageElement} */
      function _makePageImg(pageIdx, altText) {
        const img     = document.createElement('img');
        img.src       = state.pages[pageIdx] ?? '';
        img.className = _imgClass(state.doublePage ? 'paged-double' : 'paged-single');
        img.style.aspectRatio = '2/3';
        img.alt       = altText;
        img.addEventListener('load', () => {
          img.style.aspectRatio = '';
          _applyCropToImg(img);
          state.loaded.add(pageIdx); state.failed.delete(pageIdx);
          if (img.naturalWidth > 0) {
            _setDims(pageIdx, img.naturalWidth, img.naturalHeight);
            if (state.lastLayoutPage !== state.currentPage) {
              if (state.autoSpread && _isSpreadPair(state.currentPage)) {
                state.lastLayoutPage = state.currentPage;
                _renderPages();
                return;
              }
              if (state.doublePage && (_isWideImage(state.currentPage) || _isWideImage(state.currentPage + 1))) {
                state.lastLayoutPage = state.currentPage;
                _renderPages();
                return;
              }
            }
          }
          state.loadVersion++;
        });
        img.addEventListener('error', () => {
          state.failed.add(pageIdx); state.loaded.delete(pageIdx); state.loadVersion++;
          const err = document.createElement('div');
          err.className = 'absolute inset-0 flex flex-col items-center justify-center gap-3 pointer-events-none';
          err.innerHTML = `<p class="text-muted text-sm">${t('reader.error.page', { page: pageIdx + 1 })}</p>`;
          pagesEl.appendChild(err);
        });
        if (img.complete) {
          if (img.naturalWidth) {
            img.style.aspectRatio = '';
            state.loaded.add(pageIdx);
            _setDims(pageIdx, img.naturalWidth, img.naturalHeight);
          } else {
            state.failed.add(pageIdx);
          }
        }
        _applyCropToImg(img);
        return img;
      }

      const _firstAlone = state.doublePage && !(getPrefs()?.spreadOffset ?? false);

      if (state.doublePage && state.pages.length > 1) {
        const leftIdx  = state.direction === 'rtl' ? state.currentPage + 1 : state.currentPage;
        const rightIdx = state.direction === 'rtl' ? state.currentPage     : state.currentPage + 1;

        if (_firstAlone && state.currentPage === 0) {
          const img = _makePageImg(0, 'Page 1');
          img.className = _imgClass('paged-single');
          pagesEl.appendChild(img);
        } else if (_isSpreadPair(state.currentPage) && leftIdx < state.pages.length && rightIdx < state.pages.length) {
          const a = /** @type {{w:number,h:number}} */ (state.imgDims.get(leftIdx));
          const b = /** @type {{w:number,h:number}} */ (state.imgDims.get(rightIdx));
          const expectedW = a.w + b.w;
          const expectedH = Math.max(a.h, b.h);

          const canvas = document.createElement('canvas');
          canvas.className = _spreadClass();
          canvas.width  = expectedW;
          canvas.height = expectedH;
          canvas.setAttribute('role', 'img');
          canvas.setAttribute('aria-label',
            `Spread: pages ${Math.min(leftIdx, rightIdx) + 1}–${Math.max(leftIdx, rightIdx) + 1}`);

          const leftImg  = new Image();
          const rightImg = new Image();
          leftImg.src  = state.pages[leftIdx];
          rightImg.src = state.pages[rightIdx];

          let _lReady = false;
          let _rReady = false;
          const _drawSpread = () => {
            if (!_lReady || !_rReady) return;
            const W = _cW(leftImg) + _cW(rightImg);
            const H = Math.max(_cH(leftImg), _cH(rightImg));
            canvas.width  = W;
            canvas.height = H;
            const ctx = canvas.getContext('2d');
            if (!ctx) return;
            _drawPage(ctx, leftImg,  0,            (H - _cH(leftImg))  / 2);
            _drawPage(ctx, rightImg, _cW(leftImg),  (H - _cH(rightImg)) / 2);
            state.loaded.add(leftIdx); state.loaded.add(rightIdx);
            state.loadVersion++;
          };
          leftImg.addEventListener('load', () => {
            _lReady = true;
            _setDims(leftIdx,  leftImg.naturalWidth,  leftImg.naturalHeight);
            _drawSpread();
          });
          rightImg.addEventListener('load', () => {
            _rReady = true;
            _setDims(rightIdx, rightImg.naturalWidth, rightImg.naturalHeight);
            _drawSpread();
          });
          let _spreadErrorShown = false;
          const _showSpreadError = () => {
            state.failed.add(leftIdx); state.failed.add(rightIdx);
            state.loadVersion++;
            if (_spreadErrorShown) return;
            _spreadErrorShown = true;
            canvas.remove();
            const err = document.createElement('div');
            err.className = 'absolute inset-0 flex flex-col items-center justify-center gap-3';
            err.innerHTML = `
              <p class="text-muted text-sm">${t('reader.spread.load_failed')}</p>
              <button class="btn-ghost">${t('common.retry')}</button>
            `;
            err.querySelector('button')?.addEventListener('click', () => {
              state.failed.delete(leftIdx); state.failed.delete(rightIdx);
              _renderPages();
            });
            pagesEl.appendChild(err);
          };
          leftImg.addEventListener('error',  _showSpreadError);
          rightImg.addEventListener('error', _showSpreadError);
          if (leftImg.complete && leftImg.naturalWidth)   { _lReady = true;  _setDims(leftIdx,  leftImg.naturalWidth,  leftImg.naturalHeight);  }
          if (rightImg.complete && rightImg.naturalWidth) { _rReady = true;  _setDims(rightIdx, rightImg.naturalWidth, rightImg.naturalHeight); }
          _drawSpread();

          pagesEl.appendChild(canvas);
        } else if (_isWideImage(state.currentPage) || (state.currentPage + 1 < state.pages.length && _isWideImage(state.currentPage + 1))) {
          const img = _makePageImg(state.currentPage, `Page ${state.currentPage + 1}`);
          img.className = _imgClass('paged-single');
          pagesEl.appendChild(img);
        } else {
          const spread   = document.createElement('div');
          spread.className = 'flex items-center justify-center gap-0.5 max-w-full h-full';
          if (leftIdx < state.pages.length) spread.appendChild(_makePageImg(leftIdx, `Page ${leftIdx + 1}`));
          if (rightIdx < state.pages.length && rightIdx !== leftIdx) spread.appendChild(_makePageImg(rightIdx, `Page ${rightIdx + 1}`));
          pagesEl.appendChild(spread);
        }
        _prefetch(state.currentPage + 1);
      } else if (state.autoSpread && _isSpreadPair(state.currentPage) && state.currentPage + 1 < state.pages.length) {
        const leftIdx  = state.direction === 'rtl' ? state.currentPage + 1 : state.currentPage;
        const rightIdx = state.direction === 'rtl' ? state.currentPage     : state.currentPage + 1;
        const a = /** @type {{w:number,h:number}} */ (state.imgDims.get(leftIdx));
        const b = /** @type {{w:number,h:number}} */ (state.imgDims.get(rightIdx));

        const canvas = document.createElement('canvas');
        canvas.className = _spreadClass();
        canvas.width  = a.w + b.w;
        canvas.height = Math.max(a.h, b.h);
        canvas.setAttribute('role', 'img');
        canvas.setAttribute('aria-label',
          `Spread: pages ${Math.min(leftIdx, rightIdx) + 1}–${Math.max(leftIdx, rightIdx) + 1}`);

        const leftImg  = new Image();
        const rightImg = new Image();
        leftImg.src  = state.pages[leftIdx];
        rightImg.src = state.pages[rightIdx];

        let _lReady = false, _rReady = false;
        const _draw = () => {
          if (!_lReady || !_rReady) return;
          const W = _cW(leftImg) + _cW(rightImg);
          const H = Math.max(_cH(leftImg), _cH(rightImg));
          canvas.width = W; canvas.height = H;
          const ctx = canvas.getContext('2d');
          if (!ctx) return;
          _drawPage(ctx, leftImg,  0,            (H - _cH(leftImg))  / 2);
          _drawPage(ctx, rightImg, _cW(leftImg),  (H - _cH(rightImg)) / 2);
          state.loaded.add(leftIdx); state.loaded.add(rightIdx);
          state.loadVersion++;
        };
        leftImg.addEventListener('load',  () => { _lReady = true;  _setDims(leftIdx,  leftImg.naturalWidth,  leftImg.naturalHeight);  _draw(); });
        rightImg.addEventListener('load', () => { _rReady = true;  _setDims(rightIdx, rightImg.naturalWidth, rightImg.naturalHeight); _draw(); });
        let _errShown = false;
        const _onErr = () => {
          state.failed.add(leftIdx); state.failed.add(rightIdx); state.loadVersion++;
          if (_errShown) return; _errShown = true;
          canvas.remove();
          const err = document.createElement('div');
          err.className = 'absolute inset-0 flex flex-col items-center justify-center gap-3';
          err.innerHTML = `<p class="text-muted text-sm">${t('reader.error.spread')}</p><button class="btn-ghost">${t('common.retry')}</button>`;
          err.querySelector('button')?.addEventListener('click', () => { state.failed.delete(leftIdx); state.failed.delete(rightIdx); _renderPages(); });
          pagesEl.appendChild(err);
        };
        leftImg.addEventListener('error', _onErr);
        rightImg.addEventListener('error', _onErr);
        if (leftImg.complete  && leftImg.naturalWidth)  { _lReady = true;  _setDims(leftIdx,  leftImg.naturalWidth,  leftImg.naturalHeight);  }
        if (rightImg.complete && rightImg.naturalWidth) { _rReady = true;  _setDims(rightIdx, rightImg.naturalWidth, rightImg.naturalHeight); }
        _draw();
        pagesEl.appendChild(canvas);
        _prefetch(state.currentPage + 2);
      } else {
        const img = _makePageImg(state.currentPage, `Page ${state.currentPage + 1}`);
        img.className = _imgClass('paged-single');
        img.addEventListener('error', () => {
          const failedPage = state.currentPage;
          const err = document.createElement('div');
          err.className = 'absolute inset-0 flex flex-col items-center justify-center gap-3';
          err.innerHTML = `
            <p class="text-muted text-sm">${t('reader.error.page', { page: failedPage + 1 })}</p>
            <button class="btn-ghost">${t('common.retry')}</button>
          `;
          err.querySelector('button')?.addEventListener('click', () => {
            state.failed.delete(failedPage);
            _renderPages();
          });
          pagesEl.appendChild(err);
        });
        pagesEl.appendChild(img);
        _prefetch(state.currentPage);
        if (state.autoSpread && !state.hasServerAnalysis && state.currentPage > 0) _isSpreadPair(state.currentPage - 1);
      }
    }

    state.loadVersion++;
  }


  function _goPage(rawDelta) {
    if (state.mode !== 'paged' && state.mode !== 'continuous-paged') return;
    const delta = state.direction === 'rtl' ? -rawDelta : rawDelta;
    const next  = delta > 0 ? _nextStop(state.currentPage) : _prevStop(state.currentPage);

    if (next < 0) {
      if (state.chapterInfo.prev_chapter_id) {
        api.setChapterProgress(state.chapterInfo.prev_chapter_id, -1).catch(() => {});
        navigateChapter(state.chapterInfo.prev_chapter_id);
      }
      return;
    }
    if (next >= state.pages.length) {
      finishChapter({
        api, chapterId,
        nextChapterId: state.chapterInfo.next_chapter_id,
        showEndCard: !!getPrefs()?.endCardInPaged,
        onEndCard: _showPagedEndCard,
        navigateChapter, navigateToManga,
      });
      return;
    }

    state.currentPage = next;
    reportProgress();
    _maybePreloadNext();
    _recordPace();

    if (state.mode === 'continuous-paged' && state.cpTrack) {
      const first = Number(state.cpTrack.firstElementChild?.querySelector('[data-index]')?.dataset.index ?? -1);
      const last  = Number(state.cpTrack.lastElementChild?.querySelector('[data-index]')?.dataset.index ?? -1);
      if (state.currentPage >= first && state.currentPage <= last) {
        _cpSnapToPage(state.currentPage, first);
        state.loadVersion++;
        return;
      }
    }
    _renderPages();
  }

  function destroy() {
    state.scrollObs?.disconnect();
    for (const fn of _cleanup) fn();
    _cleanup.length = 0;
  }

  return {
    applyPresentation, applyTint, updatePageOverlay,
    zoomAt, resetZoom, isZoomed,
    render: _renderPages,
    goPage: _goPage,
    applyCropToAll: _applyCropToAllImages,
    etaText,
    ppm: _ppm,
    destroy,
  };
}
