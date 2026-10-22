// @ts-check
// Reader chrome visibility: the side drawer, the top/bottom bars, fine-pointer
// hover-to-reveal, and the three-zone tap handler. These share mutable state
// (bars-visible / panel-open / hovering / hide-timer) so they live together.

/**
 * @param {{
 *   fullBar: HTMLElement, segsEl: HTMLElement, topBar: HTMLElement,
 *   sidePanel: HTMLElement, backdrop: HTMLElement, miniStrip: HTMLElement,
 *   barHover: HTMLElement, pagesEl: HTMLElement,
 *   state: { mode: string, direction: string },
 *   engine: { goPage: (d: number) => void, isZoomed: () => boolean },
 *   getPrefs: () => { tapLeft?: string, tapCenter?: string, tapRight?: string } | null,
 *   isDesktop: () => boolean, isFinePointer: () => boolean,
 *   loadChapterList: () => void, panelOpenCallbacks: Array<() => void>,
 * }} deps
 */
export function createChromeVisibility({
  fullBar, segsEl, topBar, sidePanel, backdrop, miniStrip, barHover, pagesEl,
  state, engine, getPrefs, isDesktop, isFinePointer, loadChapterList, panelOpenCallbacks,
}) {
  let barsVisible = false;
  let panelOpen = false;
  let isHovering = false;
  /** @type {ReturnType<typeof setTimeout>|null} */
  let hideTimer = null;
  /** @type {Element|null} */
  let panelTrigger = null;

  const panelClosedTransform = () => isDesktop() ? 'translateX(-100%)' : 'translateX(100%)';

  // Position the side panel (left on desktop, right on mobile).
  function positionPanel() {
    if (isDesktop()) {
      sidePanel.style.left = '0';
      sidePanel.style.right = '';
      sidePanel.style.borderRight = '1px solid var(--color-border)';
      sidePanel.style.borderLeft = '';
    } else {
      sidePanel.style.right = '0';
      sidePanel.style.left = '';
      sidePanel.style.borderLeft = '1px solid var(--color-border)';
      sidePanel.style.borderRight = '';
    }
    if (!panelOpen) sidePanel.style.transform = panelClosedTransform();
  }

  function showBars() {
    barsVisible = true;
    fullBar.style.transform = '';
    segsEl.style.pointerEvents = 'auto';
    if (!isDesktop()) topBar.style.transform = '';
    if (hideTimer) clearTimeout(hideTimer);
    if (isFinePointer() && !panelOpen && !isHovering) hideTimer = setTimeout(hideBars, 3000);
  }

  function hideBars() {
    barsVisible = false;
    fullBar.style.transform = 'translateY(100%)';
    segsEl.style.pointerEvents = 'none';
    topBar.style.transform = 'translateY(-100%)';
    closePanel();
  }

  function openPanel() {
    panelOpen = true;
    positionPanel();
    sidePanel.style.transform = 'translateX(0)';
    sidePanel.setAttribute('aria-modal', 'true');
    backdrop.classList.remove('hidden');
    if (hideTimer) clearTimeout(hideTimer);
    loadChapterList();
    for (const fn of panelOpenCallbacks) fn();
    // Move focus into the drawer; remember the trigger to restore on close.
    panelTrigger = document.activeElement;
    requestAnimationFrame(() => {
      const first = /** @type {HTMLElement|null} */ (sidePanel.querySelector('button:not(:disabled),a,select,input'));
      (first ?? sidePanel).focus();
    });
  }

  function closePanel() {
    panelOpen = false;
    sidePanel.style.transform = panelClosedTransform();
    sidePanel.setAttribute('aria-modal', 'false');
    backdrop.classList.add('hidden');
    if (panelTrigger instanceof HTMLElement && document.contains(panelTrigger)) panelTrigger.focus();
    panelTrigger = null;
    if (isFinePointer() && !isHovering && barsVisible) hideTimer = setTimeout(hideBars, 1500);
  }

  function toggleBars() { if (barsVisible) hideBars(); else showBars(); }

  backdrop.addEventListener('click', () => closePanel());

  if (isFinePointer()) {
    barHover.style.pointerEvents = 'auto';
    const onEnter = () => { isHovering = true; if (hideTimer) clearTimeout(hideTimer); if (!barsVisible) showBars(); };
    const onLeave = () => { isHovering = false; if (!panelOpen) hideTimer = setTimeout(hideBars, 200); };
    barHover.addEventListener('mouseenter', onEnter);
    barHover.addEventListener('mouseleave', onLeave);
    fullBar.addEventListener('mouseenter', onEnter);
    fullBar.addEventListener('mouseleave', onLeave);
  } else {
    miniStrip.style.pointerEvents = 'auto';
    miniStrip.addEventListener('click', (e) => { e.stopPropagation(); toggleBars(); });
  }

  function triggerZoneAction(/** @type {string} */ action) {
    switch (action) {
      case 'prev': engine.goPage(state.direction === 'rtl' ? 1 : -1); break;
      case 'next': engine.goPage(state.direction === 'rtl' ? -1 : 1); break;
      case 'menu':
        if (isFinePointer()) openPanel();
        else toggleBars();
        break;
    }
  }

  pagesEl.addEventListener('click', (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    if (target.closest('button') || target.closest('a')) return;
    if (engine.isZoomed()) return;

    const rect = pagesEl.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const third = rect.width / 3;

    const prefs = getPrefs();
    const tapLeft = prefs?.tapLeft ?? 'prev';
    const tapCenter = prefs?.tapCenter ?? 'menu';
    const tapRight = prefs?.tapRight ?? 'next';

    if (state.mode === 'paged' || state.mode === 'continuous-paged') {
      if (x < third) { triggerZoneAction(tapLeft); return; }
      if (x > 2 * third) { triggerZoneAction(tapRight); return; }
    }
    if (x >= third && x <= 2 * third) triggerZoneAction(tapCenter);
  });

  function destroy() { if (hideTimer) clearTimeout(hideTimer); }

  return {
    openPanel, closePanel, showBars, hideBars, positionPanel,
    isPanelOpen: () => panelOpen,
    isBarsVisible: () => barsVisible,
    destroy,
  };
}
