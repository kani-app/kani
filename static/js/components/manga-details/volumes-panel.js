// @ts-check

import * as api from '../../api.js';
import { t } from '../../i18n.js';
import { showApiError } from '../toast.js';
import { showConfirm } from '../modal.js';
import { mkCard } from './_shared.js';
import { mkEditableRow } from '../editable-row.js';
import { createEmptyState } from '../empty-state.js';

/** @param {HTMLElement} section @param {number} mangaId */
export async function mountVolumesPanel(section, mangaId) {
  const card = mkCard();
  section.appendChild(card);

  const head = document.createElement('div');
  head.className = 'detail-card-head';
  head.innerHTML = `<span>${t('manga.details.volumes.loading')}</span>`;

  const addBtn = document.createElement('button');
  addBtn.type = 'button';
  addBtn.className = 'btn-secondary btn-sm';
  addBtn.textContent = t('manga.details.volumes.add');
  head.appendChild(addBtn);
  card.appendChild(head);

  const list = document.createElement('div');
  list.className = 'divide-y divide-border-subtle';
  card.appendChild(list);

  async function _load() {
    list.innerHTML = `<p class="px-4 py-3 text-sm text-text-muted">${t('manga.details.volumes.loading')}</p>`;
    try {
      const volumes = await api.listVolumes(mangaId);
      head.querySelector('span').textContent = t('manga.details.volumes.count', { count: volumes.length, s: volumes.length === 1 ? '' : 's' });
      list.innerHTML = '';
      if (volumes.length === 0) {
        list.appendChild(createEmptyState({ title: t('manga.details.volumes.empty'), compact: true }));
      } else {
        for (const v of volumes) list.appendChild(_mkVolumeRow(v, mangaId, _load));
      }
    } catch (e) {
      list.innerHTML = `<p class="px-4 py-3 text-sm text-danger">${e.message ?? t('manga.details.volumes.error')}</p>`;
    }
  }

  const addForm = document.createElement('div');
  addForm.className = 'hidden items-center gap-2 px-4 py-2 border-b border-border-subtle';
  addForm.innerHTML = `
    <input type="text" placeholder="${t('manga.details.volumes.name_placeholder')}" class="input text-sm flex-1" />
    <input type="number" placeholder="${t('manga.details.volumes.num_placeholder')}" class="input text-sm w-20" min="0" step="1" />
    <button type="button" class="btn-primary btn-sm">${t('common.add')}</button>
    <button type="button" class="btn-ghost btn-sm">${t('common.cancel')}</button>
  `;
  card.appendChild(addForm);

  const addNameInput = /** @type {HTMLInputElement} */ (addForm.querySelector('input[type="text"]'));
  const addNumInput  = /** @type {HTMLInputElement} */ (addForm.querySelector('input[type="number"]'));
  const addSubmitBtn = /** @type {HTMLButtonElement} */ (addForm.querySelector('.btn-primary'));
  const addCancelBtn = /** @type {HTMLButtonElement} */ (addForm.querySelector('.btn-ghost'));

  addBtn.addEventListener('click', () => {
    addBtn.classList.add('hidden');
    addForm.classList.remove('hidden');
    addForm.classList.add('flex');
    addNameInput.focus();
  });

  addCancelBtn.addEventListener('click', () => {
    addForm.classList.add('hidden');
    addForm.classList.remove('flex');
    addBtn.classList.remove('hidden');
    addNameInput.value = '';
    addNumInput.value = '';
  });

  addSubmitBtn.addEventListener('click', async () => {
    addSubmitBtn.disabled = true;
    try {
      const volume_num = addNumInput.value ? Number(addNumInput.value) : undefined;
      await api.createVolume(mangaId, { name: addNameInput.value.trim() || undefined, volume_num });
      addForm.classList.add('hidden');
      addForm.classList.remove('flex');
      addBtn.classList.remove('hidden');
      addNameInput.value = '';
      addNumInput.value = '';
      await _load();
    } catch (e) {
      showApiError(e);
    } finally {
      addSubmitBtn.disabled = false;
    }
  });

  await _load();
}

/**
 * @param {any} volume
 * @param {number} mangaId
 * @param {() => Promise<void>} reload
 */
function _mkVolumeRow(volume, mangaId, reload) {
  return mkEditableRow({
    editLabel: t('manga.details.volumes.rename'),
    deleteLabel: t('manga.details.volumes.delete'),
    renderView: () => {
      const label = document.createElement('span');
      label.className = 'flex-1 text-sm text-text truncate';
      label.textContent = volume.name
        ? (volume.volume_num != null ? t('manga.details.volumes.label.numbered_named', { num: volume.volume_num, name: volume.name }) : volume.name)
        : (volume.volume_num != null ? t('manga.details.volumes.label.numbered', { num: volume.volume_num }) : t('manga.details.volumes.label.fallback', { id: volume.id }));
      return label;
    },
    renderForm: () => {
      const nameInput = document.createElement('input');
      nameInput.type = 'text';
      nameInput.className = 'input text-sm flex-1';
      nameInput.value = volume.name ?? '';
      nameInput.placeholder = t('manga.details.volumes.edit_name_placeholder');

      const numInput = document.createElement('input');
      numInput.type = 'number';
      numInput.className = 'input text-sm w-20';
      numInput.value = volume.volume_num != null ? String(volume.volume_num) : '';
      numInput.placeholder = t('manga.details.volumes.num_placeholder');
      numInput.min = '0';
      numInput.step = '1';

      const wrap = document.createElement('div');
      wrap.className = 'flex items-center gap-2';
      wrap.append(nameInput, numInput);

      return {
        el: wrap,
        focusEl: nameInput,
        getValue: () => ({
          name: nameInput.value.trim() || undefined,
          volume_num: numInput.value ? Number(numInput.value) : undefined,
        }),
      };
    },
    onSave: async ({ name, volume_num }) => {
      await api.updateVolume(mangaId, volume.id, { name, volume_num });
      await reload();
    },
    onDelete: async () => {
      const ok = await showConfirm(t('manga.details.volumes.delete.body'), { title: t('manga.details.volumes.delete'), confirmLabel: t('common.delete') });
      if (!ok) return;
      await api.deleteVolume(mangaId, volume.id);
      await reload();
    },
  });
}
