// @ts-check

import { h } from 'preact';
import htm from 'htm';
import * as api from '../api.js';
import { t } from '../i18n.js';
import { Modal, showConfirm } from './modal.js';
import { showApiError, showToast } from './toast.js';
import { useBusy } from '../hooks/use-busy.js';

const html = htm.bind(h);

/** @param {any} c */
function isReassurance(c) {
  return c.kind === 'source_downgraded';
}

/** @param {number|null|undefined} px */
function fmtResolution(px) {
  return px ? `${px} px` : null;
}

/** @param {string|null|undefined} profile */
function fmtColour(profile) {
  if (!profile || profile === 'unknown') return null;
  return t(`upgrade.colour.${profile}`);
}

/** @param {number|null|undefined} q */
function fmtEncoder(q) {
  return q == null ? null : String(q);
}

/** @param {number|null|undefined} bpm */
function fmtBitrate(bpm) {
  if (!bpm) return null;
  return bpm >= 1_000_000
    ? `${(bpm / 1_000_000).toFixed(1)} MB/MP`
    : `${Math.round(bpm / 1000)} kB/MP`;
}

/**
 * Higher wins, but only when both sides are known — a missing measurement must
 * never render as the other side losing.
 * @param {number|null|undefined} held
 * @param {number|null|undefined} cand
 */
function higherWins(held, cand) {
  if (held == null || cand == null || held === cand) return null;
  return cand > held ? 'candidate' : 'held';
}

const COLOUR_RANK = { monochrome: 0, colour_accent: 1, full_colour: 2 };

/**
 * @param {string|null|undefined} held
 * @param {string|null|undefined} cand
 */
function colourWins(held, cand) {
  const h = COLOUR_RANK[held];
  const c = COLOUR_RANK[cand];
  if (h == null || c == null || h === c) return null;
  return c > h ? 'candidate' : 'held';
}

/**
 * @param {{ label: string, held: any, candidate: any, better?: 'held'|'candidate'|null }} props
 */
function CompareRow({ label, held, candidate, better = null }) {
  const cell = (/** @type {any} */ v, /** @type {boolean} */ win) => html`
    <span class="text-sm ${win ? 'text-text font-medium' : 'text-text-muted'}">
      ${v ?? '—'}
    </span>
  `;
  return html`
    <div class="grid grid-cols-3 gap-2 items-baseline px-1 py-1.5">
      <span class="text-xs text-text-muted">${label}</span>
      ${cell(held, better === 'held')}
      ${cell(candidate, better === 'candidate')}
    </div>
  `;
}

/**
 * @param {{ open: boolean, candidate: any, chapterTitle: string,
 *           onClose: () => void, onChanged: () => void }} props
 */
export function UpgradeCompare({ open, candidate, chapterTitle, onClose, onChanged }) {
  const { busy, run } = useBusy();
  if (!candidate) return null;

  const reassurance = isReassurance(candidate);

  const apply = () =>
    run(async () => {
      const ok = await showConfirm(t('upgrade.replace.confirm'), {
        title: t('upgrade.replace'),
        confirmLabel: t('upgrade.replace'),
      });
      if (!ok) return;
      try {
        await api.applyChapterUpgrade(candidate.held_chapter_id);
        showToast(t('upgrade.replace.started'), { type: 'success' });
        onChanged();
        onClose();
      } catch (e) {
        showApiError(e);
      }
    });

  const dismiss = () =>
    run(async () => {
      try {
        await api.dismissChapterUpgrade(candidate.held_chapter_id);
        onChanged();
        onClose();
      } catch (e) {
        showApiError(e);
      }
    });

  const held = candidate.held_score ?? {};
  const cand = candidate.candidate_score ?? {};
  const candidateMeasured = candidate.candidate_score != null;

  // Only rows where at least one side measured something. A comparison of two
  // dashes tells the reader nothing and makes the panel look broken.
  const measuredRows = [
    {
      label: t('upgrade.resolution'),
      held: fmtResolution(held.median_long_edge_px),
      candidate: fmtResolution(cand.median_long_edge_px),
      better: higherWins(held.median_long_edge_px, cand.median_long_edge_px),
    },
    {
      label: t('upgrade.colour'),
      held: fmtColour(held.colour),
      candidate: fmtColour(cand.colour),
      better: colourWins(held.colour, cand.colour),
    },
    {
      label: t('upgrade.encoder'),
      held: fmtEncoder(held.median_encoder_quality),
      candidate: fmtEncoder(cand.median_encoder_quality),
      better: higherWins(held.median_encoder_quality, cand.median_encoder_quality),
    },
    {
      label: t('upgrade.bitrate'),
      held: fmtBitrate(held.bytes_per_megapixel),
      candidate: fmtBitrate(cand.bytes_per_megapixel),
      better: higherWins(held.bytes_per_megapixel, cand.bytes_per_megapixel),
    },
  ].filter((row) => row.held != null || row.candidate != null);

  const heldPages = candidate.held_page_count;
  const candPages = candidate.candidate_page_count;
  const pageWinner =
    heldPages == null || candPages == null
      ? null
      : candPages > heldPages
        ? 'candidate'
        : candPages < heldPages
          ? 'held'
          : null;

  return html`
    <${Modal}
      open=${open}
      title=${t('upgrade.title')}
      onClose=${onClose}
      footer=${reassurance
        ? html`<button type="button" class="btn-secondary btn-sm" onClick=${onClose}>
            ${t('common.done')}
          </button>`
        : html`
            <button type="button" class="btn-ghost btn-sm" disabled=${busy} onClick=${dismiss}>
              ${t('upgrade.dismiss')}
            </button>
            <button type="button" class="btn-primary btn-sm" disabled=${busy} onClick=${apply}>
              ${t('upgrade.replace')}
            </button>
          `}
    >
      <div class="flex flex-col gap-3 px-1">
        <p class="text-sm ${reassurance ? 'text-success' : 'text-text-muted'}">
          ${t(candidate.reason_key)}
        </p>
        <p class="text-xs text-text-muted">${chapterTitle}</p>

        <div class="border-t border-border-subtle pt-2">
          <div class="grid grid-cols-3 gap-2 px-1 pb-1">
            <span></span>
            <span class="text-xs font-medium text-text">${t('upgrade.yours')}</span>
            <span class="text-xs font-medium text-text">${t('upgrade.theirs')}</span>
          </div>
          <${CompareRow}
            label=${t('upgrade.pages')}
            held=${heldPages}
            candidate=${candPages}
            better=${pageWinner}
          />
          ${measuredRows.map(
            (row) => html`
              <${CompareRow}
                key=${row.label}
                label=${row.label}
                held=${row.held}
                candidate=${row.candidate}
                better=${row.better}
              />
            `,
          )}
          <${CompareRow}
            label=${t('upgrade.scanlator')}
            held=${candidate.held_scanlator ?? '—'}
            candidate=${candidate.candidate_scanlator ?? '—'}
          />
        </div>

        ${candidateMeasured
          ? html`<p class="text-xs text-text-muted">${t('upgrade.measured_note')}</p>`
          : html`<p class="text-xs text-text-muted">${t('upgrade.unprobed')}</p>`}
        ${reassurance
          ? html`<p class="text-xs text-text-muted">${t('upgrade.downgrade.note')}</p>`
          : html`<p class="text-xs text-text-muted">${t('upgrade.replace.note')}</p>`}
      </div>
    <//>
  `;
}
