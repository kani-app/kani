// @ts-check
import { h } from 'preact';
import htm from 'htm';
import { t } from '../../i18n.js';
import { Group, ToggleRow, SliderRow, SegmentedRow, ActionBtn } from './settings-controls.js';

const html = htm.bind(h);

const BG_OPTIONS = () => [
  { value: 'black', label: t('reader.bg.black') },
  { value: 'white', label: t('reader.bg.white') },
  { value: 'sepia', label: t('reader.bg.sepia') },
];

const BLEND_OPTIONS = () => [
  { value: 'multiply', label: t('reader.blend.multiply') },
  { value: 'screen',   label: t('reader.blend.screen')   },
  { value: 'overlay',  label: t('reader.blend.overlay')  },
  { value: 'color',    label: t('reader.blend.color')    },
];

const STRIP_OPTIONS = () => [
  { value: 'full',      label: t('reader.strip.full') },
  { value: 'pagecount', label: t('reader.strip.pagecount') },
  { value: 'off',       label: t('reader.strip.off') },
];

const ORIENT_OPTIONS = () => [
  { value: 'auto',      label: t('reader.orient.auto') },
  { value: 'portrait',  label: t('reader.orient.portrait') },
  { value: 'landscape', label: t('reader.orient.landscape') },
];

const ZONE_OPTIONS = () => [
  { value: 'prev', label: t('reader.zone.prev') },
  { value: 'next', label: t('reader.zone.next') },
  { value: 'menu', label: t('reader.zone.menu') },
  { value: 'none', label: t('reader.zone.none') },
];

/** @param {{ shortcuts: {description: string, key: string}[] }} props */
function ShortcutList({ shortcuts }) {
  return html`
    <div class="flex flex-col gap-1">
      ${shortcuts.map(entry => html`
        <div class="flex items-center justify-between gap-4">
          <span class="text-xs text-muted">${entry.description}</span>
          <kbd class="text-xs bg-surface-2 border border-border rounded px-1.5 py-0.5 font-mono shrink-0">${entry.key}</kbd>
        </div>`)}
    </div>`;
}

/** @param {{ prefs: any, handlers: Record<string, (v: boolean) => void>, onPageOverlay: (v: boolean) => void, onEndCard: (v: boolean) => void, onMiniStrip: (v: string) => void }} props */
export function LayoutSection({ prefs: p, handlers, onPageOverlay, onEndCard, onMiniStrip }) {
  if (!p) return null;
  return html`
    <div class="flex flex-col gap-5">
      <${Group} title=${t('reader.group.reading')} first=${true}>
        <${ToggleRow} label=${t('reader.settings.smooth_scroll')} checked=${p.smoothScroll}
          onChange=${(/** @type {boolean} */ v) => handlers.smooth(v)} />
        <${ToggleRow} label=${t('reader.settings.page_overlay')} checked=${p.pageOverlay}
          onChange=${(/** @type {boolean} */ v) => onPageOverlay(v)} />
      <//>
      <${Group} title=${t('reader.group.spreads')}>
        <${ToggleRow} label=${t('reader.settings.double_page')} checked=${p.doublePage}
          onChange=${(/** @type {boolean} */ v) => handlers.double(v)} />
        <${ToggleRow} label=${t('reader.settings.auto_spread')} checked=${p.autoSpread}
          onChange=${(/** @type {boolean} */ v) => handlers.autoSpread(v)} />
        <${ToggleRow} label=${t('reader.settings.spread_offset')} checked=${p.spreadOffset ?? false}
          onChange=${(/** @type {boolean} */ v) => handlers.spreadOffset(v)} />
      <//>
      <${Group} title=${t('reader.group.end_chapter')}>
        <${ToggleRow} label=${t('reader.settings.end_card_paged')} checked=${p.endCardInPaged ?? false}
          onChange=${(/** @type {boolean} */ v) => onEndCard(v)} />
        <${SegmentedRow} label=${t('reader.settings.progress_strip')} options=${STRIP_OPTIONS()}
          selected=${p.miniStrip ?? 'full'} onSelect=${(/** @type {string} */ v) => onMiniStrip(v)} />
      <//>
    </div>`;
}

/** @param {{ prefs: any, setPref: (k: string, v: any) => void, applyPresentation: () => void, applyCropToAll: () => void, applyTint: () => void }} props */
export function ImageSection({ prefs: p, setPref, applyPresentation, applyCropToAll, applyTint }) {
  if (!p) return null;
  const present = (/** @type {string} */ k, /** @type {any} */ v) => { setPref(k, v); applyPresentation(); };
  const crop = (/** @type {string} */ k, /** @type {number} */ v) => { setPref(k, v); applyCropToAll(); };
  const tint = (/** @type {string} */ k, /** @type {any} */ v) => { setPref(k, v); applyTint(); };
  return html`
    <div class="flex flex-col gap-5">
      <${Group} title=${t('reader.group.background')} first=${true}>
        <${SegmentedRow} label=${t('reader.settings.bg')} options=${BG_OPTIONS()} selected=${p.bg}
          onSelect=${(/** @type {string} */ v) => present('bg', v)} />
        <${ToggleRow} label=${t('reader.settings.tint_bg')} checked=${p.bgTintPage}
          onChange=${(/** @type {boolean} */ v) => present('bgTintPage', v)} />
      <//>
      <${Group} title=${t('reader.group.adjust')}>
        <${SliderRow} label=${t('reader.settings.brightness')} min=${50} max=${200} value=${p.brightness} unit="%"
          onChange=${(/** @type {number} */ v) => present('brightness', v)} />
        <${SliderRow} label=${t('reader.settings.contrast')} min=${50} max=${200} value=${p.contrast} unit="%"
          onChange=${(/** @type {number} */ v) => present('contrast', v)} />
        <${SliderRow} label=${t('reader.settings.saturation')} min=${0} max=${200} value=${p.saturation} unit="%"
          onChange=${(/** @type {number} */ v) => present('saturation', v)} />
        <${ToggleRow} label=${t('reader.settings.grayscale')} checked=${p.grayscale}
          onChange=${(/** @type {boolean} */ v) => present('grayscale', v)} />
        <${ToggleRow} label=${t('reader.settings.invert')} checked=${p.invert}
          onChange=${(/** @type {boolean} */ v) => present('invert', v)} />
      <//>
      <${Group} title=${t('reader.group.crop')}>
        <${SliderRow} label=${t('reader.settings.crop_top')} min=${0} max=${50} value=${p.cropTop} unit="%"
          onChange=${(/** @type {number} */ v) => crop('cropTop', v)} />
        <${SliderRow} label=${t('reader.settings.crop_bottom')} min=${0} max=${50} value=${p.cropBottom} unit="%"
          onChange=${(/** @type {number} */ v) => crop('cropBottom', v)} />
        <${SliderRow} label=${t('reader.settings.crop_left')} min=${0} max=${50} value=${p.cropLeft} unit="%"
          onChange=${(/** @type {number} */ v) => crop('cropLeft', v)} />
        <${SliderRow} label=${t('reader.settings.crop_right')} min=${0} max=${50} value=${p.cropRight} unit="%"
          onChange=${(/** @type {number} */ v) => crop('cropRight', v)} />
      <//>
      <${Group} title=${t('reader.group.tint')}>
        <div class="flex items-center justify-between gap-3">
          <span class="text-sm text-text">${t('reader.settings.tint_color')}</span>
          <input type="color" value=${p.tintColor}
                 aria-label=${t('reader.settings.tint_color')}
                 class="w-8 h-8 rounded cursor-pointer border border-border bg-transparent shrink-0"
                 onInput=${(/** @type {any} */ e) => tint('tintColor', e.currentTarget.value)} />
        </div>
        <${SliderRow} label=${t('reader.settings.tint_opacity')} min=${0} max=${100} value=${p.tintOpacity} unit="%"
          onChange=${(/** @type {number} */ v) => tint('tintOpacity', v)} />
        <${SegmentedRow} label=${t('reader.settings.blend_mode')} options=${BLEND_OPTIONS()} selected=${p.tintBlend}
          onSelect=${(/** @type {string} */ v) => tint('tintBlend', v)} />
      <//>
    </div>`;
}

/**
 * @param {{
 *   prefs: any, onZone: (k: string, v: string) => void, hint: boolean,
 *   shortcuts: {description: string, key: string}[],
 *   showWake: boolean, wakeChecked: boolean, onWake: (v: boolean) => void,
 *   showOrient: boolean, orient: string, onOrient: (v: string) => void,
 * }} props
 */
export function ControlsSection({ prefs: p, onZone, hint, shortcuts, showWake, wakeChecked, onWake, showOrient, orient, onOrient }) {
  if (!p) return null;
  return html`
    <div class="flex flex-col gap-5">
      <${Group} title=${t('reader.panel.tap_zones')} first=${true}>
        <${SegmentedRow} label=${t('reader.settings.zone_left')} options=${ZONE_OPTIONS()} selected=${p.tapLeft}
          onSelect=${(/** @type {string} */ v) => onZone('tapLeft', v)} />
        <${SegmentedRow} label=${t('reader.settings.zone_center')} options=${ZONE_OPTIONS()} selected=${p.tapCenter}
          onSelect=${(/** @type {string} */ v) => onZone('tapCenter', v)} />
        <${SegmentedRow} label=${t('reader.settings.zone_right')} options=${ZONE_OPTIONS()} selected=${p.tapRight}
          onSelect=${(/** @type {string} */ v) => onZone('tapRight', v)} />
        ${hint ? html`<p class="text-xs text-danger">${t('reader.tap_zone.guard')}</p>` : null}
      <//>
      ${(showWake || showOrient) ? html`
      <${Group} title=${t('reader.group.display')}>
        ${showWake ? html`<${ToggleRow} label=${t('reader.settings.wake_lock')} checked=${wakeChecked}
          onChange=${(/** @type {boolean} */ v) => onWake(v)} />` : null}
        ${showOrient ? html`<${SegmentedRow} label=${t('reader.settings.orientation')} options=${ORIENT_OPTIONS()}
          selected=${orient} onSelect=${(/** @type {string} */ v) => onOrient(v)} />` : null}
      <//>` : null}
      <${Group} title=${t('reader.shortcuts.title')}>
        <${ShortcutList} shortcuts=${shortcuts} />
      <//>
    </div>`;
}

/**
 * @param {{
 *   prefs: any, setPref: (k: string, v: any) => void, onSavePage: () => void,
 *   onSleep: (v: number) => void, slideshowActive: boolean, onSlideshow: () => void,
 *   stats: {eta: string, pace: string},
 * }} props
 */
export function BehaviorSection({ prefs: p, setPref, onSavePage, onSleep, slideshowActive, onSlideshow, stats }) {
  if (!p) return null;
  return html`
    <div class="flex flex-col gap-5">
      <${Group} title=${t('reader.group.navigation')} first=${true}>
        <${SliderRow} label=${t('reader.settings.preload')} min=${1} max=${10} value=${p.preloadCount ?? 2}
          onChange=${(/** @type {number} */ v) => setPref('preloadCount', v)} />
        <${ActionBtn} label=${t('reader.settings.save_page')} onClick=${onSavePage} />
      <//>
      <${Group} title=${t('reader.group.playback')}>
        <${ActionBtn} label=${slideshowActive ? t('reader.slideshow.stop') : t('reader.slideshow.start')}
          onClick=${onSlideshow} />
        <${SliderRow} label=${t('reader.settings.slideshow_speed')} min=${3} max=${30} value=${p.slideshowInterval} unit="s"
          onChange=${(/** @type {number} */ v) => setPref('slideshowInterval', v)} />
        <${SliderRow} label=${t('reader.settings.sleep')} min=${0} max=${60} value=${p.inactivityTimeout} unit="min"
          onChange=${(/** @type {number} */ v) => onSleep(v)} />
      <//>
      <${Group} title=${t('reader.group.stats')}>
        <div class="flex items-center justify-between">
          <span class="text-xs text-muted">${t('reader.stats.eta')}</span>
          <span class="text-xs text-text tabular-nums">${stats.eta}</span>
        </div>
        <div class="flex items-center justify-between">
          <span class="text-xs text-muted">${t('reader.stats.pace')}</span>
          <span class="text-xs text-text tabular-nums">${stats.pace}</span>
        </div>
      <//>
    </div>`;
}
