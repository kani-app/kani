// @ts-check

import { h } from 'preact';
import { useState, useEffect } from 'preact/hooks';
import htm from 'htm';
import { getLocal, setLocal, resetAllConfirmDialogs } from '../../utils.js';
import { getCoverQuality, setCoverQuality } from '../../cover-quality.js';
import { SettingsGroup, SettingsRow, ToggleRow, SelectRow } from './_shared.js';
import {
  ACCENT_SWATCHES,
  getCurrentTheme,
  saveAndApplyTheme,
  getCustomThemes,
  applyCustomTheme,
  syncServerThemes,
} from '../../theme.js';
import { activateUiTheme, deactivateUiTheme } from '../../api.js';
import { hasPermission } from '../../session.js';
import { t } from '../../i18n.js';
import { showToast, showApiError } from '../../components/toast.js';
import { ThemeEditor, ThemePreviewSwatch } from '../../components/theme-editor.js';
import { showCheatsheet } from '../../shortcuts.js';
import { isPhoneLayout, PHONE_LAYOUT_QUERY } from '../../pagination-mode.js';

const html = htm.bind(h);

const RING = '0 0 0 2.5px var(--color-bg), 0 0 0 4.5px var(--color-accent)';

const PAGINATION_PREFS = [
  { label: 'settings.general.pagination.chapters', key: 'kani_chapter_pagination', desc: 'settings.general.pagination.chapters.desc' },
  { label: 'settings.general.pagination.library', key: 'kani_library_pagination', desc: 'settings.general.pagination.library.desc' },
  { label: 'settings.general.pagination.source', key: 'kani_source_pagination', desc: 'settings.general.pagination.source.desc' },
];

function AccentRow({ accent, onPick }) {
  const isCustom = !ACCENT_SWATCHES.some((s) => s.color === accent);
  return html`
    <div class="flex items-center gap-2 flex-wrap shrink-0">
      ${ACCENT_SWATCHES.map(
        ({ color, label }) => html`
          <button
            type="button"
            title=${label}
            aria-label=${label}
            aria-pressed=${String(color === accent)}
            class="w-6 h-6 rounded-full shrink-0 transition-[box-shadow]"
            style=${`background:${color};box-shadow:${color === accent ? RING : 'none'}`}
            onClick=${() => onPick(color)}
          ></button>
        `,
      )}
      <label
        class="w-6 h-6 rounded-full border-2 border-dashed border-border cursor-pointer flex items-center justify-center transition-[box-shadow] shrink-0 overflow-hidden"
        title=${t('settings.display.accent.custom')}
        style=${isCustom ? `background:${accent};box-shadow:${RING}` : ''}
      >
        <input
          type="color"
          class="opacity-0 w-0 h-0 absolute"
          value=${isCustom ? accent : '#e8545a' /* audit-ignore: default accent source value */}
          onInput=${(/** @type {any} */ e) => onPick(e.target.value)}
        />
      </label>
    </div>
  `;
}

export function GeneralSection() {
  const [theme, setTheme] = useState(getCurrentTheme());
  const [, setTick] = useState(0);
  const [editor, setEditor] = useState(/** @type {{ open: boolean, id: string|null }} */ ({ open: false, id: null }));

  const refresh = () => {
    setTheme(getCurrentTheme());
    setTick((n) => n + 1);
  };

  useEffect(() => {
    syncServerThemes().then(refresh).catch(() => { });
  }, []);

  const [phoneLayout, setPhoneLayout] = useState(isPhoneLayout());
  useEffect(() => {
    if (typeof matchMedia !== 'function') return;
    const query = matchMedia(PHONE_LAYOUT_QUERY);
    const onChange = () => setPhoneLayout(query.matches);
    query.addEventListener('change', onChange);
    return () => query.removeEventListener('change', onChange);
  }, []);

  const applyTheme = (/** @type {string} */ th, /** @type {string} */ dn, /** @type {string} */ ac) => {
    saveAndApplyTheme(th, dn, ac);
    refresh();
    if (!th.startsWith('custom:')) deactivateUiTheme().catch(showApiError);
  };

  const pickCustomTheme = (/** @type {string} */ id) => {
    applyCustomTheme(id);
    refresh();
    activateUiTheme(id).catch(showApiError);
  };

  const openEditor = (/** @type {string|null} */ id) => setEditor({ open: true, id });
  const closeEditor = () => setEditor({ open: false, id: null });
  const onEditorSave = (/** @type {string|null} */ savedId) => {
    if (savedId !== null) {
      pickCustomTheme(savedId);
    } else {
      const prev = getCurrentTheme();
      if (editor.id && prev.theme === `custom:${editor.id}`) {
        saveAndApplyTheme('dark', prev.density, prev.accent);
      }
    }
    closeEditor();
    refresh();
  };

  const localToggle = (/** @type {string} */ key, /** @type {boolean} */ checked, /** @type {(v:boolean)=>string} */ toStore) => (
    /** @type {boolean} */ v,
  ) => {
    setLocal(key, toStore(v));
    setTick((n) => n + 1);
  };

  const customThemes = getCustomThemes();
  const canPublish = hasPermission('theme:publish');
  const activeTheme = theme.theme;
  const hasNotification = typeof window !== 'undefined' && 'Notification' in window;
  // Browsers expose the API over plain HTTP but refuse the permission, reporting
  // it as denied. Told apart so the row does not blame the browser's settings
  // for something only a secure origin can change.
  const notificationsInsecure = hasNotification && !window.isSecureContext;

  return html`
    <${SettingsGroup} label=${t('settings.display.group')}>
      <${SelectRow}
        label=${t('settings.display.theme')}
        description=${t('settings.display.theme.desc')}
        value=${theme.theme}
        onChange=${(v) => applyTheme(v, theme.density, theme.accent)}
        options=${[
          { value: 'light', label: t('settings.display.theme.light') },
          { value: 'dark', label: t('settings.display.theme.dark') },
          { value: 'black', label: t('settings.display.theme.black') },
          { value: 'system', label: t('settings.display.theme.system') },
        ]}
      />
      <${SelectRow}
        label=${t('settings.display.density')}
        description=${t('settings.display.density.desc')}
        value=${theme.density}
        onChange=${(v) => applyTheme(theme.theme, v, theme.accent)}
        options=${[
          { value: 'comfortable', label: t('settings.display.density.comfortable') },
          { value: 'compact', label: t('settings.display.density.compact') },
        ]}
      />
      <${SelectRow}
        label=${t('settings.display.cover_quality')}
        description=${t('settings.display.cover_quality.desc')}
        value=${getCoverQuality()}
        onChange=${(v) => {
          setCoverQuality(v);
          setTick((n) => n + 1);
        }}
        options=${[
          { value: 'sm', label: t('settings.display.cover_quality.sm') },
          { value: 'md', label: t('settings.display.cover_quality.md') },
          { value: 'lg', label: t('settings.display.cover_quality.lg') },
        ]}
      />
      <${SettingsRow} label=${t('settings.display.accent')} description=${t('settings.display.accent.desc')}>
        <${AccentRow} accent=${theme.accent} onPick=${(hex) => applyTheme(theme.theme, theme.density, hex)} />
      <//>
      <${SettingsRow}
        label=${t('settings.display.reset_confirms')}
        description=${t('settings.display.reset_confirms.desc')}
      >
        <button
          type="button"
          class="btn-ghost btn-sm"
          onClick=${() => {
            resetAllConfirmDialogs();
            showToast(t('settings.display.confirms_reset'), { type: 'success' });
          }}
        >
          ${t('settings.display.reset_confirms.action')}
        </button>
      <//>
      <${SettingsRow}
        label=${t('settings.display.shortcuts')}
        description=${t('settings.display.shortcuts.desc')}
      >
        <button type="button" class="btn-ghost btn-sm" onClick=${() => showCheatsheet()}>
          ${t('settings.display.shortcuts.action')}
        </button>
      <//>
    <//>

    ${!phoneLayout && html`<${SettingsGroup} label=${t('settings.general.pagination.group')}>
      ${PAGINATION_PREFS.map(
        (p) => html`<${SelectRow}
          key=${p.key}
          label=${t(p.label)}
          description=${t(p.desc)}
          value=${getLocal(p.key) || 'paginated'}
          onChange=${(v) => {
            setLocal(p.key, v);
            setTick((n) => n + 1);
          }}
          options=${[
            { value: 'paginated', label: t('settings.general.pagination.paginated') },
            { value: 'infinite', label: t('settings.general.pagination.infinite') },
          ]}
        />`,
      )}
    <//>`}

    <${SettingsGroup} label=${t('settings.general.reading.group')}>
      <${ToggleRow}
        label=${t('settings.general.reading.coalesce')}
        description=${t('settings.general.reading.coalesce.desc')}
        checked=${getLocal('kani_coalesce_read') === 'true'}
        onChange=${localToggle('kani_coalesce_read', false, (v) => (v ? 'true' : 'false'))}
      />
      <${ToggleRow}
        label=${t('settings.general.reading.external_warn')}
        description=${t('settings.general.reading.external_warn.desc')}
        checked=${getLocal('kani_skip_external_warning') !== 'true'}
        onChange=${localToggle('kani_skip_external_warning', false, (v) => (v ? 'false' : 'true'))}
      />
    <//>

    <${SettingsGroup} label=${t('settings.general.notifications.group')}>
      <${ToggleRow}
        label=${t('settings.general.notifications.in_app')}
        description=${t('settings.general.notifications.in_app.desc')}
        checked=${getLocal('kani_disable_notifications') !== 'true'}
        onChange=${localToggle('kani_disable_notifications', false, (v) => (v ? 'false' : 'true'))}
      />
      ${hasNotification &&
      html`<${SettingsRow}
        label=${t('settings.general.notifications.browser')}
        description=${notificationsInsecure
          ? t('settings.general.notifications.browser.insecure')
          : Notification.permission === 'denied'
            ? t('settings.general.notifications.browser.blocked')
            : t('settings.general.notifications.browser.desc')}
      >
        <label class="kani-toggle">
          <input
            type="checkbox"
            class="kani-toggle__input"
            checked=${getLocal('kani_browser_notifications') === 'true' && Notification.permission === 'granted'}
            disabled=${notificationsInsecure || Notification.permission === 'denied'}
            onChange=${async (/** @type {any} */ e) => {
              const on = e.target.checked;
              if (on) {
                const perm = await Notification.requestPermission();
                if (perm !== 'granted') {
                  e.target.checked = false;
                  return;
                }
              }
              setLocal('kani_browser_notifications', on ? 'true' : 'false');
              setTick((n) => n + 1);
            }}
          />
          <span class="kani-toggle__track"></span>
        </label>
      <//>`}
    <//>

    <${SettingsGroup} label=${t('theme.custom.group')}>
      ${customThemes.map((ct) => {
        const isActive = activeTheme === `custom:${ct.id}`;
        return html`
          <div
            key=${ct.id}
            class=${`flex items-center gap-3 px-4 py-3 cursor-pointer hover:bg-surface-3 transition-colors${isActive ? ' bg-surface-3' : ''}`}
            aria-current=${isActive ? 'true' : undefined}
            onClick=${() => pickCustomTheme(ct.id)}
          >
            <div><${ThemePreviewSwatch} tokens=${ct.tokens} /></div>
            <span class=${`text-sm flex-1 min-w-0 truncate${isActive ? ' font-semibold text-accent' : ' text-text'}`}
              >${ct.name}</span
            >
            ${ct.instanceWide &&
            html`<span class="text-xs text-text-faint shrink-0">${t('theme.custom.instance_wide')}</span>`}
            ${(!ct.instanceWide || canPublish) &&
            html`<button
              type="button"
              class="btn-ghost btn-sm shrink-0"
              onClick=${(/** @type {Event} */ e) => {
                e.stopPropagation();
                openEditor(ct.id);
              }}
            >
              ${t('theme.custom.edit_action')}
            </button>`}
          </div>
        `;
      })}
      ${customThemes.length === 0 &&
      html`<div class="px-4 py-3"><p class="text-sm text-text-muted">${t('theme.custom.empty')}</p></div>`}
      <div class="flex items-center px-4 py-3">
        <button type="button" class="btn-ghost btn-sm" onClick=${() => openEditor(null)}>
          ${t('theme.custom.new')}
        </button>
      </div>
    <//>

    <p class="text-xs text-text-muted">${t('settings.general.local_prefs_note')}</p>

    ${editor.open &&
    html`<${ThemeEditor} themeId=${editor.id} onClose=${closeEditor} onSave=${onEditorSave} />`}
  `;
}
