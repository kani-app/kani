// @ts-check
// Editable row — view content with edit/delete icon buttons, toggling to an inline
// form (save/cancel) below. Caller owns all API calls and DOM content; onSave/onDelete
// are expected to trigger the caller's own list reload on success.

import { t } from '../i18n.js';
import { iconPencil, iconPlus, iconTrash } from '../icons.js';
import { showApiError } from './toast.js';

/**
 * @param {{
 *   renderView: () => HTMLElement,
 *   renderForm: () => {
 *     el: HTMLElement,
 *     getValue: () => any,
 *     focusEl?: HTMLElement,
 *     validate?: () => boolean,
 *     reset?: () => void,
 *   },
 *   onSave: (value: any) => Promise<void>,
 *   onDelete?: () => Promise<void>,
 *   canEdit?: boolean,
 *   editLabel?: string,
 *   deleteLabel?: string,
 * }} opts
 * @returns {HTMLElement}
 */
export function mkEditableRow({ renderView, renderForm, onSave, onDelete, canEdit = true, editLabel, deleteLabel }) {
  const row = document.createElement('div');
  row.className = 'px-4 py-3';

  const viewRow = document.createElement('div');
  viewRow.className = 'flex items-center gap-3';
  viewRow.appendChild(renderView());

  const editBtn = document.createElement('button');
  editBtn.type = 'button';
  editBtn.className = 'btn-icon text-text-muted shrink-0' + (canEdit ? '' : ' hidden');
  editBtn.setAttribute('aria-label', editLabel ?? t('common.edit'));
  editBtn.innerHTML = iconPencil;
  viewRow.appendChild(editBtn);

  let delBtn = null;
  if (onDelete) {
    delBtn = document.createElement('button');
    delBtn.type = 'button';
    delBtn.className = 'btn-icon text-danger shrink-0';
    delBtn.setAttribute('aria-label', deleteLabel ?? t('common.delete'));
    delBtn.innerHTML = iconTrash;
    viewRow.appendChild(delBtn);
  }

  row.appendChild(viewRow);

  const { el: formEl, getValue, focusEl, validate, reset } = renderForm();

  const editForm = document.createElement('div');
  editForm.className = 'hidden flex-col gap-2 mt-3';

  const editActions = document.createElement('div');
  editActions.className = 'flex gap-2 justify-end';

  const cancelBtn = document.createElement('button');
  cancelBtn.type = 'button';
  cancelBtn.className = 'btn-ghost btn-sm';
  cancelBtn.textContent = t('common.cancel');

  const saveBtn = document.createElement('button');
  saveBtn.type = 'button';
  saveBtn.className = 'btn-primary btn-sm';
  saveBtn.textContent = t('common.save');

  editActions.append(cancelBtn, saveBtn);
  editForm.append(formEl, editActions);
  row.appendChild(editForm);

  const _showEdit = () => {
    editForm.classList.remove('hidden');
    editForm.classList.add('flex');
    editBtn.classList.add('hidden');
    delBtn?.classList.add('hidden');
    focusEl?.focus();
  };

  const _hideEdit = () => {
    editForm.classList.add('hidden');
    editForm.classList.remove('flex');
    editBtn.classList.remove('hidden');
    delBtn?.classList.remove('hidden');
    reset?.();
  };

  editBtn.addEventListener('click', _showEdit);
  cancelBtn.addEventListener('click', _hideEdit);

  saveBtn.addEventListener('click', async () => {
    if (validate && !validate()) return;
    saveBtn.disabled = true;
    try {
      await onSave(getValue());
    } catch (e) {
      showApiError(e);
    } finally {
      saveBtn.disabled = false;
    }
  });

  if (delBtn) {
    delBtn.addEventListener('click', async () => {
      delBtn.disabled = true;
      try {
        await onDelete();
      } catch (e) {
        showApiError(e);
      } finally {
        delBtn.disabled = false;
      }
    });
  }

  return row;
}

