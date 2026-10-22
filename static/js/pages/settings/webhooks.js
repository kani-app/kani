// @ts-check

import { h } from 'preact';
import { useState, useEffect, useCallback } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { showToast, showApiError } from '../../components/toast.js';
import { SettingsGroup, SettingsRow } from './_shared.js';
import { showConfirm } from '../../components/modal.js';
import { t } from '../../i18n.js';
import { ErrorState } from '../../components/error-state.js';
import { EmptyState } from '../../components/empty-state.js';
import { skeletonSettingsCards } from '../../components/skeletons.js';
import { useBusy } from '../../hooks/use-busy.js';

const html = htm.bind(h);

const ALL_EVENTS = [
  { value: 'chapter.new', get label() { return t('settings.webhooks.event.chapter_new.label'); }, get description() { return t('settings.webhooks.event.chapter_new.desc'); } },
  { value: 'manga.added', get label() { return t('settings.webhooks.event.manga_added.label'); }, get description() { return t('settings.webhooks.event.manga_added.desc'); } },
  { value: 'manga.deleted', get label() { return t('settings.webhooks.event.manga_deleted.label'); }, get description() { return t('settings.webhooks.event.manga_deleted.desc'); } },
  { value: 'chapter.downloaded', get label() { return t('settings.webhooks.event.chapter_downloaded.label'); }, get description() { return t('settings.webhooks.event.chapter_downloaded.desc'); } },
  { value: 'scan.completed', get label() { return t('settings.webhooks.event.scan_completed.label'); }, get description() { return t('settings.webhooks.event.scan_completed.desc'); } },
];

/** @param {string} eventsJson @returns {{ all: boolean, selected: Set<string> }} */
function parseEvents(eventsJson) {
  let arr;
  try {
    arr = JSON.parse(eventsJson);
  } catch {
    arr = ['*'];
  }
  if (!Array.isArray(arr)) arr = ['*'];
  const all = arr.includes('*');
  return { all, selected: new Set(all ? ALL_EVENTS.map((e) => e.value) : arr) };
}

/** @param {{ all: boolean, selected: Set<string> }} state @returns {string} */
function serializeEvents(state) {
  if (state.all) return '["*"]';
  return JSON.stringify(ALL_EVENTS.map((e) => e.value).filter((v) => state.selected.has(v)));
}

function WebhookForm({ existing, onSave, onCancel }) {
  const st = parseEvents(existing?.events ?? '["*"]');
  const [url, setUrl] = useState(existing?.url ?? '');
  const [secret, setSecret] = useState('');
  const [all, setAll] = useState(st.all);
  const [selected, setSelected] = useState(new Set(st.selected));
  const { busy, run } = useBusy();

  const toggleEvent = (/** @type {string} */ v) =>
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(v)) n.delete(v);
      else n.add(v);
      return n;
    });

  const save = () =>
    run(async () => {
      const u = url.trim();
      if (!u.startsWith('http://') && !u.startsWith('https://')) {
        showToast(t('settings.webhooks.form.url_error'), { type: 'error' });
        return;
      }
      const data = /** @type {any} */ ({ url: u, events: serializeEvents({ all, selected }) });
      if (secret) data.secret = secret;
      try {
        await onSave(data);
      } catch (e) {
        showApiError(e);
      }
    });

  return html`
    <div class="bg-surface-2 rounded-xl p-4 flex flex-col gap-3 mb-4">
      <p class="text-sm font-semibold text-text">
        ${existing ? t('settings.webhooks.form.edit') : t('settings.webhooks.form.add')}
      </p>
      <label class="flex flex-col gap-1">
        <span class="text-xs text-text-muted font-medium"
          >${t('settings.webhooks.form.url')} <span class="text-danger">*</span></span
        >
        <input
          type="url"
          class="input text-sm"
          placeholder="https://example.com/hook"
          value=${url}
          onInput=${(e) => setUrl(e.target.value)}
        />
      </label>
      <label class="flex flex-col gap-1">
        <span class="text-xs text-text-muted font-medium">${t('settings.webhooks.form.secret')}</span>
        <input
          type="password"
          class="input text-sm"
          autocomplete="new-password"
          placeholder=${existing
            ? t('settings.webhooks.form.secret.unchanged')
            : t('settings.webhooks.form.secret.placeholder')}
          value=${secret}
          onInput=${(e) => setSecret(e.target.value)}
        />
      </label>
      <div class="flex flex-col gap-1.5">
        <span class="text-xs text-text-muted font-medium">${t('settings.webhooks.form.events')}</span>
        <label class="flex items-center gap-2 text-sm text-text cursor-pointer">
          <input type="checkbox" class="rounded" checked=${all} onChange=${(e) => setAll(e.target.checked)} />
          ${t('settings.webhooks.event.all')}
        </label>
        <div class="ml-4 flex flex-col gap-1">
          ${ALL_EVENTS.map(
            (ev) => html`
              <label class="flex items-center gap-2 text-sm text-text cursor-pointer">
                <input
                  type="checkbox"
                  class="rounded"
                  checked=${all || selected.has(ev.value)}
                  disabled=${all}
                  onChange=${() => toggleEvent(ev.value)}
                />
                ${ev.label}
                <span class="text-xs text-text-muted">— ${ev.description}</span>
              </label>
            `,
          )}
        </div>
      </div>
      <div class="flex items-center gap-2 mt-1">
        <button type="button" class="btn-primary btn-sm" disabled=${busy} onClick=${save}>
          ${existing ? t('settings.webhooks.form.save_changes') : t('settings.webhooks.form.add')}
        </button>
        <button type="button" class="btn-ghost btn-sm" onClick=${onCancel}>${t('common.cancel')}</button>
      </div>
    </div>
  `;
}

function DeliveryLog({ deliveries }) {
  if (deliveries.length === 0) {
    return html`<p class="text-xs text-text-muted">${t('settings.webhooks.deliveries.empty')}</p>`;
  }
  return html`
    <div class="overflow-x-auto">
    <table class="data-table mt-1">
      <thead>
        <tr>
          <th>${t('settings.webhooks.deliveries.col.event')}</th>
          <th>${t('settings.webhooks.deliveries.col.status')}</th>
          <th>${t('settings.webhooks.deliveries.col.time')}</th>
        </tr>
      </thead>
      <tbody>
        ${deliveries.map((d, i) => {
          const ok = d.http_status && d.http_status >= 200 && d.http_status < 300;
          return html`
            <tr key=${i}>
              <td>${d.event_type}</td>
              <td class=${ok ? 'text-success' : 'text-danger'}>
                ${d.http_status ?? '—'}${d.error ? ` · ${d.error.slice(0, 60)}` : ''}
              </td>
              <td class="muted">${new Date(d.delivered_at).toLocaleString()}</td>
            </tr>
          `;
        })}
      </tbody>
    </table>
    </div>
  `;
}

function DeliveryLogToggle({ wh }) {
  const [open, setOpen] = useState(false);
  const [state, setState] = useState(
    /** @type {{ status: string, data: any[] }} */ ({ status: 'idle', data: [] }),
  );

  const toggle = async () => {
    const next = !open;
    setOpen(next);
    if (next && state.status === 'idle') {
      setState({ status: 'loading', data: [] });
      try {
        const d = await api.listWebhookDeliveries(wh.id);
        setState({ status: 'ready', data: Array.isArray(d) ? d : [] });
      } catch {
        setState({ status: 'error', data: [] });
      }
    }
  };

  return html`
    <div class="flex flex-col gap-1">
      <button
        type="button"
        class="text-xs text-text-muted hover:text-accent transition-colors self-start"
        onClick=${toggle}
      >
        ${open ? t('settings.webhooks.deliveries.hide') : t('settings.webhooks.deliveries.show')}
      </button>
      ${open &&
      (state.status === 'loading'
        ? html`${html([skeletonSettingsCards(3)])}`
        : state.status === 'error'
        ? html`<${ErrorState} message=${t('settings.webhooks.deliveries.load_failed')} />`
        : html`<${DeliveryLog} deliveries=${state.data} />`)}
    </div>
  `;
}

function WebhookRow({ wh, reload }) {
  const [enabled, setEnabled] = useState(wh.enabled);
  const [editing, setEditing] = useState(false);
  const { busy: testing, run: runTest } = useBusy();

  const toggleEnabled = async () => {
    const next = !enabled;
    setEnabled(next);
    try {
      await api.updateWebhook(wh.id, { enabled: next });
    } catch {
      setEnabled(!next);
    }
  };

  const test = () =>
    runTest(async () => {
      try {
        const r = await api.testWebhook(wh.id);
        showToast(
          r?.ok
            ? t('settings.webhooks.row.test.success')
            : t('settings.webhooks.row.test.failed', {
                error: r?.error ?? t('settings.webhooks.row.test.default_error'),
              }),
          { type: r?.ok ? 'success' : 'error' },
        );
      } catch (e) {
        showApiError(e);
      }
    });

  const del = async () => {
    if (
      !(await showConfirm(t('settings.webhooks.row.delete.message', { url: wh.url }), {
        title: t('settings.webhooks.row.delete.title'),
        confirmLabel: t('common.delete'),
        danger: true,
      }))
    )
      return;
    try {
      await api.deleteWebhook(wh.id);
      await reload();
    } catch (e) {
      showApiError(e);
    }
  };

  const eventState = parseEvents(wh.events);
  const eventLabels = eventState.all
    ? [{ label: t('settings.webhooks.event.all'), value: '*' }]
    : ALL_EVENTS.filter((e) => eventState.selected.has(e.value));

  return html`
    <div class="flex flex-col gap-2 px-4 py-3">
      <div class="flex items-start justify-between gap-3">
        <div class="flex flex-col gap-1 min-w-0">
          <span class="text-sm font-medium text-text truncate" title=${wh.url}
            >${wh.url.length > 60 ? wh.url.slice(0, 57) + '…' : wh.url}</span
          >
          <div class="flex flex-wrap gap-1 mt-0.5">
            ${eventLabels.map(
              (ev) => html`<span
                class="text-xs px-1.5 py-0.5 rounded bg-accent/15 text-accent font-medium"
                >${ev.label ?? ev.value}</span
              >`,
            )}
          </div>
        </div>
        <div class="flex items-center gap-2 shrink-0">
          <label class="kani-toggle">
            <input type="checkbox" class="kani-toggle__input" checked=${enabled} onChange=${toggleEnabled} />
            <span class="kani-toggle__track"></span>
          </label>
          <button type="button" class="btn-ghost btn-sm text-xs" disabled=${testing} onClick=${test}>
            ${testing ? '…' : t('settings.webhooks.row.test')}
          </button>
          <button type="button" class="btn-ghost btn-sm text-xs" onClick=${() => setEditing((e) => !e)}>
            ${editing ? t('common.cancel') : t('common.edit')}
          </button>
          <button type="button" class="btn-ghost btn-sm text-xs text-danger" onClick=${del}>
            ${t('common.delete')}
          </button>
        </div>
      </div>
      <${DeliveryLogToggle} wh=${wh} />
      ${editing &&
      html`<${WebhookForm}
        existing=${wh}
        onSave=${async (/** @type {any} */ data) => {
          await api.updateWebhook(wh.id, data);
          setEditing(false);
          await reload();
        }}
        onCancel=${() => setEditing(false)}
      />`}
    </div>
  `;
}

const EXAMPLE_PAYLOAD = JSON.stringify(
  {
    event: 'chapter.new',
    timestamp: '2026-05-19T14:30:00Z',
    data: {
      manga_id: 42,
      manga_name: 'One Piece',
      chapter_count: 3,
      chapter_ids: [101, 102, 103],
      chapter_names: ['Ch. 1101', 'Ch. 1102', 'Ch. 1103'],
    },
  },
  null,
  2,
);

const HMAC_SNIPPET = `# Python
import hmac, hashlib
sig = 'sha256=' + hmac.new(secret.encode(), body, hashlib.sha256).hexdigest()
assert sig == request.headers['X-Kani-Signature']

# Node.js
const sig = 'sha256=' + crypto.createHmac('sha256', secret).update(body).digest('hex');
assert.strictEqual(sig, req.headers['x-kani-signature']);`;

function PayloadReference() {
  return html`
    <${SettingsGroup} label=${t('settings.webhooks.payload.group')}>
      <details class="px-4 py-3">
        <summary class="text-sm font-medium text-text cursor-pointer select-none">
          ${t('settings.webhooks.payload.show')}
        </summary>
        <pre
          class="mt-3 text-xs bg-surface rounded-lg p-3 overflow-x-auto text-text-muted leading-relaxed"
        >${EXAMPLE_PAYLOAD}</pre>
        <p class="mt-3 text-xs text-text-muted">
          ${t('settings.webhooks.payload.hmac_prefix')}${' '}
          <code class="bg-surface px-1 rounded">X-Kani-Signature: sha256=&lt;hex&gt;${/* i18n-ignore */ ''}</code>${' '}
          ${t('settings.webhooks.payload.hmac_suffix')}
        </p>
        <pre
          class="mt-2 text-xs bg-surface rounded-lg p-3 overflow-x-auto text-text-muted leading-relaxed"
        >${HMAC_SNIPPET}</pre>
      </details>
    <//>
  `;
}

export function WebhooksSection() {
  const [state, setState] = useState(
    /** @type {{ status: string, list: any[] }} */ ({ status: 'loading', list: [] }),
  );
  const [addOpen, setAddOpen] = useState(false);

  const load = useCallback(async () => {
    setState((s) => ({ ...s, status: 'loading' }));
    try {
      const list = await api.listWebhooks();
      setState({ status: 'ready', list: Array.isArray(list) ? list : [] });
    } catch {
      setState({ status: 'error', list: [] });
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  let listContent;
  if (state.status === 'loading') listContent = html`${html([skeletonSettingsCards(2)])}`;
  else if (state.status === 'error') {
    listContent = html`<${ErrorState} message=${t('settings.webhooks.load_failed')} onRetry=${load} />`;
  } else if (state.list.length === 0) {
    listContent = html`<${EmptyState}
      title=${t('settings.webhooks.empty.title')}
      subtitle=${t('settings.webhooks.empty.desc')}
    />`;
  } else {
    listContent = html`<div class="divide-y divide-border-subtle">
      ${state.list.map((wh) => html`<${WebhookRow} key=${wh.id} wh=${wh} reload=${load} />`)}
    </div>`;
  }

  return html`
    <${SettingsGroup} label=${t('settings.webhooks.group')}>
      ${listContent}
      <${SettingsRow}
        label=${t('settings.webhooks.add.label')}
        description=${t('settings.webhooks.add.desc')}
      >
        <button type="button" class="btn-ghost btn-sm" onClick=${() => setAddOpen((o) => !o)}>
          ${t('settings.webhooks.add_btn')}
        </button>
      <//>
    <//>
    ${addOpen &&
    html`<${WebhookForm}
      existing=${null}
      onSave=${async (/** @type {any} */ data) => {
        await api.createWebhook(data);
        setAddOpen(false);
        await load();
      }}
      onCancel=${() => setAddOpen(false)}
    />`}
    <${PayloadReference} />
  `;
}
