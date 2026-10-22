// @ts-check

import { h, Fragment } from 'preact';
import { useState, useEffect, useCallback } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { t } from '../../i18n.js';
import { iconTrash } from '../../icons.js';
import { showToast, showApiError } from '../../components/toast.js';
import { showConfirm, Modal } from '../../components/modal.js';
import { EmptyState } from '../../components/empty-state.js';
import { ErrorState } from '../../components/error-state.js';
import { SettingsGroup } from './_shared.js';
import { useBusy } from '../../hooks/use-busy.js';

const html = htm.bind(h);

const STATUS_LABELS = ['Ongoing', 'Completed', 'Hiatus', 'Cancelled', 'Unknown'];

function describeRule(ruleJson) {
  try {
    const r = JSON.parse(ruleJson);
    switch (r.op) {
      case 'has_unread': return t('collections.rule.has_unread');
      case 'status': return `${t('collections.rule.status')}: ${STATUS_LABELS[r.value] ?? r.value}`;
      case 'tag': return `${t('collections.rule.tag')}: ${r.name}`;
      case 'chapter_count_gt': return t('collections.rule.chapter_count_gt_label', { count: r.value });
      case 'chapter_count_lt': return t('collections.rule.chapter_count_lt_label', { count: r.value });
      case 'and': return t('collections.rule.and', { count: r.rules?.length ?? 0 });
      case 'or': return t('collections.rule.or', { count: r.rules?.length ?? 0 });
      default: return r.op ?? '—';
    }
  } catch {
    return '—';
  }
}

function parseSimpleRule(ruleJson) {
  try {
    const r = JSON.parse(ruleJson);
    if (r.op === 'and' || r.op === 'or') return null;
    return r;
  } catch {
    return null;
  }
}

const RULE_TYPES = [
  ['has_unread', 'collections.rule.has_unread'],
  ['status', 'collections.rule.status'],
  ['tag', 'collections.rule.tag'],
  ['chapter_count_gt', 'collections.rule.chapter_count_gt'],
  ['chapter_count_lt', 'collections.rule.chapter_count_lt'],
];

/** @param {{ rule: any, setRule: (r:any)=>void }} props */
function RuleBuilder({ rule, setRule }) {
  const op = rule.op ?? 'has_unread';
  const setOp = (/** @type {string} */ newOp) => {
    if (newOp === 'has_unread') setRule({ op: newOp });
    else if (newOp === 'status') setRule({ op: newOp, value: 0 });
    else if (newOp === 'tag') setRule({ op: newOp, name: '' });
    else setRule({ op: newOp, value: 0 });
  };
  return html`
    <div class="flex items-center gap-2 flex-wrap">
      <select class="input text-sm" value=${op} onChange=${(e) => setOp(e.target.value)}>
        ${RULE_TYPES.map(([v, key]) => html`<option value=${v}>${t(key)}</option>`)}
      </select>
      ${op === 'status'
        ? html`<select
            class="input text-sm"
            value=${String(rule.value ?? 0)}
            onChange=${(e) => setRule({ op, value: Number(e.target.value) })}
          >
            ${STATUS_LABELS.map((l, i) => html`<option value=${i}>${l}</option>`)}
          </select>`
        : op === 'tag'
        ? html`<input
            type="text"
            class="input text-sm w-36"
            placeholder=${t('collections.tag.placeholder')}
            value=${rule.name ?? ''}
            onInput=${(e) => setRule({ op, name: e.target.value })}
          />`
        : op === 'chapter_count_gt' || op === 'chapter_count_lt'
        ? html`<input
            type="number"
            class="input text-sm w-20"
            min="0"
            step="1"
            placeholder="0"
            value=${String(rule.value ?? 0)}
            onInput=${(e) => setRule({ op, value: Number(e.target.value) })}
          />`
        : null}
    </div>
  `;
}

/** @param {{ col: any, onChanged: () => void }} props */
function CollectionRow({ col, onChanged }) {
  const simple = parseSimpleRule(col.rule_json);
  const [editing, setEditing] = useState(false);
  const [matches, setMatches] = useState(/** @type {any[]|null} */ (null));
  const [name, setName] = useState(col.name);
  const [rule, setRule] = useState(simple ?? { op: 'has_unread' });
  const { busy, run } = useBusy();

  const cancel = () => {
    setEditing(false);
    setName(col.name);
    setRule(simple ?? { op: 'has_unread' });
  };

  const save = () =>
    run(async () => {
      if (!name.trim()) return;
      try {
        await api.updateCollection(col.id, { name: name.trim(), rule, sort_order: col.sort_order });
        showToast(t('collections.toast.updated'), { type: 'success' });
        setEditing(false);
        onChanged();
      } catch (e) {
        showApiError(e);
      }
    });

  const del = async () => {
    if (
      !(await showConfirm(t('collections.delete.confirm', { name: col.name }), {
        confirmLabel: t('common.delete'),
      }))
    )
      return;
    try {
      await api.deleteCollection(col.id);
      showToast(t('collections.toast.deleted'), { type: 'success' });
      onChanged();
    } catch (e) {
      showApiError(e);
    }
  };

  // A smart collection's membership is derived from its rule, so the only way
  // to know what it currently matches is to ask. `getCollectionManga` had no
  // caller, which meant a rule could be written with no way to check it.
  const preview = () =>
    run(async () => {
      try {
        const res = await api.getCollectionManga(col.id);
        const items = Array.isArray(res) ? res : (res?.manga ?? []);
        setMatches(items);
      } catch (e) {
        showApiError(e);
      }
    });

  if (editing) {
    return html`
      <div class="px-4 py-3 flex flex-col gap-2" data-settings-row>
        <input
          type="text"
          class="input text-sm w-full"
          value=${name}
          onInput=${(e) => setName(e.target.value)}
        />
        <${RuleBuilder} rule=${rule} setRule=${setRule} />
        <div class="flex gap-2">
          <button type="button" class="btn-primary btn-sm" disabled=${busy} onClick=${save}>
            ${t('common.save')}
          </button>
          <button type="button" class="btn-ghost btn-sm" onClick=${cancel}>
            ${t('common.cancel')}
          </button>
        </div>
      </div>
    `;
  }

  return html`<${Fragment}>
    <div class="flex items-center gap-3 px-4 py-3" data-settings-row>
      <span class="flex-1 text-sm font-medium text-text truncate">${col.name}</span>
      <span class="text-xs text-text-muted shrink-0 max-w-[10rem] truncate"
        >${describeRule(col.rule_json)}</span
      >
      <button type="button" class="btn-ghost btn-sm shrink-0" disabled=${busy} onClick=${preview}>
        ${t('collections.preview')}
      </button>
      ${simple &&
      html`<button type="button" class="btn-ghost btn-sm shrink-0" onClick=${() => setEditing(true)}>
        ${t('common.edit')}
      </button>`}
      <button
        type="button"
        class="btn-icon text-danger shrink-0"
        aria-label=${t('common.delete')}
        onClick=${del}
      >
        ${html([iconTrash])}
      </button>
    </div>
    ${matches !== null &&
    html`<${Modal}
      open=${true}
      title=${t('collections.preview.title', { name: col.name })}
      onClose=${() => setMatches(null)}
    >
      <div class="flex flex-col gap-1 px-1">
        <p class="text-xs text-text-muted pb-1">
          ${t('collections.preview.count', { n: matches.length })}
        </p>
        ${matches.length === 0
          ? html`<p class="text-sm text-text-muted">${t('collections.preview.empty')}</p>`
          : matches.map(
              (m) => html`<a
                key=${m.id}
                href=${`/manga/${m.id}`}
                class="text-sm text-text truncate px-1 py-1.5 border-b border-border-subtle hover:bg-surface-hover"
                onClick=${() => setMatches(null)}
                >${m.name ?? m.title}</a
              >`,
            )}
      </div>
    <//>`}
  <//>`;
}

/** @param {{ onAdded: () => void }} props */
function AddCollection({ onAdded }) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  const [rule, setRule] = useState({ op: 'has_unread' });
  const { busy, run } = useBusy();

  const submit = () =>
    run(async () => {
      if (!name.trim()) return;
      try {
        await api.createCollection({ name: name.trim(), rule, sort_order: 0 });
        showToast(t('collections.toast.created'), { type: 'success' });
        setOpen(false);
        setName('');
        setRule({ op: 'has_unread' });
        onAdded();
      } catch (e) {
        showApiError(e);
      }
    });

  if (!open) {
    return html`
      <div class="border-t border-border-subtle px-4 py-3">
        <button type="button" class="btn-secondary btn-sm" onClick=${() => setOpen(true)}>
          ${t('collections.add')}
        </button>
      </div>
    `;
  }

  return html`
    <div class="border-t border-border-subtle px-4 py-3 flex flex-col gap-2">
      <input
        type="text"
        class="input text-sm w-full"
        placeholder=${t('collections.name.placeholder')}
        value=${name}
        onInput=${(e) => setName(e.target.value)}
      />
      <${RuleBuilder} rule=${rule} setRule=${setRule} />
      <div class="flex gap-2">
        <button type="button" class="btn-primary btn-sm" disabled=${busy} onClick=${submit}>
          ${t('collections.add')}
        </button>
        <button
          type="button"
          class="btn-ghost btn-sm"
          onClick=${() => {
            setOpen(false);
            setName('');
          }}
        >
          ${t('common.cancel')}
        </button>
      </div>
    </div>
  `;
}

export function CollectionsSection() {
  const [state, setState] = useState(
    /** @type {{ status: string, cols: any[], error: string }} */ ({
      status: 'loading',
      cols: [],
      error: '',
    }),
  );

  const load = useCallback(async () => {
    setState((s) => ({ ...s, status: 'loading' }));
    try {
      const cols = await api.listCollections();
      setState({ status: 'ready', cols: Array.isArray(cols) ? cols : [], error: '' });
    } catch (e) {
      setState({ status: 'error', cols: [], error: e?.message ?? t('collections.error.load') });
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  if (state.status === 'loading') {
    return html`<div class="text-sm text-text-muted px-1 py-4">${t('common.loading')}</div>`;
  }
  if (state.status === 'error') {
    return html`<${ErrorState} message=${state.error} onRetry=${load} />`;
  }

  return html`
    <${SettingsGroup}>
      ${state.cols.length === 0
        ? html`<${EmptyState}
            title=${t('collections.empty.title')}
            subtitle=${t('collections.empty.desc')}
          />`
        : html`<div class="divide-y divide-border-subtle">
            ${state.cols.map((col) => html`<${CollectionRow} key=${col.id} col=${col} onChanged=${load} />`)}
          </div>`}
      <${AddCollection} onAdded=${load} />
    <//>
  `;
}
