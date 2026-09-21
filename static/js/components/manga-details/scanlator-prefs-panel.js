// @ts-check

import { h, render } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { t } from '../../i18n.js';
import { showToast } from '../toast.js';
import { EmptyState } from '../empty-state.js';
import { Combobox } from '../combobox.js';
import { iconX } from '../../icons.js';
import { mountSortableList } from '../sortable-list.js';
import { Tabs } from '../tabs.js';
const html = htm.bind(h);

/**
 * @param {HTMLElement} bodyEl
 * @param {any[]} initialPrefs
 * @param {string} initialMode
 * @param {number} dbId
 * @param {(prefs: any[]) => void} onPrefsChange
 * @param {boolean} [initialAutoReplace]
 * @param {(on: boolean) => void} [onAutoReplaceChange]
 */
export function mountScanlatorPrefsPanel(
  bodyEl,
  initialPrefs,
  initialMode,
  dbId,
  onPrefsChange,
  initialAutoReplace = false,
  onAutoReplaceChange = () => {},
) {
  const mount = document.createElement('div');
  bodyEl.appendChild(mount);
  render(html`<${ScanlatorPrefsPanel}
    initialPrefs=${initialPrefs}
    initialMode=${initialMode}
    dbId=${dbId}
    onPrefsChange=${onPrefsChange}
    initialAutoReplace=${initialAutoReplace}
    onAutoReplaceChange=${onAutoReplaceChange}
  />`, mount);
}


function ScanlatorPrefsPanel({
  initialPrefs,
  initialMode,
  dbId,
  onPrefsChange,
  initialAutoReplace,
  onAutoReplaceChange,
}) {
  const [autoReplace, setAutoReplace] = useState(Boolean(initialAutoReplace));
  const [autoReplaceBusy, setAutoReplaceBusy] = useState(false);
  const [prefs, setPrefs] = useState(/** @type {any[]} */ (Array.isArray(initialPrefs) ? [...initialPrefs] : []));
  const [mode, setMode] = useState(/** @type {string} */ (initialMode ?? 'priority'));
  const [scOptions, setScOptions] = useState(/** @type {Array<{id:number,name:string}>} */ ([{ id: -1, name: '* (Any scanlator)' }]));
  const [scCmbVal, setScCmbVal] = useState('');
  const listContainerRef = useRef(/** @type {HTMLDivElement | null} */ (null));

  useEffect(() => {
    api.getChapterScanlators(dbId).then(scanlators => {
      setScOptions([
        { id: -1, name: '* (Any scanlator)' },
        ...(Array.isArray(scanlators) ? scanlators : []).map((s, i) => ({ id: i, name: s })),
      ]);
    }).catch(() => {});
  }, [dbId]);

  useEffect(() => {
    const el = listContainerRef.current;
    if (!el || prefs.length === 0) return;

    const sortedPrefs = [...prefs].sort((a, b) => (b.priority ?? 0) - (a.priority ?? 0));

    const { destroy } = mountSortableList(el, {
      items: sortedPrefs,
      getId: (pref) => pref.id,
      renderItem: (pref) => {
        const content = document.createElement('div');
        content.className = 'flex flex-1 items-center gap-3 min-w-0';

        const nameSpan = document.createElement('span');
        nameSpan.className = 'flex-1 text-sm ' + (pref.blocked ? 'text-danger line-through' : 'text-text');
        nameSpan.textContent = pref.scanlator || t('manga.scanlator.any');
        content.appendChild(nameSpan);

        const btns = document.createElement('div');
        btns.className = 'flex items-center gap-2 shrink-0';

        if (mode === 'priority') {
          const blockBtn = document.createElement('button');
          blockBtn.type = 'button';
          blockBtn.className = 'btn-sm ' + (pref.blocked ? 'btn-danger' : 'btn-ghost');
          blockBtn.title = pref.blocked ? t('manga.scanlator.unblock') : t('manga.scanlator.block');
          blockBtn.textContent = pref.blocked ? t('manga.scanlator.blocked') : t('manga.scanlator.block');
          blockBtn.addEventListener('click', async () => {
            const newBlocked = !pref.blocked;
            try {
              await api.setScanlatorPref(dbId, pref.scanlator, pref.priority, newBlocked);
              setPrefs(prev => {
                const next = prev.map(p => p.id === pref.id ? { ...p, blocked: newBlocked } : p);
                onPrefsChange([...next]);
                return next;
              });
            } catch { }
          });
          btns.appendChild(blockBtn);
        }

        const rmBtn = document.createElement('button');
        rmBtn.type = 'button';
        rmBtn.className = 'btn-icon text-danger';
        rmBtn.setAttribute('aria-label', t('manga.scanlator.remove', { name: pref.scanlator || t('manga.scanlator.any') }));
        rmBtn.innerHTML = iconX;
        rmBtn.addEventListener('click', async () => {
          try {
            await api.deleteScanlatorPref(pref.id);
            setPrefs(prev => {
              const next = prev.filter(p => p.id !== pref.id);
              onPrefsChange([...next]);
              return next;
            });
          } catch { }
        });
        btns.appendChild(rmBtn);
        content.appendChild(btns);
        return content;
      },
      onReorder: async (_ids, newOrder) => {
        for (let i = 0; i < newOrder.length; i++) {
          const pref = newOrder[i];
          const newPriority = newOrder.length - i;
          if (pref.priority !== newPriority) {
            api.setScanlatorPref(dbId, pref.scanlator, newPriority, pref.blocked).catch(() => {});
          }
        }
        const priorityMap = Object.fromEntries(newOrder.map((p, i) => [p.id, newOrder.length - i]));
        setPrefs(prev => {
          const next = prev.map(p => ({ ...p, priority: priorityMap[p.id] ?? p.priority }));
          onPrefsChange([...next]);
          return next;
        });
      },
      className: 'flex flex-col divide-y divide-border-subtle',
    });

    return () => {
      destroy?.();
      el.innerHTML = '';
    };
  }, [prefs, mode, dbId]);

  async function handleAutoReplaceChange(on) {
    setAutoReplaceBusy(true);
    const previous = autoReplace;
    setAutoReplace(on);
    try {
      await api.setUpgradeAutoReplace(dbId, on);
      onAutoReplaceChange(on);
    } catch (e) {
      setAutoReplace(previous);
      showToast(
        /** @type {any} */ (e)?.hint ?? /** @type {any} */ (e)?.message ?? t('manga.scanlator.auto_replace.failed'),
        { type: 'error' },
      );
    } finally {
      setAutoReplaceBusy(false);
    }
  }

  async function handleModeChange(newMode) {
    try {
      await api.setScanlatorMode(dbId, newMode);
      setMode(newMode);
    } catch { }
  }

  async function handleAdd() {
    const name = scCmbVal.trim();
    if (!name) return;
    const priority = prefs.length + 1;
    try {
      const finalName = name === '* (Any scanlator)' ? '' : name;
      await api.setScanlatorPref(dbId, finalName, priority, false);
      setPrefs(prev => {
        const existing = prev.find(p => p.scanlator === finalName);
        const next = existing
          ? prev.map(p => p.id === existing.id ? { ...p, priority, blocked: false } : p)
          : [...prev, { id: Date.now(), manga_id: dbId, scanlator: finalName, priority, blocked: false }];
        onPrefsChange([...next]);
        return next;
      });
      setScCmbVal('');
    } catch (e) {
      showToast(/** @type {any} */(e)?.hint ?? /** @type {any} */(e)?.message ?? t('manga.scanlator.add_failed'), { type: 'error' });
    }
  }

  const used = new Set(prefs.map(p => p.scanlator === '' ? '* (Any scanlator)' : p.scanlator));
  const cmbOpts = scOptions.filter(o => !used.has(o.name));
  const cmbValue = cmbOpts.find(o => o.name === scCmbVal)?.id ?? null;

  const fallbackRow = mode === 'priority' ? html`
    <div class="flex items-center gap-3 py-2 opacity-50 border-t border-border-subtle" title="Always present as the lowest-priority fallback — cannot be removed">
      <span class="shrink-0 icon-sm text-transparent" aria-hidden="true">
        <svg viewBox="0 0 24 24" fill="currentColor"><circle cx="9" cy="12" r="1.5"/></svg>
      </span>
      <span class="flex-1 text-sm text-text-muted italic">${t('manga.scanlator.fallback')}</span>
    </div>
  ` : null;

  return html`
    <div class="flex flex-col gap-3">
      <div class="flex items-center gap-3">
        <span class="text-sm font-medium text-text">${t('manga.scanlator.mode')}</span>
        <${Tabs}
          variant="pill"
          tabs=${[
            { id: 'priority', name: t('manga.scanlator.mode.priority') },
            { id: 'whitelist', name: t('manga.scanlator.mode.whitelist') },
          ]}
          activeId=${mode}
          onSelect=${(/** @type {string} */ id) => handleModeChange(id)}
        />
      </div>
      <p class="text-xs text-text-muted">${mode === 'priority'
        ? t('manga.scanlator.mode.priority.desc')
        : t('manga.scanlator.mode.whitelist.desc')
      }</p>

      ${prefs.length > 0
        ? html`<div><div ref=${listContainerRef}></div>${fallbackRow}</div>`
        : html`
          <div class="flex flex-col gap-0">
            <${EmptyState} title=${mode === 'priority' ? t('manga.scanlator.empty.priority') : t('manga.scanlator.empty.whitelist')} />
            ${fallbackRow}
          </div>
        `
      }

      <div class="flex flex-wrap items-center gap-2 mt-2">
        <div class="flex-1 min-w-48">
          <${Combobox}
            options=${cmbOpts}
            value=${cmbValue}
            onChange=${(/** @type {any} */ id) => {
              setScCmbVal(cmbOpts.find(o => o.id === id)?.name ?? '');
            }}
            placeholder=${t('manga.scanlator.select_placeholder')}
          />
        </div>
        <button type="button" class="btn-ghost btn-sm" onClick=${handleAdd}>${t('common.add')}</button>
      </div>

      <label class="flex items-start gap-3 pt-3 mt-1 border-t border-border-subtle cursor-pointer">
        <input
          type="checkbox"
          class="mt-0.5"
          checked=${autoReplace}
          disabled=${autoReplaceBusy}
          onChange=${(/** @type {any} */ e) => handleAutoReplaceChange(e.currentTarget.checked)}
        />
        <span class="flex flex-col gap-0.5">
          <span class="text-sm text-text">${t('manga.scanlator.auto_replace')}</span>
          <span class="text-xs text-text-muted">${t('manga.scanlator.auto_replace.desc')}</span>
        </span>
      </label>
    </div>
  `;
}
