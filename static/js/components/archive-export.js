// @ts-check
// Archive export dialogue: produces a portable copy of the library that stays
// readable without Kani.

import { h } from 'preact';
import { useState, useRef } from 'preact/hooks';
import htm from 'htm';
import * as api from '../api.js';
import { t } from '../i18n.js';
import { Modal } from './modal.js';
import { showApiError } from './toast.js';
import { useSSE } from '../hooks/use-sse.js';
import { useBusy } from '../hooks/use-busy.js';

const html = htm.bind(h);

/** @param {{ open: boolean, onClose: () => void }} props */
export function ArchiveExportModal({ open, onClose }) {
  const [zip, setZip] = useState(true);
  const [includeViewer, setIncludeViewer] = useState(true);
  const [jobId, setJobId] = useState(/** @type {string | null} */ (null));
  const [progress, setProgress] = useState(/** @type {{done: number, total: number} | null} */ (null));
  const [done, setDone] = useState(/** @type {any} */ (null));
  // useSSE subscribes once and never re-binds its callback, so a handler that
  // closed over `jobId` would forever see the value from the first render.
  const jobIdRef = useRef(/** @type {string | null} */ (null));
  const { busy, run } = useBusy();

  useSSE('job_progress', (/** @type {any} */ ev) => {
    if (ev?.job_id !== jobIdRef.current) return;
    setProgress({ done: ev.current ?? 0, total: ev.total ?? 0 });
  });
  // JobCompleted carries only job_id/job_type/description — the report lives on
  // the job row, so fetch it rather than reading a field the event never has.
  const finish = (/** @type {string} */ id) => {
    setProgress(null);
    api
      .getJob(id)
      .then((status) => setDone(status?.result ?? {}))
      .catch(() => setDone({}));
  };

  useSSE('job_completed', (/** @type {any} */ ev) => {
    const id = jobIdRef.current;
    if (!id || ev?.job_id !== id) return;
    finish(id);
  });

  const reset = () => {
    jobIdRef.current = null;
    setJobId(null);
    setProgress(null);
    setDone(null);
  };

  const close = () => {
    reset();
    onClose();
  };

  const start = () =>
    run(async () => {
      try {
        reset();
        const res = await api.exportArchive({
          manga_ids: null,
          zip,
          include_viewer: includeViewer,
        });
        jobIdRef.current = res.job_id;
        setJobId(res.job_id);
        // A small library exports faster than this request round-trips, so the
        // completion event can be broadcast before the id is even known — and
        // it never repeats. Check once, then let SSE handle the slow case.
        const status = await api.getJob(res.job_id).catch(() => null);
        if (status && (status.status === 'completed' || status.result)) {
          finish(res.job_id);
        }
      } catch (e) {
        showApiError(e);
      }
    });

  const pct = progress && progress.total > 0
    ? Math.round((progress.done / progress.total) * 100)
    : null;

  return html`
    <${Modal}
      open=${open}
      title=${t('storage.archive.title')}
      onClose=${close}
      footer=${done
        ? html`<button type="button" class="btn-secondary btn-sm" onClick=${close}>
            ${t('common.done')}
          </button>`
        : html`
            <button type="button" class="btn-ghost btn-sm" onClick=${close}>
              ${t('common.cancel')}
            </button>
            <button
              type="button"
              class="btn-primary btn-sm"
              disabled=${busy || !!jobId}
              onClick=${start}
            >
              ${t('storage.archive.start')}
            </button>
          `}
    >
      <div class="flex flex-col gap-4 px-1">
        <p class="text-sm text-text-muted">${t('storage.archive.desc')}</p>

        ${!jobId
          ? html`
              <label class="flex items-center gap-2 text-sm cursor-pointer">
                <input
                  type="checkbox"
                  class="accent-accent cursor-pointer"
                  checked=${zip}
                  onChange=${(/** @type {any} */ e) => setZip(e.target.checked)}
                />
                ${t('storage.archive.zip')}
              </label>
              <label class="flex items-center gap-2 text-sm cursor-pointer">
                <input
                  type="checkbox"
                  class="accent-accent cursor-pointer"
                  checked=${includeViewer}
                  onChange=${(/** @type {any} */ e) => setIncludeViewer(e.target.checked)}
                />
                ${t('storage.archive.viewer')}
              </label>
              <p class="text-xs text-text-muted">${t('storage.archive.viewer.desc')}</p>
            `
          : null}

        ${jobId && !done
          ? html`
              <div class="flex flex-col gap-1.5">
                <span class="text-sm text-text"
                  >${pct === null
                    ? t('storage.archive.starting')
                    : t('storage.archive.progress', {
                        done: progress?.done ?? 0,
                        total: progress?.total ?? 0,
                      })}</span
                >
                <div class="h-1.5 rounded bg-surface-alt overflow-hidden">
                  <div
                    class="h-full bg-accent transition-all"
                    style=${`width: ${pct ?? 0}%`}
                  ></div>
                </div>
              </div>
            `
          : null}

        ${done
          ? html`
              <div class="flex flex-col gap-2">
                <p class="text-sm text-success">
                  ${t('storage.archive.complete', {
                    series: done.series_count ?? 0,
                    chapters: done.chapter_count ?? 0,
                  })}
                </p>
                <code class="text-xs font-mono text-text-muted break-all">${done.root ?? ''}</code>
                ${done.zipped
                  ? html`<a
                      class="btn-secondary btn-sm self-start"
                      href=${api.archiveDownloadUrl(jobId ?? '')}
                    >
                      ${t('storage.archive.download')}
                    </a>`
                  : html`<p class="text-xs text-text-muted">
                      ${t('storage.archive.on_disk')}
                    </p>`}
                <p class="text-xs text-text-muted">${t('storage.archive.verify_hint')}</p>
              </div>
            `
          : null}
      </div>
    <//>
  `;
}
