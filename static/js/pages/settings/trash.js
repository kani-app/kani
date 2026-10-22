// @ts-check

import { h } from 'preact';
import { useState, useEffect, useCallback } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { iconTrash } from '../../icons.js';
import { t } from '../../i18n.js';
import { showToast, showApiError } from '../../components/toast.js';
import { showConfirm } from '../../components/modal.js';
import { EmptyState } from '../../components/empty-state.js';
import { ErrorState } from '../../components/error-state.js';
import { SettingsGroup } from './_shared.js';
import { useBusy } from '../../hooks/use-busy.js';

const html = htm.bind(h);

export function TrashSection() {
  const [state, setState] = useState(
    /** @type {{ status: string, items: any[], error: string }} */ ({
      status: 'loading',
      items: [],
      error: '',
    }),
  );
  const emptyBusy = useBusy();

  const load = useCallback(async () => {
    setState((s) => ({ ...s, status: 'loading' }));
    try {
      const items = await api.listTrash();
      setState({ status: 'ready', items: Array.isArray(items) ? items : [], error: '' });
    } catch (e) {
      setState({ status: 'error', items: [], error: e?.message ?? t('trash.error.load') });
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const onEmptyAll = () =>
    emptyBusy.run(async () => {
      if (
        !(await showConfirm(t('trash.confirm.empty'), {
          title: t('trash.action.empty'),
          confirmLabel: t('trash.action.purge'),
        }))
      )
        return;
      try {
        const res = await api.purgeTrashAll();
        showToast(t('trash.toast.emptied', { count: res?.purged ?? 0 }), { type: 'success' });
        await load();
      } catch (e) {
        showApiError(e);
      }
    });

  const onRestore = async (/** @type {any} */ m) => {
    try {
      await api.untrashManga(m.id);
      showToast(t('trash.toast.restored', { title: m.name ?? '' }), { type: 'success' });
      await load();
    } catch (e) {
      showApiError(e);
    }
  };

  const onPurge = async (/** @type {any} */ m) => {
    if (
      !(await showConfirm(t('trash.confirm.purge', { title: m.name ?? '' }), {
        confirmLabel: t('trash.action.purge'),
      }))
    )
      return;
    try {
      await api.purgeTrashOne(m.id);
      showToast(t('trash.toast.purged', { title: m.name ?? '' }), { type: 'success' });
      await load();
    } catch (e) {
      showApiError(e);
    }
  };

  if (state.status === 'loading') {
    return html`<div class="text-sm text-text-muted px-1 py-4">${t('common.loading')}</div>`;
  }
  if (state.status === 'error') {
    return html`<${ErrorState} message=${state.error} onRetry=${load} />`;
  }
  if (state.items.length === 0) {
    return html`<${EmptyState} title=${t('trash.empty.title')} subtitle=${t('trash.empty.desc')} />`;
  }

  const n = state.items.length;
  return html`
    <${SettingsGroup}>
      <div class="detail-card-head">
        <span>${n === 1 ? t('trash.count.one', { n }) : t('trash.count.other', { n })}</span>
        <button
          type="button"
          class="btn-danger btn-sm"
          disabled=${emptyBusy.busy}
          onClick=${onEmptyAll}
        >
          ${t('trash.action.empty')}
        </button>
      </div>
      <div class="divide-y divide-border-subtle">
        ${state.items.map(
          (m) => html`
            <div class="flex items-center gap-3 px-4 py-3" key=${m.id}>
              <span class="flex-1 text-sm text-text truncate">${m.name ?? 'Unknown'}</span>
              <span class="text-xs text-text-muted shrink-0"
                >${m.deleted_at ? new Date(m.deleted_at).toLocaleDateString() : ''}</span
              >
              <button
                type="button"
                class="btn-secondary btn-sm shrink-0"
                onClick=${() => onRestore(m)}
              >
                ${t('trash.action.restore')}
              </button>
              <button
                type="button"
                class="btn-icon text-danger shrink-0"
                aria-label=${t('trash.action.purge')}
                onClick=${() => onPurge(m)}
              >
                ${html([iconTrash])}
              </button>
            </div>
          `,
        )}
      </div>
    <//>
  `;
}
