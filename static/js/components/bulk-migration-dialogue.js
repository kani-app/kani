// @ts-check

import { h } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';
import htm from 'htm';
import * as api from '../api.js';
import { Modal, mountIntoModalRoot } from './modal.js';
import { Select } from './form/select.js';
import { Callout } from './form/callout.js';
import { t } from '../i18n.js';
import { chunk, planBatch, jobErrorMessage } from '../bulk-migration.js';

const html = htm.bind(h);

const MATCH_CHUNK = 5;
const STATUS_CHUNK = 100;
const POLL_MS = 4000;

/**
 * @typedef {{ id: number, title: string, cover_url: string | null }} SelectedManga
 * @typedef {import('../bulk-migration.js').Match} Match
 * @typedef {import('../bulk-migration.js').RowStatus} RowStatus
 * @typedef {{ job_id: string | null, status: string, error: string | null, chapters_matched: number | null }} Progress
 */

const STATUS_BADGE = {
  searching: 'badge-muted',
  matched: 'badge-success',
  review: 'bg-warn/15 text-warn',
  skipped: 'badge-muted',
  none: 'badge-muted',
  unsure: 'bg-warn/15 text-warn',
  in_library: 'bg-warn/15 text-warn',
  duplicate: 'bg-warn/15 text-warn',
  error: 'badge-danger',
};

const TERMINAL = new Set(['completed', 'failed', 'cancelled', 'refused']);

/** @param {{ url: string | null, title: string }} props */
function Thumb({ url, title }) {
  return html`
    <div class="w-10 shrink-0 rounded-sm overflow-hidden bg-surface-2" style="aspect-ratio:2/3">
      ${url && html`<img src=${url} alt=${title} loading="lazy" class="w-full h-full object-cover" />`}
    </div>
  `;
}

/**
 * @param {{
 *   manga: SelectedManga[],
 *   onClose: () => void,
 *   onMigrated: () => void,
 * }} props
 */
function BulkMigrationDialogue({ manga, onClose, onMigrated }) {
  const [step, setStep] = useState(/** @type {'source'|'review'|'migrating'} */ ('source'));
  const [sources, setSources] = useState(/** @type {{ id: number, name: string }[]} */ ([]));
  const [sourceId, setSourceId] = useState('');
  const [matches, setMatches] = useState(/** @type {Map<number, Match>} */ (new Map()));
  const [choices, setChoices] = useState(/** @type {Map<number, string>} */ (new Map()));
  const [queries, setQueries] = useState(/** @type {Map<number, string>} */ (new Map()));
  const [searchOpen, setSearchOpen] = useState(/** @type {Set<number>} */ (new Set()));
  const [keepDownloads, setKeepDownloads] = useState(true);
  const [progress, setProgress] = useState(/** @type {Map<number, Progress>} */ (new Map()));
  const [error, setError] = useState(/** @type {string | null} */ (null));
  const abortRef = useRef(/** @type {AbortController | null} */ (null));

  useEffect(() => {
    api.getSources()
      .then((list) => setSources((Array.isArray(list) ? list : []).filter(s => s.enabled).map(s => ({ id: s.id, name: s.name }))))
      .catch(() => setError(t('migration.bulk.error.sources')));
    return () => abortRef.current?.abort();
  }, []);

  const order = manga.map(m => m.id);
  const titleOf = new Map(manga.map(m => [m.id, m]));
  const plan = planBatch(order, matches, choices);
  const matching = step === 'review' && matches.size < manga.length;

  async function _startMatching() {
    abortRef.current?.abort();
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    setMatches(new Map());
    setChoices(new Map());
    setError(null);
    setStep('review');
    for (const ids of chunk(order, MATCH_CHUNK)) {
      try {
        const found = await api.matchMigrationTargets(Number(sourceId), ids.map(id => ({ manga_id: id })), ctrl.signal);
        if (ctrl.signal.aborted) return;
        setMatches(prev => {
          const next = new Map(prev);
          for (const m of Array.isArray(found) ? found : []) next.set(m.manga_id, m);
          return next;
        });
      } catch (/** @type {any} */ e) {
        if (e?.name === 'AbortError') return;
        setMatches(prev => {
          const next = new Map(prev);
          for (const id of ids) next.set(id, { manga_id: id, title: titleOf.get(id)?.title ?? '', candidates: [], best: null, error: t('migration.bulk.error.search') });
          return next;
        });
      }
    }
  }

  /** @param {number} mangaId */
  async function _searchAgain(mangaId) {
    const query = (queries.get(mangaId) ?? '').trim();
    if (!query) return;
    setMatches(prev => { const next = new Map(prev); next.delete(mangaId); return next; });
    setChoices(prev => { const next = new Map(prev); next.delete(mangaId); return next; });
    try {
      const [found] = await api.matchMigrationTargets(Number(sourceId), [{ manga_id: mangaId, query }]);
      setMatches(prev => new Map(prev).set(mangaId, found));
    } catch {
      setMatches(prev => new Map(prev).set(mangaId, { manga_id: mangaId, title: titleOf.get(mangaId)?.title ?? '', candidates: [], best: null, error: t('migration.bulk.error.search') }));
    }
  }

  async function _migrate() {
    setError(null);
    try {
      const outcomes = await api.submitBulkMigration(Number(sourceId), plan.items, keepDownloads);
      /** @type {Map<number, Progress>} */
      const next = new Map();
      for (const o of Array.isArray(outcomes) ? outcomes : []) {
        next.set(o.manga_id, o.job_id
          ? { job_id: o.job_id, status: 'pending', error: null, chapters_matched: null }
          : { job_id: null, status: 'refused', error: o.error ?? t('migration.error.migrate_failed'), chapters_matched: null });
      }
      setProgress(next);
      setStep('migrating');
    } catch {
      setError(t('migration.error.migrate_failed'));
    }
  }

  const pendingJobs = [...progress.values()].filter(p => p.job_id && !TERMINAL.has(p.status)).map(p => /** @type {string} */ (p.job_id));
  const allDone = step === 'migrating' && pendingJobs.length === 0;

  useEffect(() => {
    if (step !== 'migrating' || pendingJobs.length === 0) return;
    let stopped = false;
    /** @type {ReturnType<typeof setTimeout> | null} */
    let timer = null;

    async function refresh() {
      for (const ids of chunk(pendingJobs, STATUS_CHUNK)) {
        try {
          const statuses = await api.getMigrationStatuses(ids);
          if (stopped) return;
          const byJob = new Map((Array.isArray(statuses) ? statuses : []).map(s => [s.id, s]));
          setProgress(prev => {
            const next = new Map(prev);
            for (const [mangaId, p] of prev) {
              const s = p.job_id ? byJob.get(p.job_id) : null;
              if (!s) continue;
              next.set(mangaId, {
                ...p,
                status: s.status,
                error: s.status === 'failed' || s.status === 'cancelled' ? (jobErrorMessage(s.error) ?? t('migration.error.migrate_failed')) : null,
                chapters_matched: s.result?.chapters_matched ?? null,
              });
            }
            return next;
          });
        } catch { }
      }
    }

    const schedule = () => { timer = setTimeout(async () => { await refresh(); if (!stopped) schedule(); }, POLL_MS); };
    /** @param {Event} e */
    const onEvent = (e) => {
      const data = /** @type {any} */ (e).detail;
      if (data?.job_type === 'migration' && pendingJobs.includes(data.job_id) && /^job_(completed|failed|cancelled)$/.test(data.type)) refresh();
    };
    window.addEventListener('kani:sse', onEvent);
    schedule();
    return () => { stopped = true; if (timer) clearTimeout(timer); window.removeEventListener('kani:sse', onEvent); };
  }, [step, pendingJobs.join(',')]);

  useEffect(() => { if (allDone) onMigrated(); }, [allDone]);

  const sourceName = sources.find(s => String(s.id) === sourceId)?.name ?? '';
  const counts = { migrating: plan.items.length, review: 0, skipped: 0 };
  for (const status of plan.statuses.values()) {
    if (status === 'review' || status === 'unsure') counts.review++;
    if (status !== 'matched' && status !== 'review' && status !== 'unsure' && status !== 'searching') counts.skipped++;
  }
  const done = [...progress.values()];
  const summary = {
    migrated: done.filter(p => p.status === 'completed').length,
    failed: done.filter(p => p.status === 'failed' || p.status === 'cancelled' || p.status === 'refused').length,
    running: done.filter(p => !TERMINAL.has(p.status)).length,
  };

  /** @param {number} mangaId */
  function _reviewRow(mangaId) {
    const m = titleOf.get(mangaId);
    const match = matches.get(mangaId);
    const status = plan.statuses.get(mangaId) ?? 'searching';
    const choice = choices.get(mangaId) ?? match?.best ?? '';
    const options = [
      { value: '', label: t('migration.bulk.skip') },
      ...(match?.candidates ?? []).map(c => ({
        value: c.id,
        label: t('migration.bulk.candidate', { title: c.title, score: Math.round(c.score * 100) }),
      })),
    ];
    return html`
      <li key=${mangaId} class="flex flex-col sm:flex-row sm:items-center gap-3 py-3 border-b border-border-subtle last:border-b-0">
        <div class="flex items-center gap-3 min-w-0 sm:w-2/5">
          <${Thumb} url=${m?.cover_url ?? null} title=${m?.title ?? ''} />
          <span class="text-sm text-text line-clamp-2">${m?.title}</span>
        </div>
        <div class="flex flex-col gap-1.5 min-w-0 flex-1">
          ${status === 'searching'
            ? html`<div class="skeleton h-9 w-full rounded-md"></div>`
            : match?.error
              ? html`<p class="text-sm text-danger">${match.error}</p>`
              : html`<${Select}
                  options=${options}
                  value=${choice}
                  ariaLabel=${t('migration.bulk.target_for', { title: m?.title ?? '' })}
                  onChange=${(v) => setChoices(prev => new Map(prev).set(mangaId, v))}
                />`}
          ${status === 'matched' && !searchOpen.has(mangaId) && html`
            <button type="button" class="self-start text-xs text-text-muted hover:text-text underline-offset-2 hover:underline" onClick=${() => setSearchOpen(prev => new Set(prev).add(mangaId))}>
              ${t('migration.bulk.search_again')}
            </button>
          `}
          ${status !== 'searching' && (status !== 'matched' || searchOpen.has(mangaId)) && html`
            <form class="flex gap-2" onSubmit=${(e) => { e.preventDefault(); _searchAgain(mangaId); }}>
              <input
                type="search"
                class="input flex-1 min-w-0"
                placeholder=${t('migration.bulk.search_again')}
                aria-label=${t('migration.bulk.search_again_for', { title: m?.title ?? '' })}
                value=${queries.get(mangaId) ?? ''}
                onInput=${(e) => { const v = /** @type {HTMLInputElement} */ (e.target).value; setQueries(prev => new Map(prev).set(mangaId, v)); }}
              />
              <button type="submit" class="btn-ghost btn-sm">${t('migration.bulk.search')}</button>
            </form>
          `}
        </div>
        <span class=${'badge self-start sm:self-center whitespace-nowrap ' + STATUS_BADGE[status]}>${t(`migration.bulk.status.${status}`)}</span>
      </li>
    `;
  }

  /** @param {number} mangaId */
  function _progressRow(mangaId) {
    const m = titleOf.get(mangaId);
    const p = progress.get(mangaId);
    if (!p) return null;
    const badge = p.status === 'completed' ? 'badge-success'
      : (p.status === 'failed' || p.status === 'cancelled' || p.status === 'refused') ? 'badge-danger'
      : 'badge-muted';
    return html`
      <li key=${mangaId} class="flex items-center gap-3 py-3 border-b border-border-subtle last:border-b-0">
        <${Thumb} url=${m?.cover_url ?? null} title=${m?.title ?? ''} />
        <div class="flex flex-col min-w-0 flex-1">
          <span class="text-sm text-text truncate">${m?.title}</span>
          ${p.error && html`<span class="text-xs text-danger">${p.error}</span>`}
          ${p.status === 'completed' && p.chapters_matched != null && html`<span class="text-xs text-text-muted">${t('migration.bulk.chapters_matched', { count: p.chapters_matched })}</span>`}
        </div>
        <span class=${'badge whitespace-nowrap ' + badge}>${t(`migration.bulk.job.${p.status}`)}</span>
      </li>
    `;
  }

  const footer = step === 'source'
    ? html`
        <div class="flex gap-3 justify-end">
          <button type="button" class="btn-ghost" onClick=${onClose}>${t('common.cancel')}</button>
          <button type="button" class="btn-primary" disabled=${!sourceId} onClick=${_startMatching}>${t('migration.bulk.find_matches')}</button>
        </div>`
    : step === 'review'
      ? html`
          <div class="flex flex-wrap gap-3 justify-end">
            <button type="button" class="btn-ghost" onClick=${() => { abortRef.current?.abort(); setStep('source'); }}>${t('migration.action.back')}</button>
            <button type="button" class="btn-primary" disabled=${matching || plan.items.length === 0} onClick=${_migrate}>
              ${t('migration.bulk.migrate', { count: plan.items.length })}
            </button>
          </div>`
      : html`
          <div class="flex justify-end">
            <button type="button" class="btn-primary" onClick=${onClose}>${allDone ? t('migration.bulk.done') : t('migration.bulk.close_background')}</button>
          </div>`;

  return html`
    <${Modal} open=${true} onClose=${onClose} title=${t('migration.bulk.title', { count: manga.length })} wide=${true} footer=${footer}>
      ${step === 'source' && html`
        <div class="flex flex-col gap-4">
          <p class="text-sm text-text-muted">${t('migration.bulk.intro')}</p>
          <div class="flex flex-col gap-1.5">
            <span class="text-sm font-medium text-text" aria-hidden="true">${t('migration.bulk.target_source')}</span>
            <${Select}
              options=${[{ value: '', label: t('migration.bulk.choose_source') }, ...sources.map(s => ({ value: String(s.id), label: s.name }))]}
              value=${sourceId}
              ariaLabel=${t('migration.bulk.target_source')}
              onChange=${setSourceId}
            />
          </div>
          ${error && html`<${Callout} tone="danger">${error}</${Callout}>`}
        </div>
      `}

      ${step === 'review' && html`
        <div class="flex flex-col gap-3">
          <p class="text-sm text-text-muted" aria-live="polite">
            ${matching
              ? t('migration.bulk.searching', { done: matches.size, total: manga.length, source: sourceName })
              : t('migration.bulk.review_summary', { migrating: counts.migrating, review: counts.review, skipped: counts.skipped })}
          </p>
          ${matching && html`
            <div class="h-1 rounded-full bg-surface-2 overflow-hidden" role="progressbar" aria-valuemin="0" aria-valuemax=${manga.length} aria-valuenow=${matches.size}>
              <div class="h-full bg-accent transition-all" style=${`width:${Math.round((matches.size / Math.max(manga.length, 1)) * 100)}%`}></div>
            </div>
          `}
          <ul class="flex flex-col">${order.map(_reviewRow)}</ul>
          <label class="flex items-start gap-2 text-sm text-text pt-1">
            <span class="kani-toggle mt-0.5">
              <input type="checkbox" class="kani-toggle__input" checked=${keepDownloads}
                onChange=${(e) => setKeepDownloads(/** @type {HTMLInputElement} */ (e.target).checked)} />
              <span class="kani-toggle__track"></span>
            </span>
            <span class="flex flex-col">
              ${t('migration.preview.keep_downloaded')}
              <span class="text-xs text-text-muted">${t('migration.bulk.keep_downloaded.desc')}</span>
            </span>
          </label>
          ${error && html`<${Callout} tone="danger">${error}</${Callout}>`}
        </div>
      `}

      ${step === 'migrating' && html`
        <div class="flex flex-col gap-3">
          <p class="text-sm text-text-muted" aria-live="polite">
            ${allDone
              ? t('migration.bulk.finished', { migrated: summary.migrated, failed: summary.failed })
              : t('migration.bulk.progress', { migrated: summary.migrated, running: summary.running, failed: summary.failed })}
          </p>
          <ul class="flex flex-col">${order.map(_progressRow)}</ul>
        </div>
      `}
    <//>
  `;
}

/**
 * @param {{ manga: SelectedManga[], onMigrated: () => void }} props
 * @returns {() => void}
 */
export function openBulkMigrationDialogue({ manga, onMigrated }) {
  let close = () => {};
  close = mountIntoModalRoot(html`<${BulkMigrationDialogue} manga=${manga} onClose=${() => close()} onMigrated=${onMigrated} />`);
  return close;
}
