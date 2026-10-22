
import { signal } from '@preact/signals';
import { useEffect } from 'preact/hooks';

/** True when the active section has unsaved changes. Read reactively by the host. */
export const formDirty = signal(false);

/** @type {null | (() => Promise<void>)} */
let _save = null;
/** @type {null | (() => void)} */
let _reset = null;

/** @param {{ dirty: boolean, save: () => Promise<void>, reset: () => void }} handle */
function registerForm({ dirty, save, reset }) {
  formDirty.value = dirty;
  _save = save;
  _reset = reset;
}

/** Clears the active-section registration (called when a section unmounts). */
function clearForm() {
  formDirty.value = false;
  _save = null;
  _reset = null;
}

/** Persist the active section's changes. No-op if nothing is registered. */
export async function runSave() {
  if (_save) await _save();
}

/** Revert the active section's changes to the last saved snapshot. */
export function runReset() {
  if (_reset) _reset();
}

/**
 * Registers a section's form with the save bar. Dirty is a JSON diff of the
 * controlled state (`current`) against the last saved snapshot (`saved`).
 * Re-registers every render so `save`/`reset` capture fresh state; clears on unmount.
 * @param {{ current: any, saved: any, save: () => Promise<void>, reset: () => void }} opts
 */
export function useSettingsForm({ current, saved, save, reset }) {
  const dirty = JSON.stringify(current) !== JSON.stringify(saved);
  useEffect(() => {
    registerForm({ dirty, save, reset });
  });
  useEffect(() => () => clearForm(), []);
}
