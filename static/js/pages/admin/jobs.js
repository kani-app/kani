// @ts-check

import { h, render } from 'preact';
import { useState, useEffect, useCallback } from 'preact/hooks';
import htm from 'htm';
import { hasPermission } from '../../session.js';
import { getJobs, cancelJob, pauseJob, resumeJob } from '../../api.js';
import { showApiError, showToast } from '../../components/toast.js';
import { setPageHeader, clearPageHeader } from '../../components/app-header.js';
import { EmptyState } from '../../components/empty-state.js';
import { Tabs } from '../../components/tabs.js';
import { Pagination } from '../../components/pagination.js';
import { Select } from '../../components/form/select.js';
import { Icon } from '../../components/icon.js';
import { iconCheck, iconWarning, iconDocument } from '../../icons.js';
import { t } from '../../i18n.js';
const html = htm.bind(h);


/** @param {number} ts */
function _fmtTime(ts) {
  if (!ts) return '';
  return new Date(ts * 1000).toLocaleString();
}

/** @param {string} jobType */
function _jobLabel(jobType) {
  const key = `jobs.type.${jobType}`;
  const val = t(key);
  return val === key ? jobType.replace(/_/g, ' ') : val;
}

/** @param {any} job */
function _pct(job) {
  const p = job.progress;
  if (!p || !p.total) return 0;
  return Math.round((p.current / p.total) * 100);
}



function ActiveJobRow({ job, onCancel, onPause, onResume }) {
  const pct = _pct(job);
  const spinning = pct === 0 || job.status === 'pending';
  const circ = 75.4;

  return html`
    <div class="flex flex-col gap-2 px-4 py-3 border-b border-border-subtle last:border-b-0">
      <div class="flex items-center gap-3">
        <div class="shrink-0 w-7 h-7 flex items-center justify-center rounded-full text-accent">
          <svg class=${'w-4 h-4 ' + (spinning ? 'dl-ring-spin' : '-rotate-90')} viewBox="0 0 32 32" aria-hidden="true">
            <circle cx="16" cy="16" r="12" fill="none" stroke="currentColor" stroke-width="2.5" opacity="0.25" />
            <circle cx="16" cy="16" r="12" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"
              stroke-dasharray=${spinning ? '56.5 18.9' : String(circ)}
              stroke-dashoffset=${spinning ? undefined : String(circ - circ * pct / 100)}
              style=${{ transition: 'stroke-dashoffset 0.3s ease' }}
            />
          </svg>
        </div>
        <div class="flex-1 min-w-0">
          <p class="text-xs text-text-muted truncate">${_jobLabel(job.job_type)}</p>
          <p class="text-sm text-text truncate" title=${job.description}>${job.description}</p>
          ${job.progress?.message && html`<p class="text-xs text-text-muted mt-0.5 truncate">${job.progress.message}</p>`}
          ${job.progress?.total > 0 && html`<p class="text-xs text-text-muted mt-0.5">${job.progress.current} / ${job.progress.total}</p>`}
        </div>
        <span class=${'text-xs px-1.5 py-0.5 rounded-full font-medium shrink-0 ' + (job.status === 'running' ? 'bg-accent/15 text-accent' : 'bg-surface-2 text-text-muted')}>
          ${job.status}
        </span>
        ${job.status === 'pending' && html`
          <button class="btn-ghost btn-sm shrink-0" onClick=${() => onPause(job.id)} aria-label=${t('jobs.pause')}>
            ${t('jobs.pause')}
          </button>
        `}
        ${job.status === 'paused' && html`
          <button class="btn-ghost btn-sm shrink-0" onClick=${() => onResume(job.id)} aria-label=${t('jobs.resume')}>
            ${t('jobs.resume')}
          </button>
        `}
        <button class="btn-ghost btn-sm shrink-0 text-danger" onClick=${() => onCancel(job.id)} aria-label=${t('jobs.cancel')}>
          ${t('jobs.cancel')}
        </button>
      </div>
      <div class="h-1 rounded-full bg-surface-2 overflow-hidden ml-10">
        <div class="h-full rounded-full bg-accent transition-[width] duration-300" style=${{ width: (spinning ? 30 : pct) + '%' }}></div>
      </div>
    </div>
  `;
}

function CompletedJobRow({ job }) {
  return html`
    <div class="flex items-center gap-3 px-4 py-3 border-b border-border-subtle last:border-b-0">
      <div class="shrink-0 w-7 h-7 flex items-center justify-center icon-sm text-success">
        <${Icon} svg=${iconCheck} />
      </div>
      <div class="flex-1 min-w-0">
        <p class="text-xs text-text-muted truncate">${_jobLabel(job.job_type)}</p>
        <p class="text-sm text-text truncate" title=${job.description}>${job.description}</p>
        ${job.completed_at && html`<p class="text-xs text-text-muted mt-0.5">${_fmtTime(job.completed_at)}</p>`}
      </div>
    </div>
  `;
}

function FailedJobRow({ job }) {
  const [expanded, setExpanded] = useState(false);
  const errMsg = job.error?.message ?? (typeof job.error === 'string' ? job.error : null);

  return html`
    <div class="flex flex-col border-b border-border-subtle last:border-b-0">
      <div class="flex items-center gap-3 px-4 py-3">
        <div class="shrink-0 w-7 h-7 flex items-center justify-center icon-sm text-danger">
          <${Icon} svg=${iconWarning} />
        </div>
        <div class="flex-1 min-w-0">
          <p class="text-xs text-text-muted truncate">${_jobLabel(job.job_type)}</p>
          <p class="text-sm text-text truncate" title=${job.description}>${job.description}</p>
          ${errMsg && html`<p class="text-xs text-danger mt-0.5 truncate">${errMsg}</p>`}
          ${job.completed_at && html`<p class="text-xs text-text-muted mt-0.5">${_fmtTime(job.completed_at)}</p>`}
        </div>
        ${errMsg && html`
          <button class="btn-ghost btn-xs shrink-0 text-text-muted" onClick=${() => setExpanded(v => !v)}>
            ${expanded ? t('jobs.error.hide') : t('jobs.error.details')}
          </button>
        `}
      </div>
      ${expanded && errMsg && html`
        <div class="px-4 pb-3 ml-10">
          <pre class="text-xs text-danger bg-danger/5 border border-danger/20 rounded-md p-3 overflow-auto max-h-32 whitespace-pre-wrap">${errMsg}</pre>
        </div>
      `}
    </div>
  `;
}


const PAGE_SIZE = 25;

/** Status groups per tab — the server accepts a comma-separated status list. */
const TAB_STATUSES = {
  // `paused` belongs here: a paused job is still outstanding work, and leaving
  // it out made pausing look like the job vanished — with the Resume button
  // unreachable, since it only renders on the row.
  active:    ['pending', 'running', 'paused'],
  completed: ['completed'],
  failed:    ['failed', 'cancelled'],
};

function JobsPage() {
  const [tab, setTab] = useState(/** @type {'active'|'completed'|'failed'} */ ('active'));
  const [jobType, setJobType] = useState('');
  const [page, setPage] = useState(1);
  const [jobs, setJobs] = useState(/** @type {any[]} */ ([]));
  const [total, setTotal] = useState(0);
  const [jobTypes, setJobTypes] = useState(/** @type {string[]} */ ([]));
  const [loading, setLoading] = useState(true);

  const _load = useCallback(() => {
    setLoading(true);
    getJobs({
      status: TAB_STATUSES[tab].join(','),
      job_type: jobType || undefined,
      limit: PAGE_SIZE,
      offset: (page - 1) * PAGE_SIZE,
    }).then(res => {
      setJobs(Array.isArray(res?.jobs) ? res.jobs : []);
      setTotal(res?.total ?? 0);
      setJobTypes(Array.isArray(res?.job_types) ? res.job_types : []);
    }).catch(showApiError).finally(() => setLoading(false));
  }, [tab, jobType, page]);

  useEffect(() => { _load(); }, [_load]);

  // A job changing state can move it in or out of the current tab, so refetch
  // rather than patching a page whose offsets the server owns.
  useEffect(() => {
    const onSSE = (/** @type {CustomEvent} */ e) => {
      const type = /** @type {any} */ (e).detail?.type;
      if (typeof type === 'string' && type.startsWith('job_')) _load();
    };
    window.addEventListener('kani:sse', /** @type {any} */ (onSSE));
    return () => window.removeEventListener('kani:sse', /** @type {any} */ (onSSE));
  }, [_load]);

  async function handlePause(id) {
    try {
      await pauseJob(id);
      showToast(t('jobs.action.paused'));
      _load();
    } catch (err) {
      showApiError(err);
    }
  }

  async function handleResume(id) {
    try {
      await resumeJob(id);
      showToast(t('jobs.action.resumed'));
      _load();
    } catch (err) {
      showApiError(err);
    }
  }

  async function handleCancel(id) {
    try {
      await cancelJob(id);
      showToast(t('jobs.action.cancelled'));
      _load();
    } catch (err) {
      showApiError(err);
    }
  }

  const _setTab = (/** @type {any} */ id) => { setTab(id); setPage(1); };

  const typeOptions = [
    { value: '', label: t('jobs.filter.all_types') },
    ...jobTypes.map(jt => ({ value: jt, label: _jobLabel(jt) })),
  ];

  const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE));
  const emptyFor = {
    active:    { icon: iconDocument, title: t('jobs.empty.active'),    subtitle: t('jobs.empty.active.desc') },
    completed: { icon: iconCheck,    title: t('jobs.empty.completed'), subtitle: t('jobs.empty.completed.desc') },
    failed:    { icon: iconWarning,  title: t('jobs.empty.failed'),    subtitle: undefined },
  }[tab];

  return html`
    <div class="max-w-page mx-auto w-full px-4 md:px-6 py-6 flex flex-col gap-4 page-body-host page-col">
      <${Tabs}
        tabs=${[
          { id: 'active',    name: t('jobs.tab.active') },
          { id: 'completed', name: t('jobs.tab.completed') },
          { id: 'failed',    name: t('jobs.tab.failed') },
        ]}
        activeId=${tab}
        onSelect=${_setTab}
      />

      <div class="flex items-center gap-3 flex-wrap">
        <${Select}
          options=${typeOptions}
          value=${jobType}
          ariaLabel=${t('jobs.filter.type')}
          onChange=${(/** @type {string} */ v) => { setJobType(v); setPage(1); }}
        />
        ${total > 0 && html`
          <span class="text-xs text-text-muted">${t('jobs.count', { count: total, s: total !== 1 ? 's' : '' })}</span>
        `}
      </div>

      <div class=${'bg-surface border border-border rounded-xl overflow-x-hidden page-body--fit' + (loading ? ' opacity-60' : '')}>
        ${jobs.length === 0 && !loading
          ? html`<${EmptyState} icon=${emptyFor.icon} title=${emptyFor.title} subtitle=${emptyFor.subtitle} />`
          : jobs.map(j => {
              if (tab === 'active')    return html`<${ActiveJobRow}    key=${j.id} job=${j} onCancel=${handleCancel} onPause=${handlePause} onResume=${handleResume} />`;
              if (tab === 'completed') return html`<${CompletedJobRow} key=${j.id} job=${j} />`;
              return html`<${FailedJobRow} key=${j.id} job=${j} />`;
            })
        }
      </div>

      ${totalPages > 1 && html`
        <${Pagination} page=${page} hasNext=${page < totalPages} total=${totalPages} onPageChange=${setPage} />
      `}
    </div>
  `;
}


/** @param {HTMLElement} container */
export async function init(container) {
  document.title = t('jobs.title') + ' - Kani';

  if (!hasPermission('admin:jobs')) {
    container.innerHTML = `
      <div class="flex flex-col items-center justify-center gap-3 py-20 text-text-muted">
        <p class="text-base font-medium text-text">${t('jobs.denied.title')}</p>
        <p class="text-sm">${t('jobs.denied.desc')}</p>
      </div>
    `;
    return;
  }

  setPageHeader({ crumbs: [{ label: t('jobs.title') }] });
  container.classList.add('page-fixed');
  render(html`<${JobsPage} />`, container);
}

/** @param {HTMLElement} container */
export function destroy(container) {
  clearPageHeader();
  render(null, container);
}
