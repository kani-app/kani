// @ts-check
// API tokens ("Clients") settings section — Preact/htm. Lets a user mint and
// revoke long-lived tokens, either for OPDS reading apps or for integrations
// that drive the REST API.

import { h } from 'preact';
import { useState, useEffect, useCallback } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { t } from '../../i18n.js';
import { formatDate, formatRelativeTime } from '../../utils.js';
import { SettingsGroup, SettingsRow } from './_shared.js';
import { EmptyState } from '../../components/empty-state.js';
import { Modal, showConfirm } from '../../components/modal.js';
import { showApiError, showToast } from '../../components/toast.js';
import { useBusy } from '../../hooks/use-busy.js';
import { getState, hasPermission } from '../../session.js';

const html = htm.bind(h);

/** @param {any} tok */
function tokenMeta(tok) {
  const parts = [`${t('clients.created')} ${formatDate(tok.created_at)}`];
  parts.push(
    tok.last_used_at
      ? `${t('clients.last_used')} ${formatRelativeTime(new Date(tok.last_used_at))}`
      : t('clients.never_used'),
  );
  if (tok.expires_at) parts.push(`${t('clients.expires')} ${formatDate(tok.expires_at)}`);
  return parts.join(' · ');
}

/** @param {any} tok */
function scopeSummary(tok) {
  if (tok.kind !== 'api') return t('clients.kind.opds.scopes');
  const scopes = (tok.scopes || '').split(' ').filter(Boolean);
  if (scopes.length === 0) return t('clients.scopes.none');
  const stale = new Set(tok.stale_scopes || []);
  return html`<span class="inline-flex flex-wrap gap-1">
    ${scopes.map(
      (/** @type {string} */ s) => html`<code
        class="font-mono ${stale.has(s) ? 'text-warn line-through' : ''}"
        >${s}</code
      >`,
    )}
  </span>`;
}

/**
 * A GET the token can actually reach, chosen from the scopes it was granted.
 * A generic example would 403 for most tokens, which reads as "the token is
 * broken" rather than "that endpoint needs a scope you did not grant".
 * The fallback needs no permission beyond being authenticated.
 * @param {string} scopes
 */
function exampleUrl(scopes) {
  const held = new Set((scopes || '').split(' ').filter(Boolean));
  if (held.has('metrics:read')) return `${location.origin}/metrics`;
  if (held.has('library:view')) return `${location.origin}/rest/library?page=1&page_size=20`;
  if (held.has('source:browse')) return `${location.origin}/rest/sources`;
  return `${location.origin}/rest/me/api-tokens`;
}

/**
 * A labelled, read-only field with its copy affordance in a fixed place.
 * Both fields in the reveal modal use it so their labels, edges and buttons
 * line up; a single-line value and a multi-line snippet only differ by `rows`.
 * @param {{ label: string, value: string, rows?: number }} props
 */
function RevealField({ label, value, rows = 1 }) {
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      showToast(t('common.copied'), { type: 'success' });
    } catch {
      /* clipboard blocked — the field is selectable as a fallback */
    }
  };
  return html`
    <div class="flex flex-col gap-1.5">
      <div class="flex items-center justify-between gap-2 min-h-7">
        <span class="text-sm font-medium text-text">${label}</span>
        <button type="button" class="btn-ghost btn-sm" onClick=${copy}>${t('common.copy')}</button>
      </div>
      <textarea
        readonly
        rows=${String(rows)}
        class="input w-full text-sm font-mono resize-none"
        onClick=${(/** @type {Event} */ e) =>
          /** @type {HTMLTextAreaElement} */ (e.target).select()}
        value=${value}
      ></textarea>
    </div>
  `;
}

/**
 * @param {{ label: string, value: string, description?: any }} props
 */
function CopyableRow({ label, value, description }) {
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      showToast(t('common.copied'), { type: 'success' });
    } catch {
      /* clipboard blocked — the field is selectable as a fallback */
    }
  };
  return html`
    <${SettingsRow} label=${label} description=${description}>
      <div class="flex items-center gap-2">
        <input
          readonly
          class="input text-sm font-mono w-64"
          value=${value}
          onClick=${(/** @type {Event} */ e) => /** @type {HTMLInputElement} */ (e.target).select()}
        />
        <button type="button" class="btn-ghost btn-sm" onClick=${copy}>${t('common.copy')}</button>
      </div>
    <//>
  `;
}

/**
 * @param {{ open: boolean, canCreateOpds: boolean, canCreateApi: boolean,
 *           held: string[], onCreated: (tok: any) => void, onClose: () => void }} props
 */
function CreateTokenModal({ open, canCreateOpds, canCreateApi, held, onCreated, onClose }) {
  const [name, setName] = useState('');
  const [expiry, setExpiry] = useState('');
  const [kind, setKind] = useState(canCreateOpds ? 'opds' : 'api');
  const [scopes, setScopes] = useState(/** @type {Set<string>} */ (new Set()));
  const { busy, run } = useBusy();

  const reset = () => {
    setName('');
    setExpiry('');
    setKind(canCreateOpds ? 'opds' : 'api');
    setScopes(new Set());
  };

  const close = () => {
    reset();
    onClose();
  };

  const canSubmit = !busy && !!name.trim() && !(kind === 'api' && scopes.size === 0);

  const submit = async () => {
    if (!canSubmit) return;
    await run(async () => {
      try {
        const created = await api.createApiToken(
          name.trim(),
          expiry ? Number(expiry) : null,
          kind,
          kind === 'api' ? [...scopes] : [],
        );
        reset();
        onCreated(created);
      } catch (err) {
        showApiError(err);
      }
    });
  };

  const toggleScope = (/** @type {string} */ perm, /** @type {boolean} */ on) => {
    const next = new Set(scopes);
    if (on) next.add(perm);
    else next.delete(perm);
    setScopes(next);
  };

  return html`
    <${Modal}
      open=${open}
      title=${t('clients.create.title')}
      onClose=${close}
      footer=${html`
        <button type="button" class="btn-ghost btn-sm" onClick=${close}>
          ${t('common.cancel')}
        </button>
        <button
          type="button"
          class="btn-primary btn-sm"
          onClick=${submit}
          disabled=${!canSubmit}
        >
          ${t('clients.create')}
        </button>
      `}
    >
      <div class="flex flex-col gap-4 px-1">
        <div class="flex flex-col gap-1.5">
          <label class="text-sm font-medium text-text" for="token-name">
            ${t('clients.name.label')}
          </label>
          <input
            type="text"
            id="token-name"
            class="input"
            autoFocus
            placeholder=${kind === 'api'
              ? t('clients.name.placeholder.api')
              : t('clients.name.placeholder.opds')}
            value=${name}
            maxLength="100"
            onInput=${(/** @type {any} */ e) => setName(e.target.value)}
            onKeyDown=${(/** @type {KeyboardEvent} */ e) => e.key === 'Enter' && submit()}
          />
        </div>

        <div class="flex flex-col gap-1.5">
          <label class="text-sm font-medium text-text" for="token-kind">
            ${t('clients.kind.label')}
          </label>
          ${canCreateOpds && canCreateApi
            ? html`<select
                id="token-kind"
                class="input"
                value=${kind}
                onChange=${(/** @type {any} */ e) => setKind(e.target.value)}
              >
                <option value="opds">${t('clients.kind.opds')}</option>
                <option value="api">${t('clients.kind.api')}</option>
              </select>`
            : html`<span class="text-sm text-text">${t(`clients.kind.${kind}`)}</span>`}
          <span class="text-xs text-text-muted">
            ${kind === 'api' ? t('clients.kind.api.desc') : t('clients.kind.opds.desc')}
          </span>
        </div>

        ${kind === 'api'
          ? html`<div class="flex flex-col gap-1.5">
              <div class="flex items-baseline justify-between gap-2">
                <span class="text-sm font-medium text-text">${t('clients.scopes.label')}</span>
                <span class="text-xs text-text-muted tabular-nums"
                  >${t('clients.scopes.count', { n: scopes.size, total: held.length })}</span
                >
              </div>
              <span class="text-xs text-text-muted">${t('clients.scopes.desc')}</span>
              <div
                class="grid grid-cols-1 sm:grid-cols-2 gap-x-4 gap-y-1 max-h-56 overflow-y-auto"
              >
                ${held.map(
                  (/** @type {string} */ perm) => html`
                    <label key=${perm} class="flex items-center gap-2 text-sm cursor-pointer">
                      <input
                        type="checkbox"
                        class="accent-accent cursor-pointer"
                        checked=${scopes.has(perm)}
                        onChange=${(/** @type {any} */ e) => toggleScope(perm, e.target.checked)}
                      />
                      <code class="font-mono text-xs">${perm}</code>
                    </label>
                  `,
                )}
              </div>
              ${scopes.size === 0
                ? html`<p class="text-xs text-text-muted">${t('clients.scopes.required')}</p>`
                : null}
            </div>`
          : null}

        <div class="flex flex-col gap-1.5">
          <label class="text-sm font-medium text-text" for="token-expiry">
            ${t('clients.expiry.label')}
          </label>
          <select
            id="token-expiry"
            class="input"
            value=${expiry}
            onChange=${(/** @type {any} */ e) => setExpiry(e.target.value)}
          >
            <option value="">${t('clients.expiry.never')}</option>
            <option value="30">${t('clients.expiry.30')}</option>
            <option value="90">${t('clients.expiry.90')}</option>
            <option value="365">${t('clients.expiry.365')}</option>
          </select>
        </div>
      </div>
    <//>
  `;
}

export function ClientsSection() {
  const [tokens, setTokens] = useState(/** @type {any[] | null} */ (null));
  const [reveal, setReveal] = useState(/** @type {any} */ (null));
  const [addOpen, setAddOpen] = useState(false);

  const canCreateOpds = hasPermission('token:create_opds');
  const canCreateApi = hasPermission('token:create_api');
  const held = [...(getState('permissions') || [])].sort();

  const load = useCallback(async () => {
    try {
      setTokens(await api.listApiTokens());
    } catch (e) {
      showApiError(e);
      setTokens([]);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const onRevoke = async (/** @type {any} */ tok) => {
    const ok = await showConfirm(t('clients.revoke.confirm'), {
      title: t('clients.revoke'),
      confirmLabel: t('clients.revoke'),
      danger: true,
    });
    if (!ok) return;
    try {
      await api.revokeApiToken(tok.id);
      await load();
    } catch (e) {
      showApiError(e);
    }
  };

  const opdsUrl = `${location.origin}/opds`;
  const apiUrl = `${location.origin}/rest`;
  const canCreate = canCreateOpds || canCreateApi;

  const listBody =
    tokens === null
      ? null
      : tokens.length === 0
        ? html`<div class="p-2">
            <${EmptyState} title=${t('clients.empty')} subtitle=${t('clients.empty.subtitle')} />
          </div>`
        : tokens.map(
            (tok) => html`
              <${SettingsRow}
                key=${tok.id}
                label=${html`<span class="inline-flex items-baseline gap-2">
                  <span>${tok.name}</span>
                  <span class="text-xs text-text-muted font-normal"
                    >${t(`clients.kind.${tok.kind}`)}</span
                  >
                </span>`}
                badge=${tok.stale_scopes?.length ? t('clients.scopes.stale.badge') : null}
                description=${html`${tokenMeta(tok)}
                  <span class="block mt-0.5">${scopeSummary(tok)}</span>
                  ${tok.stale_scopes?.length
                    ? html`<span class="block mt-0.5 text-warn"
                        >${t('clients.scopes.stale.desc')}</span
                      >`
                    : null}`}
              >
                <button
                  type="button"
                  class="btn-ghost btn-sm text-danger"
                  onClick=${() => onRevoke(tok)}
                >
                  ${t('clients.revoke')}
                </button>
              <//>
            `,
          );

  return html`
    <div class="flex flex-col gap-6">
      <${SettingsGroup} label=${t('clients.list.title')}>
        ${listBody}
        ${canCreate
          ? html`<${SettingsRow} label=${t('clients.add.label')} description=${t('clients.add.desc')}>
              <button type="button" class="btn-secondary btn-sm" onClick=${() => setAddOpen(true)}>
                ${t('clients.add_btn')}
              </button>
            <//>`
          : null}
      <//>

      <${CreateTokenModal}
        open=${addOpen}
        canCreateOpds=${canCreateOpds}
        canCreateApi=${canCreateApi}
        held=${held}
        onClose=${() => setAddOpen(false)}
        onCreated=${async (/** @type {any} */ created) => {
          setAddOpen(false);
          setReveal(created);
          await load();
        }}
      />

      <${SettingsGroup} label=${t('clients.endpoints.group')}>
        <${CopyableRow}
          label=${t('clients.opds.url.label')}
          description=${t('clients.opds.url.desc')}
          value=${opdsUrl}
        />
        <${CopyableRow}
          label=${t('clients.api.url.label')}
          description=${t('clients.api.url.desc')}
          value=${apiUrl}
        />
      <//>

      <${Modal}
        open=${!!reveal}
        onClose=${() => setReveal(null)}
        title=${t('clients.token.reveal.title')}
        footer=${html`
          <button type="button" class="btn-secondary btn-sm" onClick=${() => setReveal(null)}>
            ${t('common.done')}
          </button>
        `}
      >
        ${reveal &&
        (() => {
          const isApi = reveal.kind === 'api';
          const snippet = isApi
            ? `curl -H "Authorization: Bearer ${reveal.raw_token}" \\\n  "${exampleUrl(reveal.scopes)}"` // i18n-ignore — shell line continuation
            : opdsUrl;
          return html`
            <div class="flex flex-col gap-4">
              <p class="text-sm text-warn">${t('clients.token.reveal.warning')}</p>
              <${RevealField}
                label=${t('clients.token.label')}
                value=${reveal.raw_token}
                rows=${2}
              />
              <${RevealField}
                label=${isApi ? t('clients.token.reveal.try') : t('clients.opds.url.label')}
                value=${snippet}
                rows=${isApi ? 4 : 2}
              />
            </div>
          `;
        })()}
      <//>
    </div>
  `;
}
