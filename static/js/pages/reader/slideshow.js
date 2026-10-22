// @ts-check
// Reader slideshow + inactivity timeout. Auto-advances pages/chapters on a
// timer; any user input pauses the slideshow and resets the sleep timer.

/**
 * @param {{
 *   state: { mode: string, direction: string, chapterInfo: { next_chapter_id?: number|null } },
 *   pagesEl: HTMLElement,
 *   engine: { goPage: (d: number) => void },
 *   getPrefs: () => { slideshowInterval?: number, inactivityTimeout?: number } | null,
 *   slideshowSignal: { value: boolean },
 *   navigateChapter: (chId: number) => void,
 *   navigateToManga: () => void,
 * }} deps
 */
export function createSlideshow({ state, pagesEl, engine, getPrefs, slideshowSignal, navigateChapter, navigateToManga }) {
  let active = false;
  /** @type {ReturnType<typeof setTimeout>|null} */
  let timer = null;
  /** Timestamp of the most recent play() — ignore the starting tap in onUserInput. */
  let startedAt = 0;
  /** @type {ReturnType<typeof setTimeout>|null} */
  let inactivityTimer = null;

  function stop() {
    active = false;
    if (timer) { clearTimeout(timer); timer = null; }
    slideshowSignal.value = false;
  }

  function advance() {
    if (!active) return;
    const isScrollLike = state.mode === 'scroll' || state.mode === 'webtoon';
    if (isScrollLike) {
      const before = pagesEl.scrollTop;
      pagesEl.scrollBy({ top: pagesEl.clientHeight, behavior: 'smooth' });
      setTimeout(() => {
        if (!active) return;
        if (Math.abs(pagesEl.scrollTop - before) < 4 ||
            pagesEl.scrollTop + pagesEl.clientHeight >= pagesEl.scrollHeight - 4) {
          if (state.chapterInfo.next_chapter_id) navigateChapter(state.chapterInfo.next_chapter_id);
        }
      }, 600);
    } else {
      // Always advance forward: compensate for RTL so goPage's direction flip doesn't reverse us.
      engine.goPage(state.direction === 'rtl' ? -1 : 1);
    }
  }

  function schedule() {
    if (!active) return;
    const ms = (getPrefs()?.slideshowInterval ?? 5) * 1000;
    timer = setTimeout(() => {
      if (!active) return;
      advance();
      schedule();
    }, ms);
  }

  function play() {
    active = true;
    startedAt = Date.now();
    schedule();
    slideshowSignal.value = true;
  }

  function resetInactivity() {
    if (inactivityTimer) clearTimeout(inactivityTimer);
    const ms = (getPrefs()?.inactivityTimeout ?? 0) * 60000;
    if (!ms) return;
    inactivityTimer = setTimeout(() => {
      if (active) stop();
      else navigateToManga();
    }, ms);
  }

  const onUserInput = () => {
    // Ignore the very interaction that started the slideshow (the tap/click on
    // Start) so it doesn't immediately cancel itself.
    if (Date.now() - startedAt < 500) { resetInactivity(); return; }
    if (active) stop();
    resetInactivity();
  };
  document.addEventListener('keydown',     onUserInput, { capture: true, passive: true });
  document.addEventListener('pointerdown', onUserInput, { capture: true, passive: true });

  function destroy() {
    stop();
    if (inactivityTimer) clearTimeout(inactivityTimer);
    document.removeEventListener('keydown',     onUserInput, { capture: true });
    document.removeEventListener('pointerdown', onUserInput, { capture: true });
  }

  return { play, stop, resetInactivity, isActive: () => active, destroy };
}
