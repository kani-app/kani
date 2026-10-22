// @ts-check
import { h } from 'preact';
import htm from 'htm';
import { t } from '../../i18n.js';
import { Modal } from '../modal.js';
import { Icon } from '../icon.js';
import { iconTabLayout, iconTabImage, iconTabControls, iconSettings } from '../../icons.js';
import { LayoutSection, ImageSection, ControlsSection, BehaviorSection } from './settings-sections.js';

const html = htm.bind(h);

const TABS = () => [
  { id: 'layout',   name: t('reader.tab.layout'),   icon: iconTabLayout },
  { id: 'image',    name: t('reader.tab.image'),    icon: iconTabImage },
  { id: 'controls', name: t('reader.tab.controls'), icon: iconTabControls },
  { id: 'behavior', name: t('reader.tab.behavior'), icon: iconSettings },
];

/** @param {string} tab @param {any} ctx */
function section(tab, ctx) {
  switch (tab) {
    case 'layout':   return h(LayoutSection,   { prefs: ctx.prefs, handlers: ctx.layoutHandlers, onPageOverlay: ctx.onOverlay, onEndCard: ctx.onEndCard, onMiniStrip: ctx.onMiniStrip });
    case 'image':    return h(ImageSection,    { prefs: ctx.prefs, setPref: ctx.setPref, applyPresentation: ctx.applyPresentation, applyCropToAll: ctx.applyCropToAll, applyTint: ctx.applyTint });
    case 'controls': return h(ControlsSection, { prefs: ctx.prefs, onZone: ctx.onZone, hint: ctx.tapHint, shortcuts: ctx.shortcuts, showWake: ctx.showWake, wakeChecked: ctx.wakeChecked, onWake: ctx.onWake, showOrient: ctx.showOrient, orient: ctx.orient, onOrient: ctx.onOrient });
    case 'behavior': return h(BehaviorSection, { prefs: ctx.prefs, setPref: ctx.setPref, onSavePage: ctx.onSavePage, onSleep: ctx.onSleep, slideshowActive: ctx.slideshowActive, onSlideshow: ctx.onSlideshow, stats: ctx.stats });
    default:         return null;
  }
}

/** @param {{ open: boolean, tab: string, onClose: () => void, onTab: (id: string) => void, ctx: any }} props */
export function ReaderSettingsModal({ open, tab, onClose, onTab, ctx }) {
  return html`
    <${Modal} open=${open} onClose=${onClose} title=${t('reader.settings.title')} wide=${true} focusContainer=${true}>
      <div class="flex gap-4 items-start">
        <div class="shrink-0 border-r border-border pr-2 flex flex-col gap-1 sticky top-0">
          ${TABS().map(tb => html`
            <button aria-pressed=${tb.id === tab} title=${tb.name}
                    class=${'flex items-center gap-2 rounded-lg px-3 py-2.5 text-sm transition-colors justify-center sm:justify-start '
                      + (tb.id === tab ? 'bg-accent-dim text-accent font-medium' : 'text-muted hover:bg-surface-2 hover:text-text')}
                    onClick=${() => onTab(tb.id)}>
              <${Icon} svg=${tb.icon} />
              <span class="hidden sm:inline">${tb.name}</span>
            </button>`)}
        </div>
        <div class="flex-1 min-w-0">
          ${section(tab, ctx)}
        </div>
      </div>
    <//>`;
}
