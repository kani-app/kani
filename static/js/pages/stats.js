// @ts-check

import * as api from '../api.js';
import { navigate } from '../router.js';
import { escapeHtml } from '../utils.js';
import { setPageHeader, clearPageHeader } from '../components/app-header.js';
import { startLoading, finishLoading } from '../components/page-loading-bar.js';
import { createErrorState } from '../components/error-state.js';
import { createEmptyState } from '../components/empty-state.js';
import { t } from '../i18n.js';

const _SETUP_CMD = 'kani-cli setup';

// Chart.js renders on <canvas> and cannot read CSS custom properties.

/** @param {string} varName e.g. '--color-text-muted' */
function cssVar(varName) {
  return getComputedStyle(document.documentElement).getPropertyValue(varName).trim() || '#888'; // audit-ignore: last-resort fallback when a token is unresolved
}

/**
 * Resolves a CSS hex color token and returns it as an RGBA string.
 * @param {string} varName
 * @param {number} alpha
 */
function chartColor(varName, alpha = 1) {
  const hex = cssVar(varName).replace(/\s/g, '');
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `rgba(${r},${g},${b},${alpha})`; // audit-ignore: built from a resolved token, not a literal colour
}

// Each widget: { id: string, pinned?: boolean, render(container, data) → { destroy() } } Adding
// a new widget requires only appending to WIDGETS — no other changes. A pinned widget sits above
// the scroll boundary and stays on screen while the charts below it move.

const WIDGETS = [
  summaryWidget(),
  dailyActivityWidget(),
  readingPaceWidget(),
  topMangaWidget(),
  genreBreakdownWidget(),
];


/** @type {AbortController | null} */       let _abort = null;
/** @type {Array<{ destroy(): void }>} */   let _widgetInstances = [];
/** @type {number} */                       let _period = 90;
/** @type {HTMLElement | null} */           let _contentEl = null;
/** @type {HTMLElement | null} */           let _pinnedEl = null;


/** @param {HTMLElement} container */
export async function init(container) {
  document.title = 'Statistics - Kani';
  _period = 90;
  _widgetInstances = [];

  const picker = document.createElement('select');
  picker.className = 'input input-sm w-28';
  for (const [v, l] of [[30, t('stats.period.30d')], [90, t('stats.period.90d')], [180, t('stats.period.180d')], [365, t('stats.period.1y')]]) {
    const opt = document.createElement('option');
    opt.value = String(v);
    opt.textContent = l;
    if (v === _period) opt.selected = true;
    picker.appendChild(opt);
  }
  picker.addEventListener('change', () => {
    _period = Number(picker.value);
    _load();
  });

  setPageHeader({ crumbs: [{ label: t('stats.crumb') }], actions: picker });

  container.innerHTML = `
    <div class="max-w-page mx-auto w-full px-4 md:px-6 py-4 md:py-6 flex flex-col gap-6 page-body-host page-col">
      <div id="stats-pinned" class="flex flex-col gap-6 empty:hidden"></div>
      <div id="stats-content" class="page-body"></div>
    </div>
  `;

  container.classList.add('page-fixed');
  _contentEl = /** @type {HTMLElement} */ (container.querySelector('#stats-content'));
  _pinnedEl  = /** @type {HTMLElement} */ (container.querySelector('#stats-pinned'));
  await _load();
}

/** @param {HTMLElement} _c */
export function destroy(_c) {
  _abort?.abort();
  _abort = null;
  _destroyWidgets();
  _contentEl = null;
  _pinnedEl = null;
  clearPageHeader();
}


async function _load() {
  if (!_contentEl) return;
  _abort?.abort();
  _abort = new AbortController();
  _destroyWidgets();
  if (_pinnedEl) _pinnedEl.innerHTML = '';
  _contentEl.innerHTML = _buildSkeleton();
  startLoading();

  try {
    const stats = await api.getReadingStats(_period);
    if (_pinnedEl) _pinnedEl.innerHTML = '';
    _contentEl.innerHTML = '';
    _renderWidgets(stats);
  } catch (err) {
    if (_pinnedEl) _pinnedEl.innerHTML = '';
    _contentEl.innerHTML = '';
    _contentEl.appendChild(createErrorState({ message: err?.message ?? t('stats.error.load') }));
  } finally {
    finishLoading();
  }
}

/** @param {any} stats */
function _renderWidgets(stats) {
  if (!_contentEl) return;
  const grid = document.createElement('div');
  grid.className = 'flex flex-col gap-6';
  _contentEl.appendChild(grid);

  for (const widget of WIDGETS) {
    const slot = document.createElement('div');
    (widget.pinned && _pinnedEl ? _pinnedEl : grid).appendChild(slot);
    try {
      const instance = widget.render(slot, stats);
      _widgetInstances.push(instance);
    } catch (e) {
      console.error(`Widget ${widget.id} render error:`, e);
    }
  }
}

function _destroyWidgets() {
  for (const inst of _widgetInstances) {
    try { inst.destroy(); } catch { }
  }
  _widgetInstances = [];
}

function _buildSkeleton() {
  return `
    <div class="flex flex-col gap-6">
      <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
        ${Array(4).fill('<div class="skeleton h-24 rounded-xl"></div>').join('')}
      </div>
      <div class="skeleton h-56 rounded-xl"></div>
      <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
        <div class="skeleton h-64 rounded-xl"></div>
        <div class="skeleton h-64 rounded-xl"></div>
      </div>
    </div>
  `;
}


function summaryWidget() {
  return {
    id: 'summary',
    pinned: true,
    /** @param {HTMLElement} container @param {any} data */
    render(container, data) {
      const cards = [
        { label: t('stats.card.chapters_read'), value: data.total_chapters_read ?? 0 },
        { label: t('stats.card.manga_read'),    value: data.total_manga_read    ?? 0 },
        { label: t('stats.card.completed'),     value: data.completed_manga     ?? 0 },
        {
          label: t('stats.card.streak'),
          value: `${data.current_streak ?? 0}d`,
          note: data.longest_streak
            ? t('stats.summary.longest_streak_note', { days: data.longest_streak })
            : null,
        },
      ];

      container.innerHTML = `
        <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
          ${cards.map(c => `
            <div class="card p-4 flex flex-col gap-1">
              <span class="text-2xl font-bold text-text">${escapeHtml(String(c.value))}</span>
              <span class="text-sm text-text-muted">${escapeHtml(c.label)}</span>
              ${c.note ? `<span class="text-xs text-text-faint mt-0.5">${escapeHtml(c.note)}</span>` : ''}
            </div>
          `).join('')}
        </div>
      `;
      return { destroy() { container.innerHTML = ''; } };
    },
  };
}

/**
 * Fill missing days with zero-value rows so the category axis represents
 * time honestly — sparse activity dates plotted as-is make a month gap look
 * like a day.
 * @param {Array<Record<string, any>>} rows — rows with a YYYY-MM-DD `date`
 * @param {string} key — the value field to zero on filler rows
 */
function _zeroFillDays(rows, key) {
  if (rows.length === 0) return rows;
  const sorted = [...rows].sort((a, b) => a.date < b.date ? -1 : 1);
  const byDate = new Map(sorted.map(r => [r.date, r]));
  const out = [];
  const start = new Date(sorted[0].date + 'T00:00:00Z');
  const end = new Date();
  for (let t = start.getTime(), i = 0; t <= end.getTime() && i < 400; t += 86400000, i++) {
    const iso = new Date(t).toISOString().slice(0, 10);
    out.push(byDate.get(iso) ?? { date: iso, [key]: 0 });
  }
  return out;
}


function readingPaceWidget() {
  /** @type {any} */ let _chart = null;

  return {
    id: 'reading_pace',
    /** @param {HTMLElement} container @param {any} data */
    render(container, data) {
      const rows = _zeroFillDays(data.reading_pace ?? [], 'pages');
      if (!rows.length) {
        container.innerHTML = `<div class="card p-4 text-text-muted text-sm">${escapeHtml(t('stats.empty.no_activity'))}</div>`;
        return { destroy() { container.innerHTML = ''; } };
      }

      container.innerHTML = `
        <div class="card p-4">
          <h2 class="text-base font-semibold mb-4">${escapeHtml(t('stats.chart.pages_per_day'))}</h2>
          <div class="relative h-48">
            <canvas id="chart-pace" class="w-full h-full"></canvas>
          </div>
        </div>
      `;

      const canvas = /** @type {HTMLCanvasElement} */ (container.querySelector('#chart-pace'));

      _loadChartJs().then(Chart => {
        if (!canvas.isConnected) return;
        _chart = new Chart(canvas.getContext('2d'), {
          type: 'line',
          data: {
            labels: rows.map((r) => r.date),
            datasets: [{
              label: t('stats.chart.dataset.pages'),
              data: rows.map((r) => r.pages),
              backgroundColor: chartColor('--chart-2', 0.15),
              borderColor: chartColor('--chart-2', 0.9),
              borderWidth: 2,
              fill: true,
              tension: 0.3,
              pointRadius: 2,
            }],
          },
          options: {
            responsive: true,
            maintainAspectRatio: false,
            plugins: { legend: { display: false } },
            scales: {
              x: { ticks: { maxTicksLimit: 8, color: cssVar('--color-text-muted') }, grid: { display: false } },
              y: { beginAtZero: true, ticks: { precision: 0, color: cssVar('--color-text-muted') } },
            },
          },
        });
      }).catch(() => {
        container.innerHTML = `<div class="card p-4 text-sm text-text-muted">${t('stats.chart_unavailable_prefix')} <code>${_SETUP_CMD}</code> ${t('stats.chart_unavailable_suffix')}</div>`;
      });

      return {
        destroy() {
          _chart?.destroy();
          _chart = null;
          container.innerHTML = '';
        },
      };
    },
  };
}


function dailyActivityWidget() {
  /** @type {any} */ let _chart = null;

  return {
    id: 'daily_activity',
    /** @param {HTMLElement} container @param {any} data */
    render(container, data) {
      const rows = _zeroFillDays(data.daily_activity ?? [], 'chapters_read');
      if (!rows.length) {
        container.innerHTML = `<div class="card p-4 text-text-muted text-sm">${escapeHtml(t('stats.empty.no_activity'))}</div>`;
        return { destroy() { container.innerHTML = ''; } };
      }

      container.innerHTML = `
        <div class="card p-4">
          <h2 class="text-base font-semibold mb-4">${escapeHtml(t('stats.chart.daily_activity'))}</h2>
          <div class="relative h-48">
            <canvas id="chart-daily" class="w-full h-full"></canvas>
          </div>
        </div>
      `;

      const canvas = /** @type {HTMLCanvasElement} */ (container.querySelector('#chart-daily'));

      _loadChartJs().then(Chart => {
        if (!canvas.isConnected) return;
        _chart = new Chart(canvas.getContext('2d'), {
          type: 'bar',
          data: {
            labels: rows.map((r) => r.date),
            datasets: [{
              label: t('stats.chart.dataset.chapters'),
              data: rows.map((r) => r.chapters_read),
              backgroundColor: chartColor('--chart-1', 0.6),
              borderRadius: 3,
              // Without a cap, a handful of data points renders as absurdly wide slabs.
              maxBarThickness: 28,
            }],
          },
          options: {
            responsive: true,
            maintainAspectRatio: false,
            plugins: { legend: { display: false } },
            scales: {
              x: { ticks: { maxTicksLimit: 8, color: cssVar('--color-text-muted') }, grid: { display: false } },
              y: { beginAtZero: true, ticks: { precision: 0, color: cssVar('--color-text-muted') } },
            },
          },
        });
      }).catch(() => {
        // Chart.js unavailable — show plain text fallback
        container.innerHTML = `<div class="card p-4 text-sm text-text-muted">${t('stats.chart_unavailable_prefix')} <code>${_SETUP_CMD}</code> ${t('stats.chart_unavailable_suffix')}</div>`;
      });

      return {
        destroy() {
          _chart?.destroy();
          _chart = null;
          container.innerHTML = '';
        },
      };
    },
  };
}


function topMangaWidget() {
  /** @type {any} */ let _chart = null;

  return {
    id: 'top_manga',
    /** @param {HTMLElement} container @param {any} data */
    render(container, data) {
      const rows = (data.top_manga ?? []).slice(0, 10);
      if (!rows.length) {
        container.innerHTML = `
          <div class="card p-4">
            <h2 class="text-base font-semibold mb-2">${escapeHtml(t('stats.chart.most_read'))}</h2>
          </div>
        `;
        container.querySelector('.card')?.appendChild(createEmptyState({
          title: t('stats.empty.no_reading'),
          subtitle: t('stats.empty.no_reading.desc'),
          compact: true,
        }));
        return { destroy() {} };
      }

      container.innerHTML = `
        <div class="card p-4">
          <h2 class="text-base font-semibold mb-4">${escapeHtml(t('stats.chart.most_read'))}</h2>
          <div class="relative h-64">
            <canvas id="chart-top-manga" class="w-full h-full"></canvas>
          </div>
        </div>
      `;

      const canvas = /** @type {HTMLCanvasElement} */ (container.querySelector('#chart-top-manga'));

      _loadChartJs().then(Chart => {
        if (!canvas.isConnected) return;
        _chart = new Chart(canvas.getContext('2d'), {
          type: 'bar',
          data: {
            labels: rows.map((r) => r.manga_name),
            datasets: [{
              label: t('stats.chart.dataset.chapters'),
              data: rows.map((r) => r.chapters_read),
              backgroundColor: chartColor('--chart-2', 0.65),
              borderRadius: 3,
              maxBarThickness: 22,
            }],
          },
          options: {
            indexAxis: 'y',
            responsive: true,
            maintainAspectRatio: false,
            plugins: {
              legend: { display: false },
              tooltip: { callbacks: { title: (items) => rows[items[0]?.dataIndex]?.manga_name ?? '' } },
            },
            scales: {
              x: { beginAtZero: true, ticks: { precision: 0, color: cssVar('--color-text-muted') } },
              y: {
                ticks: {
                  color: cssVar('--color-text-muted'),
                  autoSkip: false,
                  /**
                   * A canvas cannot reflow, and Chart.js clips a label at the
                   * axis edge with no ellipsis, so on a phone the manga cannot
                   * be identified. Budget the label, keep the full title in the
                   * tooltip.
                   * @this {any}
                   */
                  callback(value) {
                    const label = String(this.getLabelForValue(value) ?? '');
                    const budget = Math.max(8, Math.floor(this.chart.width / 20));
                    return label.length > budget ? `${label.slice(0, budget - 1)}…` : label;
                  },
                },
              },
            },
            onClick: (_evt, elements) => {
              if (elements[0]) {
                const idx = elements[0].index;
                const mangaId = rows[idx]?.manga_id;
                if (mangaId) navigate(`/manga/${mangaId}`);
              }
            },
          },
        });
      }).catch(() => {
        container.innerHTML = `
          <div class="card p-4">
            <h2 class="text-base font-semibold mb-3">${escapeHtml(t('stats.chart.most_read'))}</h2>
            <ol class="flex flex-col gap-1 text-sm">
              ${rows.map((r, i) => `<li class="flex gap-2"><span class="text-text-muted w-4">${i + 1}.</span><a href="/manga/${r.manga_id}" class="link flex-1 truncate">${escapeHtml(r.manga_name)}</a><span class="text-text-muted">${r.chapters_read}ch</span></li>`).join('')}
            </ol>
          </div>`;
      });

      return {
        destroy() {
          _chart?.destroy();
          _chart = null;
          container.innerHTML = '';
        },
      };
    },
  };
}


function genreBreakdownWidget() {
  /** @type {any} */ let _chart = null;

  return {
    id: 'genre_breakdown',
    /** @param {HTMLElement} container @param {any} data */
    render(container, data) {
      const rows = (data.genre_breakdown ?? []).slice(0, 11);
      if (!rows.length) {
        container.innerHTML = `
          <div class="card p-4">
            <h2 class="text-base font-semibold mb-2">${escapeHtml(t('stats.chart.genre_breakdown'))}</h2>
          </div>
        `;
        container.querySelector('.card')?.appendChild(createEmptyState({
          title: t('stats.empty.no_genres'),
          subtitle: t('stats.empty.no_genres.desc'),
          compact: true,
        }));
        return { destroy() {} };
      }

      container.innerHTML = `
        <div class="card p-4">
          <h2 class="text-base font-semibold mb-4">${escapeHtml(t('stats.chart.genre_breakdown'))}</h2>
          <div class="relative h-64">
            <canvas id="chart-genre" class="w-full h-full"></canvas>
          </div>
        </div>
      `;

      const canvas = /** @type {HTMLCanvasElement} */ (container.querySelector('#chart-genre'));

      const PALETTE = [
        cssVar('--chart-1'), cssVar('--chart-2'), cssVar('--chart-3'),
        cssVar('--chart-4'), cssVar('--chart-5'), cssVar('--chart-6'),
        cssVar('--chart-7'), cssVar('--chart-8'), cssVar('--chart-9'),
        cssVar('--chart-10'), cssVar('--chart-11'),
      ];

      _loadChartJs().then(Chart => {
        if (!canvas.isConnected) return;
        _chart = new Chart(canvas.getContext('2d'), {
          type: 'doughnut',
          data: {
            labels: rows.map((r) => r.genre),
            datasets: [{
              data: rows.map((r) => r.chapters_read),
              backgroundColor: rows.map((_, i) => PALETTE[i % PALETTE.length]),
              borderWidth: 1,
            }],
          },
          options: {
            responsive: true,
            maintainAspectRatio: false,
            plugins: {
              legend: { position: 'right', labels: { color: cssVar('--color-text-muted'), boxWidth: 12 } },
            },
          },
        });
      }).catch(() => {
        container.innerHTML = `
          <div class="card p-4">
            <h2 class="text-base font-semibold mb-3">${escapeHtml(t('stats.chart.genre_breakdown'))}</h2>
            <ul class="flex flex-col gap-1 text-sm">
              ${rows.map(r => `<li class="flex gap-2 justify-between"><span>${escapeHtml(r.genre)}</span><span class="text-text-muted">${r.chapters_read}</span></li>`).join('')}
            </ul>
          </div>`;
      });

      return {
        destroy() {
          _chart?.destroy();
          _chart = null;
          container.innerHTML = '';
        },
      };
    },
  };
}


/** @type {Promise<any> | null} */ let _chartJsPromise = null;

function _loadChartJs() {
  if (!_chartJsPromise) {
    _chartJsPromise = new Promise((resolve, reject) => {
      if (window.Chart) { resolve(window.Chart); return; }
      const existing = document.querySelector('script[data-chartjs]');
      if (existing) {
        existing.addEventListener('load',  () => resolve(window.Chart));
        existing.addEventListener('error', reject);
        return;
      }
      const s = document.createElement('script');
      s.src = '/js/vendor/chart.umd.min.js';
      s.dataset.chartjs = '1';
      s.onload  = () => resolve(window.Chart);
      s.onerror = () => reject(new Error('Chart.js not found — run: kani-cli setup'));
      document.head.appendChild(s);
    });
  }
  return _chartJsPromise;
}
