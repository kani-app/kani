// @ts-check

import { h, render } from 'preact';
import { useState, useRef, useEffect } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { Modal, mountIntoModalRoot } from '../modal.js';
import { Combobox } from '../combobox.js';
import { showApiError } from '../toast.js';
import { iconPencil, iconRefresh, iconX } from '../../icons.js';
import { mkCard, mkRow, mkItem } from './_shared.js';
import { hasPermission } from '../../session.js';
import { getLocal, setLocal } from '../../utils.js';
import { subscribeJob } from '../../sse.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);


const STATUS_OPTIONS = () => [
  { value: '', label: t('manga.status.use_source') },
  { value: 0, label: t('manga.status.unknown') },
  { value: 1, label: t('manga.status.ongoing') },
  { value: 2, label: t('manga.status.completed') },
  { value: 3, label: t('manga.status.hiatus') },
  { value: 4, label: t('manga.status.cancelled') },
];


/** @param {{ status: string|null }} props */
function SaveStatus({ status }) {
  if (!status) return null;
  if (status === 'error') return html`<span class="text-xs text-danger">${t('manga.meta.save.error')}</span>`;
  if (status === 'saving') return html`<span class="text-xs text-text-muted">${t('manga.meta.save.saving')}</span>`;
  return html`<span class="text-xs text-accent">${t('manga.meta.save.saved')}</span>`;
}

/** Small dot shown next to a field heading when an override is active. */
function OverrideDot() {
  return html`<span class="ml-1.5 inline-block w-1.5 h-1.5 rounded-full bg-accent align-middle" title=${t('manga.meta.override_active')}></span>`;
}


/**
 * @param {{
 *   onClose: () => void,
 *   dbId: number,
 *   initialData: any,
 *   onFieldSaved: (patch: any) => void,
 * }} props
 */
function MetadataEditModal({ onClose, dbId, initialData: d, onFieldSaved }) {
  const [authorOptions, setAuthorOptions] = useState(/** @type {Array<{id:number,name:string}>} */([]));
  const [artistOptions, setArtistOptions] = useState(/** @type {Array<{id:number,name:string}>} */([]));
  const [tagOptions, setTagOptions] = useState(/** @type {Array<{id:number,name:string}>} */([]));
  useEffect(() => {
    api.getFilterAuthors().then(setAuthorOptions).catch(() => {});
    api.getFilterArtists().then(setArtistOptions).catch(() => {});
    api.getFilterTags().then(setTagOptions).catch(() => {});
  }, []);

  const [coverOverridden, setCoverOverridden] = useState(d.cover_overridden ?? false);
  const [coverTs, setCoverTs] = useState(Date.now());
  const [coverStatus, setCoverStatus] = useState(/** @type {string|null} */(null));
  const fileInputRef = useRef(/** @type {HTMLInputElement|null} */(null));

  const [localName, setLocalName] = useState(d.local_name ?? '');
  const [titleStatus, setTitleStatus] = useState(/** @type {string|null} */(null));
  const savedName = useRef(d.local_name ?? '');

  const [localDesc, setLocalDesc] = useState(d.local_description ?? '');
  const [descStatus, setDescStatus] = useState(/** @type {string|null} */(null));
  const descTimer = useRef(/** @type {ReturnType<typeof setTimeout>|null} */(null));
  const descMounted = useRef(false);

  const [localStatus, setLocalStatus] = useState(d.local_status != null ? String(d.local_status) : '');
  const [statusStatus, setStatusStatus] = useState(/** @type {string|null} */(null));

  /** @param {any} arr @returns {string[]} */
  const toNames = (arr) => (Array.isArray(arr) ? arr : []).map(a => typeof a === 'string' ? a : (a?.name ?? ''));
  const srcAuthors = toNames(d.source_authors?.length ? d.source_authors : d.authors);
  const srcArtists = toNames(d.source_artists?.length ? d.source_artists : d.artists);
  const [authors, setAuthors] = useState(/** @type {string[]} */(d.has_local_people ? toNames(d.local_authors) : srcAuthors));
  const [artists, setArtists] = useState(/** @type {string[]} */(d.has_local_people ? toNames(d.local_artists) : srcArtists));
  const [hasLocalPeople, setHasLocalPeople] = useState(d.has_local_people ?? false);
  const [peopleStatus, setPeopleStatus] = useState(/** @type {string|null} */(null));

  const srcTags = toNames(d.source_tags?.length ? d.source_tags : d.tags);
  const [tags, setTags] = useState(/** @type {string[]} */(d.has_local_tags ? toNames(d.local_tags) : srcTags));
  const [hasLocalTags, setHasLocalTags] = useState(d.has_local_tags ?? false);
  const [tagsStatus, setTagsStatus] = useState(/** @type {string|null} */(null));

  const canRefresh = hasPermission('library:refresh');
  const [pulling, setPulling] = useState(/** @type {string|null} */(null));

  /** @param {(v: string|null) => void} setter @param {string|null} v */
  function flash(setter, v) {
    setter(v);
    setTimeout(() => setter(null), 2500);
  }


  async function handleCoverFile(/** @type {Event} */ e) {
    const file = /** @type {HTMLInputElement} */(e.target).files?.[0];
    if (!file) return;
    try {
      await api.uploadMangaCover(dbId, file);
      setCoverOverridden(true);
      setCoverTs(Date.now());
      flash(setCoverStatus, 'saved');
      onFieldSaved({ cover_overridden: true });
    } catch (err) {
      showApiError(err);
    } finally {
      if (fileInputRef.current) fileInputRef.current.value = '';
    }
  }

  async function handleRemoveCover() {
    try {
      await api.clearMangaCoverOverride(dbId);
      setCoverOverridden(false);
      setCoverTs(Date.now());
      flash(setCoverStatus, 'saved');
      onFieldSaved({ cover_overridden: false });
    } catch (err) {
      showApiError(err);
    }
  }


  async function saveTitle() {
    const val = localName.trim() || null;
    const current = val ?? '';
    if (current === savedName.current) return;
    try {
      await api.updateLocalMetadata(dbId, { local_name: val });
      savedName.current = current;
      flash(setTitleStatus, 'saved');
      onFieldSaved({ local_name: val });
    } catch { flash(setTitleStatus, 'error'); }
  }

  async function resetTitle() {
    setLocalName('');
    savedName.current = '';
    try {
      await api.updateLocalMetadata(dbId, { local_name: null });
      flash(setTitleStatus, 'saved');
      onFieldSaved({ local_name: null });
    } catch { flash(setTitleStatus, 'error'); }
  }


  useEffect(() => {
    if (!descMounted.current) { descMounted.current = true; return; }
    if (descTimer.current) clearTimeout(descTimer.current);
    descTimer.current = setTimeout(async () => {
      const val = localDesc.trim() || null;
      setDescStatus('saving');
      try {
        await api.updateLocalMetadata(dbId, { local_description: val });
        flash(setDescStatus, 'saved');
        onFieldSaved({ local_description: val });
      } catch { flash(setDescStatus, 'error'); }
    }, 500);
    return () => { if (descTimer.current) clearTimeout(descTimer.current); };
  }, [localDesc]);

  async function resetDesc() {
    if (descTimer.current) clearTimeout(descTimer.current);
    setLocalDesc('');
    try {
      await api.updateLocalMetadata(dbId, { local_description: null });
      flash(setDescStatus, 'saved');
      onFieldSaved({ local_description: null });
    } catch { flash(setDescStatus, 'error'); }
  }


  async function handleStatusChange(/** @type {Event} */ e) {
    const raw = /** @type {HTMLSelectElement} */(e.target).value;
    setLocalStatus(raw);
    const val = raw === '' ? null : parseInt(raw, 10);
    try {
      await api.updateLocalMetadata(dbId, { local_status: val });
      flash(setStatusStatus, 'saved');
      onFieldSaved({ local_status: val });
    } catch { flash(setStatusStatus, 'error'); }
  }

  async function resetStatus() {
    setLocalStatus('');
    try {
      await api.updateLocalMetadata(dbId, { local_status: null });
      flash(setStatusStatus, 'saved');
      onFieldSaved({ local_status: null });
    } catch { flash(setStatusStatus, 'error'); }
  }


  async function savePeople(nextAuthors = authors, nextArtists = artists) {
    setAuthors(nextAuthors);
    setArtists(nextArtists);
    setHasLocalPeople(true);
    try {
      await api.updateLocalMetadata(dbId, { authors: nextAuthors, artists: nextArtists });
      flash(setPeopleStatus, 'saved');
      onFieldSaved({ local_authors: nextAuthors, local_artists: nextArtists, has_local_people: true });
    } catch { flash(setPeopleStatus, 'error'); }
  }

  async function resetPeople() {
    setAuthors(srcAuthors);
    setArtists(srcArtists);
    setHasLocalPeople(false);
    try {
      await api.updateLocalMetadata(dbId, { authors: null, artists: null });
      flash(setPeopleStatus, 'saved');
      onFieldSaved({ local_authors: [], local_artists: [], has_local_people: false });
    } catch { flash(setPeopleStatus, 'error'); }
  }


  async function saveTags(next) {
    setTags(next);
    setHasLocalTags(true);
    try {
      await api.updateLocalMetadata(dbId, { tags: next });
      flash(setTagsStatus, 'saved');
      onFieldSaved({ local_tags: next, has_local_tags: true });
    } catch { flash(setTagsStatus, 'error'); }
  }

  async function resetTags() {
    setTags(srcTags);
    setHasLocalTags(false);
    try {
      await api.updateLocalMetadata(dbId, { tags: null });
      flash(setTagsStatus, 'saved');
      onFieldSaved({ local_tags: [], has_local_tags: false });
    } catch { flash(setTagsStatus, 'error'); }
  }


  /** @param {string} fieldName */
  async function handlePullField(fieldName) {
    setPulling(fieldName);
    try {
      const { job_id } = await api.refreshManga(dbId, { fields: [fieldName], fetch_chapters: false, clear_overrides: true });
      if (job_id) {
        subscribeJob(job_id, {
          onComplete: async () => {
            const fresh = await api.getMangaDetails(dbId);
            onFieldSaved(fresh);
            if (fieldName === 'title') {
              setLocalName('');
              savedName.current = '';
            } else if (fieldName === 'description') {
              if (descTimer.current) clearTimeout(descTimer.current);
              setLocalDesc('');
            } else if (fieldName === 'status') {
              setLocalStatus('');
            } else if (fieldName === 'people') {
              setAuthors(toNames(fresh.source_authors?.length ? fresh.source_authors : fresh.authors));
              setArtists(toNames(fresh.source_artists?.length ? fresh.source_artists : fresh.artists));
              setHasLocalPeople(false);
            } else if (fieldName === 'tags') {
              setTags(toNames(fresh.source_tags?.length ? fresh.source_tags : fresh.tags));
              setHasLocalTags(false);
            } else if (fieldName === 'cover') {
              setCoverOverridden(false);
              setCoverTs(Date.now());
            }
            setPulling(null);
          },
          onFailed: (data) => {
            showApiError({ message: data?.message ?? t('manga.meta.refresh_failed') });
            setPulling(null);
          },
        });
      } else {
        setPulling(null);
      }
    } catch (e) {
      showApiError(e);
      setPulling(null);
    }
  }


  const titleOverridden = d.local_name != null || !!localName;
  const descOverridden = d.local_description != null || !!localDesc;
  const statusOverridden = localStatus !== '';

  const srcStatusLabel = STATUS_OPTIONS().find(o => String(o.value) === String(d.source_status))?.label;

  const descPlaceholder = d.source_description
    ? d.source_description.slice(0, 120) + (d.source_description.length > 120 ? '…' : '')
    : t('manga.meta.desc.placeholder');


  return html`
    <${Modal} open=${true} onClose=${onClose} title=${t('manga.meta.title')} wide=${true}
      footer=${html`<button type="button" class="btn-ghost btn-sm" onClick=${onClose}>${t('common.close')}</button>`}
    >
      <!-- Two columns at wide: the cover and status sit beside the text fields,
           which are what actually need the width. -->
      <div class="flex flex-col lg:flex-row gap-6 lg:gap-8">

        <div class="flex flex-col gap-6 lg:w-52 lg:shrink-0">
        <!-- Cover -->
        <div class="flex flex-col gap-2">
          <div class="flex items-center justify-between gap-2">
            <span class="text-sm font-semibold text-text">
              ${t('manga.meta.cover')}
              ${coverOverridden && html`<${OverrideDot} />`}
            </span>
            <div class="flex items-center gap-2">
              <${SaveStatus} status=${coverStatus} />
              ${canRefresh && html`
                <button type="button"
                  class="btn-ghost btn-sm text-xs text-text-muted"
                  disabled=${!!pulling} onClick=${() => handlePullField('cover')}
                  title=${t('manga.meta.pull.cover')}>
                  ${pulling === 'cover' ? '…' : t('manga.meta.pull')}
                </button>
              `}
            </div>
          </div>
          <div class="flex items-start gap-4 lg:flex-col">
            <img
              src=${`/rest/manga/${dbId}/cover?v=${coverTs}`}
              alt=${t('manga.meta.cover')}
              class="w-20 h-30 lg:w-full lg:h-auto lg:aspect-[2/3] object-cover rounded-lg border border-border flex-shrink-0 bg-surface-2"
              onError=${(/** @type {Event} */e) => { /** @type {HTMLImageElement} */(e.target).style.display = 'none'; }}
            />
            <div class="flex flex-col gap-2 lg:w-full">
              <label class="btn-ghost btn-sm cursor-pointer">
                ${t('manga.meta.choose_image')}
                <input type="file" accept="image/*" class="sr-only"
                  ref=${fileInputRef} onChange=${handleCoverFile} />
              </label>
              ${coverOverridden && html`
                <button type="button" class="btn-ghost btn-sm text-danger text-xs"
                  onClick=${handleRemoveCover}>${t('manga.meta.remove_cover')}</button>
              `}
            </div>
          </div>
        </div>

        <!-- Status -->
        <div class="flex flex-col gap-1.5">
          <div class="flex items-center justify-between gap-2">
            <span class="text-sm font-semibold text-text">
              ${t('manga.meta.field.status')}
              ${statusOverridden && html`<${OverrideDot} />`}
            </span>
            <div class="flex items-center gap-2 shrink-0">
              <${SaveStatus} status=${statusStatus} />
              ${statusOverridden ? html`
                <button type="button"
                  class="btn-ghost btn-sm flex items-center gap-1 text-xs text-text-muted"
                  onClick=${resetStatus}>
                  <span class="icon-xs" dangerouslySetInnerHTML=${{ __html: iconX }}></span>
                  ${t('manga.meta.restore')}
                </button>
              ` : null}
              ${canRefresh && statusOverridden && html`
                <button type="button"
                  class="btn-ghost btn-sm text-xs text-text-muted"
                  disabled=${!!pulling} onClick=${() => handlePullField('status')}
                  title=${t('manga.meta.pull.status')}>
                  ${pulling === 'status' ? '…' : t('manga.meta.pull')}
                </button>
              `}
            </div>
          </div>
          <select class="input w-full text-sm" value=${localStatus}
            disabled=${pulling === 'status'}
            onChange=${handleStatusChange}>
            ${STATUS_OPTIONS().map(opt => html`
              <option key=${opt.value} value=${String(opt.value)}>${opt.label}</option>
            `)}
          </select>
          ${statusOverridden && srcStatusLabel && html`
            <p class="text-xs text-text-muted">${t('manga.header.source')} ${srcStatusLabel}</p>
          `}
        </div>

        </div>

        <div class="flex flex-col gap-6 flex-1 min-w-0">
        <!-- Title -->
        <div class="flex flex-col gap-1.5">
          <div class="flex items-center justify-between gap-2">
            <label class="text-sm font-semibold text-text">
              ${t('manga.meta.field.title')}
              ${titleOverridden && html`<${OverrideDot} />`}
            </label>
            <div class="flex items-center gap-2">
              <${SaveStatus} status=${titleStatus} />
              ${titleOverridden ? html`
                <button type="button"
                  class="btn-ghost btn-sm flex items-center gap-1 text-xs text-text-muted"
                  onClick=${resetTitle}>
                  <span class="icon-xs" dangerouslySetInnerHTML=${{ __html: iconX }}></span>
                  ${t('manga.meta.restore')}
                </button>
              ` : null}
              ${canRefresh && titleOverridden && html`
                <button type="button"
                  class="btn-ghost btn-sm text-xs text-text-muted"
                  disabled=${!!pulling} onClick=${() => handlePullField('title')}
                  title=${t('manga.meta.pull.title')}>
                  ${pulling === 'title' ? '…' : t('manga.meta.pull')}
                </button>
              `}
            </div>
          </div>
          <input type="text" class="input w-full text-sm"
            value=${localName}
            placeholder=${d.source_name ?? ''}
            disabled=${pulling === 'title'}
            onInput=${(/** @type {Event} */e) => setLocalName(/** @type {HTMLInputElement} */(e.target).value)}
            onBlur=${saveTitle}
            onKeyDown=${(/** @type {KeyboardEvent} */e) => { if (e.key === 'Enter') /** @type {HTMLElement} */(e.target).blur(); }}
          />
          ${d.local_name != null && html`
            <p class="text-xs text-text-muted">${t('manga.header.source')} ${d.source_name}</p>
          `}
        </div>

        <!-- Description -->
        <div class="flex flex-col gap-1.5">
          <div class="flex items-center justify-between gap-2">
            <label class="text-sm font-semibold text-text">
              ${t('manga.meta.field.description')}
              ${descOverridden && html`<${OverrideDot} />`}
            </label>
            <div class="flex items-center gap-2">
              <${SaveStatus} status=${descStatus} />
              ${descOverridden ? html`
                <button type="button"
                  class="btn-ghost btn-sm flex items-center gap-1 text-xs text-text-muted"
                  onClick=${resetDesc}>
                  <span class="icon-xs" dangerouslySetInnerHTML=${{ __html: iconX }}></span>
                  ${t('manga.meta.restore')}
                </button>
              ` : null}
              ${canRefresh && descOverridden && html`
                <button type="button"
                  class="btn-ghost btn-sm text-xs text-text-muted"
                  disabled=${!!pulling} onClick=${() => handlePullField('description')}
                  title=${t('manga.meta.pull.description')}>
                  ${pulling === 'description' ? '…' : t('manga.meta.pull')}
                </button>
              `}
            </div>
          </div>
          <textarea class="input w-full text-sm resize-y" rows="4"
            placeholder=${descPlaceholder}
            value=${localDesc}
            disabled=${pulling === 'description'}
            onInput=${(/** @type {Event} */e) => setLocalDesc(/** @type {HTMLTextAreaElement} */(e.target).value)}
          ></textarea>
          ${descOverridden && d.source_description != null && html`
            <p class="text-xs text-text-muted">
              ${t('manga.header.source')} ${d.source_description.slice(0, 120)}${d.source_description.length > 120 ? '…' : ''}
            </p>
          `}
        </div>

        <!-- Authors & Artists -->
        <div class="flex flex-col gap-4">
          <div class="flex items-center justify-between gap-2">
            <span class="text-sm font-semibold text-text">
              ${t('manga.meta.field.people')}
              ${hasLocalPeople && html`<${OverrideDot} />`}
            </span>
            <div class="flex items-center gap-2">
              <${SaveStatus} status=${peopleStatus} />
              ${hasLocalPeople && html`
                <button type="button"
                  class="btn-ghost btn-sm flex items-center gap-1 text-xs text-text-muted"
                  onClick=${resetPeople}>
                  <span class="icon-xs" dangerouslySetInnerHTML=${{ __html: iconX }}></span>
                  ${t('manga.meta.restore')}
                </button>
              `}
              ${canRefresh && hasLocalPeople && html`
                <button type="button"
                  class="btn-ghost btn-sm text-xs text-text-muted"
                  disabled=${!!pulling} onClick=${() => handlePullField('people')}
                  title=${t('manga.meta.pull.people')}>
                  ${pulling === 'people' ? '…' : t('manga.meta.pull')}
                </button>
              `}
            </div>
          </div>
          <div class="flex flex-col gap-1.5">
            <label class="text-xs text-text-muted">${t('manga.meta.field.authors')}</label>
            <${Combobox}
              options=${authorOptions}
              value=${authors}
              onChange=${(items) => savePeople(items, artists)}
              placeholder=${t('manga.meta.add_author')}
              creatable=${true}
            />
          </div>
          <div class="flex flex-col gap-1.5">
            <label class="text-xs text-text-muted">${t('manga.meta.field.artists')}</label>
            <${Combobox}
              options=${artistOptions}
              value=${artists}
              onChange=${(items) => savePeople(authors, items)}
              placeholder=${t('manga.meta.add_artist')}
              creatable=${true}
            />
          </div>
        </div>

        <!-- Tags -->
        <div class="flex flex-col gap-2">
          <div class="flex items-center justify-between gap-2">
            <span class="text-sm font-semibold text-text">
              ${t('manga.meta.field.tags')}
              ${hasLocalTags && html`<${OverrideDot} />`}
            </span>
            <div class="flex items-center gap-2">
              <${SaveStatus} status=${tagsStatus} />
              ${hasLocalTags && html`
                <button type="button"
                  class="btn-ghost btn-sm flex items-center gap-1 text-xs text-text-muted"
                  onClick=${resetTags}>
                  <span class="icon-xs" dangerouslySetInnerHTML=${{ __html: iconX }}></span>
                  ${t('manga.meta.restore')}
                </button>
              `}
              ${canRefresh && hasLocalTags && html`
                <button type="button"
                  class="btn-ghost btn-sm text-xs text-text-muted"
                  disabled=${!!pulling} onClick=${() => handlePullField('tags')}
                  title=${t('manga.meta.pull.tags')}>
                  ${pulling === 'tags' ? '…' : t('manga.meta.pull')}
                </button>
              `}
            </div>
          </div>
          <${Combobox}
            options=${tagOptions}
            value=${tags}
            onChange=${saveTags}
            placeholder=${t('manga.meta.add_tag')}
            creatable=${true}
          />
        </div>

        </div>

      </div>
    </${Modal}>
  `;
}


/** @type {any} */
let _liveData = null;


/**
 * @param {{ dbId: number, onRefreshed: (fresh: any) => void }} props
 */
function RefreshRow({ dbId, onRefreshed }) {
  const [redownloadCover, setRedownloadCover] = useState(() => getLocal('kani.refreshOpts.cover') !== 'false');
  const [status, setStatus] = useState(/** @type {string|null} */(null));
  const isRefreshing = status === 'refreshing';

  /** @param {string|null} v */
  function flash(v) {
    setStatus(v);
    setTimeout(() => setStatus(null), 2500);
  }

  async function handleRefresh() {
    flash('refreshing');
    const fields = redownloadCover ? undefined : ['title', 'description', 'status', 'people', 'tags'];
    try {
      const { job_id } = await api.refreshManga(dbId, { fields, fetch_chapters: false });
      if (!job_id) {
        flash('error');
        return;
      }
      subscribeJob(job_id, {
        onComplete: async () => {
          const fresh = await api.getMangaDetails(dbId);
          onRefreshed(fresh);
          flash('saved');
        },
        onFailed: (/** @type {any} */ data) => {
          flash('error');
          showApiError({ message: data?.message ?? t('manga.meta.refresh_failed') });
        },
      });
    } catch (e) {
      flash('error');
      showApiError(e);
    }
  }

  return html`
    <div class="flex items-center gap-3">
      <${SaveStatus} status=${isRefreshing ? null : status} />
      <label class="flex items-center gap-1.5 cursor-pointer select-none text-xs text-text-muted">
        <input type="checkbox" class="accent-accent" checked=${redownloadCover}
          onChange=${(/** @type {Event} */e) => {
            const v = /** @type {HTMLInputElement} */(e.target).checked;
            setRedownloadCover(v);
            setLocal('kani.refreshOpts.cover', String(v));
          }} />
        ${t('manga.meta.redownload_cover')}
      </label>
      <button type="button"
        class="btn-secondary btn-sm flex items-center gap-1"
        disabled=${isRefreshing}
        onClick=${handleRefresh}>
        <span class=${'icon-xs' + (isRefreshing ? ' icon-spin' : '')}
          dangerouslySetInnerHTML=${{ __html: iconRefresh }}></span>
        ${isRefreshing ? t('manga.meta.refreshing') : t('manga.meta.refresh')}
      </button>
    </div>
  `;
}

/**
 * Creates a compact "Edit metadata" row in the Manage tab that opens the modal on click.
 * @param {HTMLElement} containerEl
 * @param {{ dbId: number, mangaData: any }} ctx
 */
export function mountMetadataPanel(containerEl, { dbId, mangaData }) {
  _liveData = { ...mangaData };

  const card = mkCard();

  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = 'btn-ghost btn-sm flex items-center gap-2';
  btn.innerHTML = `<span class="icon-sm" style="display:inline-flex">${iconPencil}</span> ${t('manga.meta.edit')}`;

  btn.addEventListener('click', () => {
    let cleanup = /** @type {() => void} */(() => {});
    cleanup = mountIntoModalRoot(html`
      <${MetadataEditModal}
        dbId=${dbId}
        initialData=${{ ..._liveData }}
        onFieldSaved=${(/** @type {any} */patch) => { Object.assign(_liveData, patch); }}
        onClose=${() => cleanup()}
      />
    `);
  });

  card.appendChild(mkItem(mkRow(
    t('manga.meta.row_label'),
    t('manga.meta.row_desc'),
    btn,
  )));

  if (hasPermission('library:refresh')) {
    const refreshHost = document.createElement('div');
    render(
      html`<${RefreshRow}
        dbId=${dbId}
        onRefreshed=${(/** @type {any} */ fresh) => { Object.assign(_liveData, fresh); }}
      />`,
      refreshHost,
    );
    card.appendChild(mkItem(mkRow(
      t('manga.meta.refresh_source'),
      t('manga.meta.refresh_tip'),
      refreshHost,
    )));
  }

  containerEl.appendChild(card);
}
