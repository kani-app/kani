// @ts-check
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { countTracks, resolvedColumns, rowsThatFit } from './grid-columns.js';

test('countTracks: counts resolved track lists', () => {
  assert.equal(countTracks('184px 184px 184px'), 3);
  assert.equal(countTracks('  200px   200px  '), 2);
  assert.equal(countTracks('1fr'), 1);
});

test('countTracks: treats a grouped function as one track', () => {
  assert.equal(countTracks('minmax(140px, 1fr) minmax(140px, 1fr)'), 2);
});

test('countTracks: reports nothing for a grid with no explicit tracks', () => {
  assert.equal(countTracks('none'), 0);
  assert.equal(countTracks(''), 0);
});

test('resolvedColumns: an unlaid grid reports its specified repeat() as unknown', () => {
  assert.equal(resolvedColumns('repeat(auto-fill, minmax(180px, 1fr))'), 0);
  assert.equal(resolvedColumns('none'), 0);
  assert.equal(resolvedColumns(''), 0);
});

test('resolvedColumns: a laid-out grid reports its track count', () => {
  assert.equal(resolvedColumns('184px 184px 184px'), 3);
  assert.equal(resolvedColumns('210px'), 1);
});

test('rowsThatFit: a row needs the gap above it as well as its own height', () => {
  // Two 270px rows need 270+16+270 = 556. One pixel short is one row.
  assert.equal(rowsThatFit(556, 270, 16), 2);
  assert.equal(rowsThatFit(555, 270, 16), 1);
});

test('rowsThatFit: space left over does not round up into a row', () => {
  assert.equal(rowsThatFit(700, 270, 16), 2);
  assert.equal(rowsThatFit(841, 270, 16), 2);
  assert.equal(rowsThatFit(842, 270, 16), 3);
});

test('rowsThatFit: a viewport shorter than one tile still shows a row', () => {
  assert.equal(rowsThatFit(100, 270, 16), 1);
});

test('rowsThatFit: unmeasurable inputs report unknown rather than one', () => {
  assert.equal(rowsThatFit(0, 270, 16), 0);
  assert.equal(rowsThatFit(600, 0, 16), 0);
  assert.equal(rowsThatFit(-1, 270, 16), 0);
});
