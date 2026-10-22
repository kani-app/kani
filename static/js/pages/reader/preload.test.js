// @ts-check
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { pagesPerMinute, minutesRemaining, adaptivePreloadCount, preloadThreshold } from './preload.js';

test('pagesPerMinute', () => {
  assert.equal(pagesPerMinute([]), null);
  assert.equal(pagesPerMinute([1000]), null);
  assert.equal(pagesPerMinute([0, 30000, 60000]), 2);
  assert.equal(pagesPerMinute([5000, 5000]), null);
});

test('minutesRemaining', () => {
  assert.equal(minutesRemaining(null, 10), null);
  assert.equal(minutesRemaining(2, 0), 0);
  assert.equal(minutesRemaining(2, -3), 0);
  assert.equal(minutesRemaining(2, 10), 5);
});

test('adaptivePreloadCount: falls back to max without data', () => {
  assert.equal(adaptivePreloadCount({ max: 4, fetchMsLog: [], ppm: 2 }), 4);
  assert.equal(adaptivePreloadCount({ max: 4, fetchMsLog: [100, 100], ppm: 2 }), 4);
  assert.equal(adaptivePreloadCount({ max: 4, fetchMsLog: [100, 100, 100], ppm: null }), 4);
});

test('adaptivePreloadCount: computes clamped count', () => {
  assert.equal(adaptivePreloadCount({ max: 4, fetchMsLog: [100, 100, 100], ppm: 2 }), 4);
  assert.equal(adaptivePreloadCount({ max: 4, fetchMsLog: [20000, 20000, 20000], ppm: 2 }), 1);
  assert.equal(adaptivePreloadCount({ max: 4, fetchMsLog: [90000, 90000, 90000], ppm: 2 }), 1);
});

test('preloadThreshold', () => {
  assert.equal(preloadThreshold('paged', 20), 17);
  assert.equal(preloadThreshold('scroll', 20), 16);
  assert.equal(preloadThreshold('continuous-paged', 20), 16);
});
