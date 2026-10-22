// @ts-check
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { chunk, jobErrorMessage, planBatch } from './bulk-migration.js';

/** @param {number} mangaId @param {Array<[string, number, boolean?]>} candidates @param {string | null} best */
const match = (mangaId, candidates, best, error = null) => ({
  manga_id: mangaId,
  title: `Title ${mangaId}`,
  candidates: candidates.map(([id, score, inLibrary = false]) => ({ id, title: id, cover_url: null, score, in_library: inLibrary })),
  best,
  error,
});

test('chunk: splits into fixed-size runs with a short tail', () => {
  assert.deepEqual(chunk([1, 2, 3, 4, 5], 2), [[1, 2], [3, 4], [5]]);
  assert.deepEqual(chunk([], 3), []);
});

test('planBatch: the proposed match is used until the user picks another', () => {
  const matches = new Map([[1, match(1, [['a', 0.97], ['b', 0.6]], 'a')]]);
  assert.deepEqual(planBatch([1], matches, new Map()).items, [{ manga_id: 1, target_source_manga_id: 'a' }]);
  assert.deepEqual(planBatch([1], matches, new Map([[1, 'b']])).items, [{ manga_id: 1, target_source_manga_id: 'b' }]);
  assert.equal(planBatch([1], matches, new Map([[1, 'b']])).statuses.get(1), 'review');
});

test('planBatch: a second title aimed at the same series is held back', () => {
  const matches = new Map([
    [1, match(1, [['a', 0.95]], 'a')],
    [2, match(2, [['a', 0.93]], 'a')],
  ]);
  const plan = planBatch([1, 2], matches, new Map());
  assert.deepEqual(plan.items, [{ manga_id: 1, target_source_manga_id: 'a' }]);
  assert.equal(plan.statuses.get(2), 'duplicate');
});

test('planBatch: skipped, unmatched, failed, pending and in-library rows are never sent', () => {
  const matches = new Map([
    [1, match(1, [['a', 0.95]], 'a')],
    [2, match(2, [['b', 0.4]], null)],
    [3, match(3, [], null)],
    [4, match(4, [], null, 'Search failed')],
    [6, match(6, [['c', 0.99, true]], null)],
    [7, match(7, [['d', 1, true]], null)],
  ]);
  const plan = planBatch([1, 2, 3, 4, 5, 6, 7], matches, new Map([[1, ''], [6, 'c']]));
  assert.deepEqual(plan.items, []);
  assert.deepEqual([...plan.statuses.entries()], [
    [1, 'skipped'], [2, 'unsure'], [3, 'none'], [4, 'error'], [5, 'searching'], [6, 'in_library'], [7, 'in_library'],
  ]);
});

test('jobErrorMessage: unwraps the stored job error to the reason a person can act on', () => {
  assert.equal(
    jobErrorMessage({ Internal: 'Validation error: The target source matches none of this series.' }),
    'The target source matches none of this series.',
  );
  assert.equal(jobErrorMessage({ Internal: 'Conflict: Target manga is already in your library' }), 'Target manga is already in your library');
  assert.equal(jobErrorMessage('Cancelled'), 'Cancelled');
  assert.equal(jobErrorMessage(null), null);
});
