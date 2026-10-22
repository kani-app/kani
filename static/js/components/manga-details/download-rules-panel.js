// @ts-check

import { h, render } from 'preact';
import { useState, useEffect, useRef, useCallback } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { t } from '../../i18n.js';
import { debounce } from '../../utils.js';
import { showToast } from '../toast.js';
import { EmptyState } from '../empty-state.js';
import { Select } from '../form/select.js';
import { Combobox } from '../combobox.js';
import { iconX, iconPencil, iconCheck } from '../../icons.js';
const html = htm.bind(h);


function _ruleLabel(kind) {
  if (typeof kind === 'string') return kind;
  const [k, v] = Object.entries(kind)[0] ?? ['', ''];
  const labels = /** @type {Record<string,string>} */ ({
    LanguageInclude: 'Language = ' + v,
    LanguageExclude: 'Language ≠ ' + v,
    TitleContains: 'Title contains "' + v + '"',
    TitleExcludes: 'Title excludes "' + v + '"',
    ChapterNumberMin: 'Chapter ≥ ' + v,
    ChapterNumberMax: 'Chapter ≤ ' + v,
    MaxAgeDays: 'Max age: ' + v + ' days',
    PublishedAfter: 'Published after ' + new Date(Number(v) * 1000).toLocaleDateString(),
  });
  return labels[k] ?? (k + ': ' + v);
}

function _buildKind(type, v) {
  if (type === 'ExcludeFractional') return 'ExcludeFractional';
  if (['ChapterNumberMin', 'ChapterNumberMax', 'MaxAgeDays'].includes(type)) return { [type]: Number(v) };
  if (type === 'PublishedAfter') return { PublishedAfter: Math.floor(new Date(v).getTime() / 1000) };
  return { [type]: v };
}

function _epochToDateInput(epoch) {
  return new Date(epoch * 1000).toISOString().slice(0, 10);
}


/**
 * @param {HTMLElement} bodyEl
 * @param {any[]} initialRules
 * @param {number} dbId
 */
export function mountDownloadRulesPanel(bodyEl, initialRules, dbId) {
  const mount = document.createElement('div');
  bodyEl.appendChild(mount);
  render(html`<${DownloadRulesPanel} initialRules=${initialRules} dbId=${dbId} />`, mount);
}


function DownloadRulesPanel({ initialRules, dbId }) {
  const [rules, setRules] = useState(/** @type {any[]} */ (Array.isArray(initialRules) ? [...initialRules] : []));
  const [editingId, setEditingId] = useState(/** @type {number|null} */ (null));
  const [langOptions, setLangOptions] = useState(/** @type {Array<{id:number,name:string}>} */ ([]));
  const [preview, setPreview] = useState('');
  const dragFromIdx = useRef(/** @type {number|null} */ (null));
  const rulesRef = useRef(rules);
  rulesRef.current = rules;

  const refreshPreview = useCallback(
    debounce(async (currentRules) => {
      setPreview(t('manga.rules.preview.calculating'));
      try {
        const res = await api.previewDownloadRules(dbId, currentRules.map(r => r.kind));
        setPreview(
          res.matching === res.total
            ? t('manga.rules.preview.all')
            : t('manga.rules.preview.result', { matching: res.matching, total: res.total }),
        );
      } catch { setPreview(''); }
    }, 400),
    [dbId],
  );

  useEffect(() => {
    api.getChapterLanguages(dbId).then(langs => {
      setLangOptions((Array.isArray(langs) ? langs : []).map((l, i) => ({ id: i, name: l })));
    }).catch(() => {});
  }, [dbId]);

  useEffect(() => {
    refreshPreview(rules);
  }, [rules, refreshPreview]);

  const usedLangs = new Set(
    rules
      .filter(r => typeof r.kind === 'object' && ('LanguageInclude' in r.kind || 'LanguageExclude' in r.kind))
      .map(r => /** @type {string} */ (Object.values(r.kind)[0]))
  );
  const availLangOpts = langOptions.filter(o => !usedLangs.has(o.name));

  async function handleAdd(type, value, langVal) {
    const isLang = type === 'LanguageInclude' || type === 'LanguageExclude';
    const rawVal = isLang ? langVal : value;
    if (type !== 'ExcludeFractional' && !rawVal) return;
    const kind = _buildKind(type, rawVal);
    try {
      const newRule = await api.addDownloadRule(dbId, kind);
      if (newRule?.id) setRules(prev => [...prev, { id: newRule.id, manga_id: dbId, kind }]);
    } catch (e) {
      showToast(/** @type {any} */(e)?.hint ?? /** @type {any} */(e)?.message ?? t('manga.rules.add_failed'), { type: 'error' });
      throw e;
    }
  }

  async function handleUpdate(ruleId, idx, type, value, langVal) {
    const isLang = type === 'LanguageInclude' || type === 'LanguageExclude';
    const rawVal = isLang ? langVal : value;
    if (type !== 'ExcludeFractional' && !rawVal) return;
    const kind = _buildKind(type, rawVal);
    try {
      await api.updateDownloadRule(ruleId, kind);
      setRules(prev => prev.map((r, i) => i === idx ? { ...r, kind } : r));
      setEditingId(null);
    } catch (e) {
      showToast(/** @type {any} */(e)?.hint ?? t('manga.rules.update_failed'), { type: 'error' });
    }
  }

  async function handleDelete(ruleId) {
    try {
      await api.deleteDownloadRule(ruleId);
      setRules(prev => prev.filter(r => r.id !== ruleId));
    } catch { }
  }

  function handleDragStart(idx) {
    dragFromIdx.current = idx;
  }

  function handleDragOver(e, toIdx) {
    e.preventDefault();
    const fromIdx = dragFromIdx.current;
    if (fromIdx === null || fromIdx === toIdx) return;
    dragFromIdx.current = toIdx;
    setRules(prev => {
      const next = [...prev];
      const [moved] = next.splice(fromIdx, 1);
      next.splice(toIdx, 0, moved);
      return next;
    });
  }

  async function handleDrop(e) {
    e.preventDefault();
    dragFromIdx.current = null;
    try {
      await api.reorderDownloadRules(dbId, rulesRef.current.map(r => r.id));
    } catch { }
  }

  return html`
    <div class="flex flex-col gap-3">
      ${rules.length > 0
        ? html`
          <ul class="flex flex-col divide-y divide-border-subtle">
            ${rules.map((rule, idx) => html`<${RuleRow}
              key=${rule.id}
              rule=${rule}
              idx=${idx}
              editing=${editingId === rule.id}
              langOptions=${availLangOpts}
              onEdit=${() => setEditingId(rule.id)}
              onCancel=${() => setEditingId(null)}
              onSave=${(type, value, langVal) => handleUpdate(rule.id, idx, type, value, langVal)}
              onDelete=${() => handleDelete(rule.id)}
              onDragStart=${() => handleDragStart(idx)}
              onDragOver=${(e) => handleDragOver(e, idx)}
              onDrop=${handleDrop}
              onDragEnd=${() => { dragFromIdx.current = null; }}
            />`)}
          </ul>
        `
        : html`<${EmptyState} title=${t('manga.rules.empty')} />`
      }
      <${AddRuleForm} langOptions=${availLangOpts} onAdd=${handleAdd} />
      <p class="text-sm text-text-muted">${preview}</p>
    </div>
  `;
}


function RuleRow({ rule, idx, editing, langOptions, onEdit, onCancel, onSave, onDelete, onDragStart, onDragOver, onDrop, onDragEnd }) {
  return html`
    <li
      class="flex flex-col gap-2 py-2"
      draggable=${true}
      onDragStart=${onDragStart}
      onDragEnd=${onDragEnd}
      onDragOver=${onDragOver}
      onDrop=${onDrop}
    >
      ${editing
        ? html`<${EditRuleForm} rule=${rule} idx=${idx} langOptions=${langOptions} onSave=${onSave} onCancel=${onCancel} />`
        : html`
          <div class="flex items-center justify-between gap-2">
            <span class="cursor-grab text-text-faint select-none shrink-0" title=${t('manga.rules.drag_reorder')}>⠿</span>
            <span class="text-sm text-text flex-1">${_ruleLabel(rule.kind)}</span>
            <button class="btn-icon" aria-label=${t('manga.rules.edit_rule')} onClick=${onEdit} dangerouslySetInnerHTML=${{ __html: iconPencil }} />
            <button class="btn-icon text-danger" aria-label=${t('manga.rules.remove_rule')} onClick=${onDelete} dangerouslySetInnerHTML=${{ __html: iconX }} />
          </div>
        `
      }
    </li>
  `;
}


function EditRuleForm({ rule, langOptions, onSave, onCancel }) {
  const ruleType = typeof rule.kind === 'string' ? rule.kind : Object.keys(rule.kind)[0];
  const ruleVal = typeof rule.kind === 'string' ? '' : String(Object.values(rule.kind)[0]);

  const initVal = (ruleType === 'PublishedAfter' ? _epochToDateInput(Number(ruleVal)) : ruleVal);
  const initLangVal = (ruleType === 'LanguageInclude' || ruleType === 'LanguageExclude') ? ruleVal : '';

  return html`<${RuleForm}
    initType=${ruleType}
    initValue=${initVal}
    initLangVal=${initLangVal}
    langOptions=${langOptions}
    onSubmit=${onSave}
    onCancel=${onCancel}
    submitLabel=${html`<span class="icon-xs" dangerouslySetInnerHTML=${{ __html: iconCheck }} />${' ' + t('common.save')}`}
  />`;
}


function AddRuleForm({ langOptions, onAdd }) {
  return html`<${RuleForm}
    initType="LanguageInclude"
    initValue=""
    initLangVal=""
    langOptions=${langOptions}
    onSubmit=${onAdd}
    onCancel=${null}
    submitLabel=${t('common.add')}
  />`;
}


function RuleForm({ initType, initValue, initLangVal, langOptions, onSubmit, onCancel, submitLabel }) {
  const [type, setType] = useState(initType);
  const [value, setValue] = useState(initValue);
  const [langVal, setLangVal] = useState(initLangVal);

  const isLang = type === 'LanguageInclude' || type === 'LanguageExclude';
  const isNoVal = type === 'ExcludeFractional';
  const isDate = type === 'PublishedAfter';
  const isNumber = ['ChapterNumberMin', 'ChapterNumberMax', 'MaxAgeDays'].includes(type);

  const valPlaceholder = isNumber ? t('manga.rules.number_placeholder') : t('manga.rules.value_placeholder');
  const langCmbValue = langOptions.find(o => o.name === langVal)?.id ?? null;

  async function handleSubmit() {
    try {
      await onSubmit(type, value, langVal);
      if (!onCancel) {
        setValue('');
        setLangVal('');
        setType('LanguageInclude');
      }
    } catch { }
  }

  return html`
    <div class="flex flex-wrap items-center gap-2 mt-2">
      <${Select}
        options=${[
          { value: 'LanguageInclude', label: t('manga.rules.type.lang_include'), group: t('manga.rules.group.language') },
          { value: 'LanguageExclude', label: t('manga.rules.type.lang_exclude'), group: t('manga.rules.group.language') },
          { value: 'TitleContains', label: t('manga.rules.type.title_contains'), group: t('manga.rules.group.title') },
          { value: 'TitleExcludes', label: t('manga.rules.type.title_excludes'), group: t('manga.rules.group.title') },
          { value: 'ChapterNumberMin', label: t('manga.rules.type.chapter_min'), group: t('manga.rules.group.chapter_number') },
          { value: 'ChapterNumberMax', label: t('manga.rules.type.chapter_max'), group: t('manga.rules.group.chapter_number') },
          { value: 'ExcludeFractional', label: t('manga.rules.type.exclude_fractional'), group: t('manga.rules.group.other') },
          { value: 'MaxAgeDays', label: t('manga.rules.type.max_age'), group: t('manga.rules.group.other') },
          { value: 'PublishedAfter', label: t('manga.rules.type.published_after'), group: t('manga.rules.group.other') },
        ]}
        value=${type}
        ariaLabel=${t('manga.rules.type.aria')}
        onChange=${(/** @type {string} */ v) => {
          setType(v);
          setValue('');
          setLangVal('');
        }}
      />

      ${isLang && html`
        <div class="flex-1 min-w-36">
          <${Combobox}
            options=${langOptions}
            value=${langCmbValue}
            onChange=${(id) => { setLangVal(langOptions.find(o => o.id === id)?.name ?? ''); }}
            placeholder=${t('manga.rules.lang_placeholder')}
          />
        </div>
      `}
      ${!isLang && !isNoVal && html`
        <input
          type=${isDate ? 'date' : 'text'}
          class="input flex-1 min-w-24 text-sm"
          value=${value}
          placeholder=${valPlaceholder}
          onInput=${(e) => setValue(/** @type {HTMLInputElement} */ (e.target).value)}
        />
      `}

      <button type="button" class="btn-ghost btn-sm flex items-center gap-1" onClick=${handleSubmit}>${submitLabel}</button>
      ${onCancel && html`<button type="button" class="btn-ghost btn-sm text-text-muted" onClick=${onCancel}>${t('common.cancel')}</button>`}
    </div>
  `;
}
