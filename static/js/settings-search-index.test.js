// @ts-check
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildSettingsSearchIndex } from './settings-search-index.js';

const keys = (/** @type {Map<string, Array<{ key: string }>>} */ idx) => (idx.get('general') ?? []).map(r => r.key);

test('buildSettingsSearchIndex: rows under an excluded prefix are not searchable', () => {
  const all = keys(buildSettingsSearchIndex([{ id: 'general' }]));
  assert.ok(all.some(k => k.startsWith('settings.general.pagination.')), 'fixture: pagination rows are indexed by default');

  const phone = keys(buildSettingsSearchIndex([{ id: 'general' }], { exclude: ['settings.general.pagination.'] }));
  assert.equal(phone.some(k => k.startsWith('settings.general.pagination.')), false);
  assert.ok(phone.length > 0 && phone.length < all.length, 'only the excluded rows drop out');
});
