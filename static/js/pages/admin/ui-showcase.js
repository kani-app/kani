// @ts-check

import { setPageHeader, clearPageHeader } from '../../components/app-header.js';
import { createEmptyState } from '../../components/empty-state.js';
import { showToast, showApiError } from '../../components/toast.js';
import { showConfirm, showAlert } from '../../components/modal.js';
import { createStarCheckbox } from '../../components/star-checkbox.js';
import { createTagList } from '../../components/tag-list.js';
import { skeletonGrid, skeletonSourceList } from '../../components/skeletons.js';
import { renderPagination } from '../../components/pagination.js';
import { renderChipGroup } from '../../components/chip-group.js';
import { renderTabs } from '../../components/tabs.js';
import { Pill } from '../../components/pill.js';
import { NumberInput } from '../../components/form/number-input.js';
import { createCallout } from '../../components/form/callout.js';
import { Select } from '../../components/form/select.js';
import { DateInput } from '../../components/form/date-input.js';
import { Combobox } from '../../components/combobox.js';
import { COMPONENTS } from './showcase-manifest.js';
import { h, render } from 'preact';
import htm from 'htm';
const html = htm.bind(h);

/** @param {{ title: string, usage: string, el?: HTMLElement | null, slot?: any }} props */
function ShowcaseRow({ title, usage, el, slot }) {
  return html`
    <div class="grid grid-cols-1 md:grid-cols-2 gap-4 py-4 border-b border-border-subtle last:border-b-0 items-start">
      <div class="flex flex-col gap-1">
        <p class="text-sm font-medium text-text">${title}</p>
        <code class="text-xs text-text-muted font-mono bg-surface-2 rounded px-2 py-1 block whitespace-pre-wrap break-all">${usage}</code>
      </div>
      <div class="flex flex-wrap gap-2 items-start" ref=${slot ? undefined : (el ? (container => { if (container && el) container.appendChild(el); }) : undefined)}>
        ${slot ?? null}
      </div>
    </div>
  `;
}

/** @param {{ title: string, children: any }} props */
function ShowcaseSection({ title, children }) {
  return html`
    <section class="bg-surface border border-border rounded-xl overflow-hidden">
      <div class="px-5 py-3 bg-surface-2 border-b border-border">
        <h2 class="text-xs font-semibold uppercase tracking-wider text-text-muted">${title}</h2>
      </div>
      <div class="px-5 divide-y divide-border-subtle">
        ${children}
      </div>
    </section>
  `;
}

/** @param {HTMLElement} container */
function _renderSection(title, rows) {
  const sec = document.createElement('section');
  sec.className = 'bg-surface border border-border rounded-xl overflow-hidden';
  const header = document.createElement('div');
  header.className = 'px-5 py-3 bg-surface-2 border-b border-border';
  header.innerHTML = `<h2 class="text-xs font-semibold uppercase tracking-wider text-text-muted">${title}</h2>`;
  sec.appendChild(header);
  const body = document.createElement('div');
  body.className = 'px-5';
  for (const row of rows) {
    const div = document.createElement('div');
    div.className = 'grid grid-cols-1 md:grid-cols-2 gap-4 py-4 border-b border-border-subtle last:border-b-0 items-start';

    const meta = document.createElement('div');
    meta.className = 'flex flex-col gap-1';
    meta.innerHTML = `
      <p class="text-sm font-medium text-text">${row.title}</p>
      <code class="text-xs text-text-muted font-mono bg-surface-2 rounded px-2 py-1 block whitespace-pre-wrap break-all">${row.usage}</code>
    `;
    div.appendChild(meta);

    const live = document.createElement('div');
    live.className = 'flex flex-wrap gap-2 items-start min-h-8';
    row.mount(live);
    div.appendChild(live);

    body.appendChild(div);
  }
  sec.appendChild(body);
  return sec;
}

/** @param {HTMLElement} container */
export function init(container) {
  document.title = 'UI Showcase – Kani';
  setPageHeader({ crumbs: [{ label: 'Admin' }, { label: 'UI Showcase' }] });

  const root = document.createElement('div');
  root.className = 'max-w-page mx-auto w-full px-4 md:px-6 py-6 flex flex-col gap-6';

  const heading = document.createElement('div');
  heading.innerHTML = `
    <h1 class="text-2xl font-bold text-text">Component Showcase</h1>
    <p class="text-sm text-text-muted mt-1">Live catalog of every reusable component. Use this page to verify tokens, themes, and component states.</p>
  `;
  root.appendChild(heading);

  root.appendChild(_renderSection('Feedback', [
    {
      title: 'Toast — info',
      usage: "showToast('Message', { type: 'info' })",
      mount: (el) => {
        const btn = document.createElement('button');
        btn.className = 'btn-ghost btn-sm';
        btn.textContent = 'Info toast';
        btn.addEventListener('click', () => showToast('This is an info message', { type: 'info' }));
        el.appendChild(btn);
      },
    },
    {
      title: 'Toast — success',
      usage: "showToast('Saved!', { type: 'success' })",
      mount: (el) => {
        const btn = document.createElement('button');
        btn.className = 'btn-ghost btn-sm';
        btn.textContent = 'Success toast';
        btn.addEventListener('click', () => showToast('Saved successfully!', { type: 'success' }));
        el.appendChild(btn);
      },
    },
    {
      title: 'Toast — warn',
      usage: "showToast('Warning', { type: 'warn' })",
      mount: (el) => {
        const btn = document.createElement('button');
        btn.className = 'btn-ghost btn-sm';
        btn.textContent = 'Warn toast';
        btn.addEventListener('click', () => showToast('Something to be aware of', { type: 'warn' }));
        el.appendChild(btn);
      },
    },
    {
      title: 'Toast — error',
      usage: "showToast('Failed', { type: 'error' })",
      mount: (el) => {
        const btn = document.createElement('button');
        btn.className = 'btn-ghost btn-sm';
        btn.textContent = 'Error toast';
        btn.addEventListener('click', () => showToast('Something went wrong', { type: 'error' }));
        el.appendChild(btn);
      },
    },
    {
      title: 'Toast — with undo action',
      usage: "showToast('Deleted', { type: 'info', action: { label: 'Undo', onClick: fn } })",
      mount: (el) => {
        const btn = document.createElement('button');
        btn.className = 'btn-ghost btn-sm';
        btn.textContent = 'Toast with action';
        btn.addEventListener('click', () =>
          showToast('Item deleted', { type: 'info', action: { label: 'Undo', onClick: () => showToast('Undone', { type: 'success' }) } })
        );
        el.appendChild(btn);
      },
    },
    {
      title: 'showApiError(err)',
      usage: 'showApiError(err) — shows error message from any thrown API response',
      mount: (el) => {
        const btn = document.createElement('button');
        btn.className = 'btn-ghost btn-sm';
        btn.textContent = 'Simulate API error';
        btn.addEventListener('click', () => showApiError({ message: 'Simulated API error', status: 422 }));
        el.appendChild(btn);
      },
    },
    {
      title: 'showConfirm()',
      usage: "showConfirm('Are you sure?', { danger: true })",
      mount: (el) => {
        const btn = document.createElement('button');
        btn.className = 'btn-ghost btn-sm';
        btn.textContent = 'Confirm dialog';
        btn.addEventListener('click', async () => {
          const ok = await showConfirm('Delete this item?', { danger: true, confirmLabel: 'Delete' });
          showToast(ok ? 'Confirmed' : 'Cancelled', { type: ok ? 'success' : 'info' });
        });
        el.appendChild(btn);
      },
    },
    {
      title: 'showAlert()',
      usage: "showAlert('Information message')",
      mount: (el) => {
        const btn = document.createElement('button');
        btn.className = 'btn-ghost btn-sm';
        btn.textContent = 'Alert dialog';
        btn.addEventListener('click', () => showAlert('This is an informational alert.'));
        el.appendChild(btn);
      },
    },
  ]));

  root.appendChild(_renderSection('States', [
    {
      title: 'createEmptyState()',
      usage: "createEmptyState({ icon: '📚', title: 'Nothing here', subtitle: '…' })",
      mount: (el) => {
        el.className += ' w-full';
        const es = createEmptyState({ icon: '📚', title: 'Your library is empty', subtitle: 'Add manga to get started.' });
        el.appendChild(es);
      },
    },
    {
      title: 'createEmptyState() with action',
      usage: "createEmptyState({ …, action: { label: 'Browse', href: '/sources' } })",
      mount: (el) => {
        el.className += ' w-full';
        const es = createEmptyState({ icon: '🔍', title: 'No results', subtitle: 'Try a different search term.', action: { label: 'Browse sources', href: '/sources' } });
        el.appendChild(es);
      },
    },
    {
      title: 'skeletonGrid()',
      usage: 'skeletonGrid(6) — manga card grid skeletons',
      mount: (el) => {
        el.className += ' w-full';
        const sk = document.createElement('div');
        sk.innerHTML = skeletonGrid(6);
        el.appendChild(sk);
      },
    },
    {
      title: 'skeletonSourceList()',
      usage: 'skeletonSourceList(3) — source list skeletons',
      mount: (el) => {
        el.className += ' w-full';
        const sk = document.createElement('div');
        sk.innerHTML = skeletonSourceList(3);
        el.appendChild(sk);
      },
    },
  ]));

  root.appendChild(_renderSection('Inputs & Controls', [
    {
      title: 'Star checkbox (favourite toggle)',
      usage: "createStarCheckbox({ checked, onChange, label: 'Favourite' })",
      mount: (el) => {
        const star = createStarCheckbox({ checked: false, onChange: () => {}, label: 'Favourite' });
        el.appendChild(star.el);
      },
    },
    {
      title: 'renderChipGroup()',
      usage: "renderChipGroup(el, { items, selected, onToggle, multi: true })",
      mount: (el) => {
        let sel = new Set(['Action']);
        const items = [{ id: 'Action', label: 'Action' }, { id: 'Romance', label: 'Romance' }, { id: 'Sci-Fi', label: 'Sci-Fi' }];
        const update = () => {
          el.innerHTML = '';
          renderChipGroup(el, { items: items.map(i => ({ ...i, label: i.label })), selected: sel, onToggle: (id) => {
            sel = new Set(sel);
            sel.has(id) ? sel.delete(id) : sel.add(id);
            update();
          }, multi: true });
        };
        update();
      },
    },
    {
      title: 'renderTabs()',
      usage: "renderTabs(el, { tabs, activeId, onSelect })",
      mount: (el) => {
        const tabs = [{ id: 'a', name: 'Active' }, { id: 'b', name: 'History' }, { id: 'c', name: 'Failed' }];
        const handle = renderTabs(el, { tabs, activeId: 'a', onSelect: (id) => handle.update(id) });
      },
    },
    {
      title: 'NumberInput (steppers)',
      usage: "html`<${NumberInput} value min max onChange />`",
      mount: (el) => {
        render(html`<${NumberInput} value=${5} min=${0} max=${23} onChange=${() => {}} />`, el);
      },
    },
    {
      title: 'Select',
      usage: "html`<${Select} options value onChange />`",
      mount: (el) => {
        let value = 'daily';
        const options = [
          { value: 'daily', label: 'Daily' },
          { value: 'weekly', label: 'Weekly' },
          { value: 'monthly', label: 'Monthly' },
        ];
        const update = () => render(html`<${Select} options=${options} value=${value} onChange=${(v) => { value = v; update(); }} />`, el);
        update();
      },
    },
    {
      title: 'DateInput',
      usage: "html`<${DateInput} label value onChange />`",
      mount: (el) => {
        let value = '2026-07-12';
        const update = () => render(html`<${DateInput} label="From" value=${value} onChange=${(v) => { value = v; update(); }} />`, el);
        update();
      },
    },
    {
      title: 'Combobox — multi (chips in input)',
      usage: "html`<${Combobox} multiple options value onChange />`",
      mount: (el) => {
        let value = [1];
        const options = [{ id: 1, name: 'Action' }, { id: 2, name: 'Romance' }, { id: 3, name: 'Sci-Fi' }, { id: 4, name: 'Slice of Life' }];
        const update = () => render(html`<div class="w-72"><${Combobox} multiple options=${options} value=${value} onChange=${(v) => { value = v; update(); }} /></div>`, el);
        update();
      },
    },
  ]));

  root.appendChild(_renderSection('Callouts', [
    {
      title: 'Callout — info / warn / danger',
      usage: "createCallout({ tone, text })",
      mount: (el) => {
        const wrap = document.createElement('div');
        wrap.className = 'flex flex-col gap-2 w-full';
        wrap.appendChild(createCallout({ tone: 'info', text: 'Session timeout applies after the next restart.' }));
        wrap.appendChild(createCallout({ tone: 'warn', text: 'SMTP credentials are stored unencrypted unless a secret key is set.' }));
        wrap.appendChild(createCallout({ tone: 'danger', text: 'This permanently deletes all downloaded chapters.' }));
        el.appendChild(wrap);
      },
    },
  ]));

  root.appendChild(_renderSection('Layout & Navigation', [
    {
      title: 'renderPagination()',
      usage: "renderPagination(el, { page, hasNext, total, onPageChange })",
      mount: (el) => {
        let page = 3;
        const update = () => {
          el.innerHTML = '';
          renderPagination(el, { page, hasNext: true, total: 120, onPageChange: (p) => { page = p; update(); } });
        };
        update();
      },
    },
    {
      title: 'createTagList()',
      usage: "createTagList({ tags: [{id, name}], getHref })",
      mount: (el) => {
        const list = createTagList({ tags: [{ id: 1, name: 'Action' }, { id: 2, name: 'Adventure' }, { id: 3, name: 'Sci-Fi' }], getHref: (id) => `/?tag_id=${id}` });
        el.appendChild(list);
      },
    },
  ]));

  root.appendChild(_renderSection('Buttons & Chips', [
    {
      title: 'Button variants',
      usage: '.btn-primary  .btn-ghost  .btn-danger  .btn-icon',
      mount: (el) => {
        el.innerHTML = `
          <button class="btn-primary btn-sm">Primary</button>
          <button class="btn-ghost btn-sm">Ghost</button>
          <button class="btn-danger btn-sm">Danger</button>
          <button class="btn-icon btn-sm" aria-label="Close">✕</button>
        `;
      },
    },
    {
      title: 'Button — disabled state',
      usage: "button[disabled]",
      mount: (el) => {
        el.innerHTML = `
          <button class="btn-primary btn-sm" disabled>Primary disabled</button>
          <button class="btn-ghost btn-sm" disabled>Ghost disabled</button>
        `;
      },
    },
    {
      title: 'Chip variants',
      usage: '.chip  .chip-active',
      mount: (el) => {
        el.innerHTML = `
          <button class="chip">Inactive</button>
          <button class="chip chip-active">Active</button>
          <button class="chip" disabled>Disabled</button>
        `;
      },
    },
    {
      title: 'Pill (dismissible tag)',
      usage: "Pill({ label: 'Tag', onDismiss })",
      mount: (el) => {
        const { h: ph, render: pr } = /** @type {any} */ ({ h, render });
        const container = document.createElement('div');
        container.className = 'flex gap-2 flex-wrap';
        const tags = ['Action', 'Romance', 'Sci-Fi'];
        let remaining = [...tags];
        const rerender = () => {
          render(null, container);
          remaining.forEach(label => {
            const wrapper = document.createElement('span');
            render(h(Pill, { label, onDismiss: () => { remaining = remaining.filter(t => t !== label); rerender(); } }), wrapper);
            container.appendChild(wrapper);
          });
        };
        rerender();
        el.appendChild(container);
      },
    },
  ]));

  root.appendChild(_renderSection('Typography & Color Tokens', [
    {
      title: 'Text scale',
      usage: 'text-xs  text-sm  text-base  text-lg  text-xl  text-2xl  text-3xl',
      mount: (el) => {
        el.className += ' flex flex-col gap-1 w-full';
        el.innerHTML = `
          <span class="text-xs text-text">text-xs — Body small</span>
          <span class="text-sm text-text">text-sm — Body</span>
          <span class="text-base text-text">text-base — Body base</span>
          <span class="text-lg text-text font-medium">text-lg — Heading small</span>
          <span class="text-xl text-text font-semibold">text-xl — Heading</span>
          <span class="text-2xl text-text font-bold">text-2xl — Title</span>
          <span class="text-3xl text-text font-bold">text-3xl — Display</span>
        `;
      },
    },
    {
      title: 'Color roles',
      usage: '--color-text  --color-text-muted  --color-text-faint  --color-accent',
      mount: (el) => {
        el.className += ' flex flex-col gap-1 w-full';
        el.innerHTML = `
          <span class="text-sm text-text">text — Primary text</span>
          <span class="text-sm text-text-muted">text-muted — Secondary text</span>
          <span class="text-sm text-text-faint">text-faint — Tertiary / placeholder</span>
          <span class="text-sm text-accent font-medium">text-accent — Accent / brand</span>
          <span class="text-sm text-success">text-success — Success state</span>
          <span class="text-sm text-warn">text-warn — Warning state</span>
          <span class="text-sm text-danger">text-danger — Danger state</span>
        `;
      },
    },
    {
      title: 'Surface layers',
      usage: '--color-bg  --color-surface  --color-surface-2  --color-surface-3',
      mount: (el) => {
        el.className += ' flex gap-2 w-full';
        el.innerHTML = `
          <div class="flex-1 h-12 rounded-lg bg-bg border border-border flex items-center justify-center text-xs text-text-muted">bg</div>
          <div class="flex-1 h-12 rounded-lg bg-surface border border-border flex items-center justify-center text-xs text-text-muted">surface</div>
          <div class="flex-1 h-12 rounded-lg bg-surface-2 border border-border flex items-center justify-center text-xs text-text-muted">surface-2</div>
          <div class="flex-1 h-12 rounded-lg bg-surface-3 border border-border flex items-center justify-center text-xs text-text-muted">surface-3</div>
        `;
      },
    },
    {
      title: 'Shadow scale',
      usage: '--shadow-sm  --shadow-md  --shadow-lg  --shadow-card  --shadow-popover',
      mount: (el) => {
        el.className += ' flex gap-3 flex-wrap w-full';
        for (const [cls, label] of [['shadow-sm', 'sm'], ['shadow-md', 'md'], ['shadow-lg', 'lg'], ['shadow-card', 'card']]) {
          const div = document.createElement('div');
          div.className = `w-16 h-12 rounded-lg bg-surface ${cls} flex items-center justify-center text-xs text-text-muted`;
          div.textContent = label;
          el.appendChild(div);
        }
      },
    },
  ]));

  root.appendChild(_renderCoverage());

  container.appendChild(root);
}

/**
 * Lists every component in the generated manifest and whether this page demos it.
 * Hand-written demos drift behind the component directory silently; showing the
 * gap here is what stops that.
 */
function _renderCoverage() {
  const demoed = new Set(
    [...document.querySelectorAll('code')].map((c) => c.textContent ?? ''),
  );
  const rows = COMPONENTS.map((entry) => {
    const shown = entry.exports.some((name) =>
      [...demoed].some((usage) => usage.includes(name)),
    );
    return { module: entry.module, exports: entry.exports, shown };
  });
  const missing = rows.filter((r) => !r.shown);

  const section = document.createElement('section');
  section.className = 'flex flex-col gap-2';
  const head = document.createElement('div');
  head.innerHTML = `
    <h2 class="text-lg font-semibold text-text">Coverage</h2>
    <p class="text-sm text-text-muted mt-1">
      ${rows.length - missing.length} of ${rows.length} components appear above.
      The list is generated from <code class="font-mono text-xs">static/js/components/</code>,
      so a new component shows up here whether or not anyone demos it.
    </p>
  `;
  section.appendChild(head);

  const list = document.createElement('ul');
  list.className = 'grid grid-cols-1 md:grid-cols-2 gap-x-6 text-sm';
  for (const row of rows) {
    const item = document.createElement('li');
    item.className = 'flex items-baseline gap-2 py-1 border-b border-border-subtle';
    item.innerHTML = `
      <span class="${row.shown ? 'text-text-muted' : 'text-accent'}">${row.shown ? '✓' : '—'}</span>
      <code class="font-mono text-xs">${row.module.replace('components/', '')}</code>
      <span class="text-xs text-text-muted ml-auto">${row.exports.join(', ')}</span>
    `;
    list.appendChild(item);
  }
  section.appendChild(list);
  return section;
}

/** @param {HTMLElement} container */
export function destroy(container) {
  clearPageHeader();
  container.innerHTML = '';
}
