// @ts-check
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { prefersInfiniteScroll } from './pagination-mode.js';

/** @param {boolean} phone @param {Record<string, string>} stored */
function stub(phone, stored) {
  Object.assign(globalThis, {
    matchMedia: (/** @type {string} */ q) => ({ matches: phone && q.includes('max-width') }),
    localStorage: { getItem: (/** @type {string} */ k) => stored[k] ?? null },
  });
}

test('prefersInfiniteScroll: a phone scrolls infinitely whatever the stored preference', () => {
  stub(true, { kani_library_pagination: 'paginated' });
  assert.equal(prefersInfiniteScroll('kani_library_pagination'), true);
  stub(true, {});
  assert.equal(prefersInfiniteScroll('kani_library_pagination'), true);
});

test('prefersInfiniteScroll: a wider screen follows the stored preference', () => {
  stub(false, { kani_library_pagination: 'infinite' });
  assert.equal(prefersInfiniteScroll('kani_library_pagination'), true);
  stub(false, { kani_library_pagination: 'paginated' });
  assert.equal(prefersInfiniteScroll('kani_library_pagination'), false);
  stub(false, {});
  assert.equal(prefersInfiniteScroll('kani_library_pagination'), false);
});
