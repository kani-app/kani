// @ts-check
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { clampPan, zoomStep, ZOOM_MAX } from './zoom.js';

test('clampPan: within bounds unchanged, beyond bounds clamped', () => {
  assert.deepEqual(clampPan(100, 100, 2, -50, -50), { tx: -50, ty: -50 });
  assert.deepEqual(clampPan(100, 100, 2, -200, -200), { tx: -100, ty: -100 });
  assert.deepEqual(clampPan(100, 100, 2, 50, 50), { tx: 0, ty: 0 });
});

test('zoomStep: zoom out to <=1 snaps to identity', () => {
  assert.deepEqual(zoomStep({ scale: 1.5, tx: -20, ty: -30 }, 0.5, 40, 40), { scale: 1, tx: 0, ty: 0 });
  assert.deepEqual(zoomStep({ scale: 1, tx: 0, ty: 0 }, 0.5, 40, 40), { scale: 1, tx: 0, ty: 0 });
});

test('zoomStep: zoom in keeps the focal point stationary', () => {
  const r = zoomStep({ scale: 1, tx: 0, ty: 0 }, 2, 50, 50, { viewportW: 100, viewportH: 100 });
  assert.equal(r.scale, 2);
  assert.equal(r.tx, -50);
  assert.equal(r.ty, -50);
});

test('zoomStep: clamps to ZOOM_MAX', () => {
  const r = zoomStep({ scale: 2.8, tx: 0, ty: 0 }, 2, 0, 0, { viewportW: 100, viewportH: 100 });
  assert.equal(r.scale, ZOOM_MAX);
});

test('zoomStep: pan result is clamped to bounds', () => {
  const r = zoomStep({ scale: 1, tx: 0, ty: 0 }, 2, 0, 0, { viewportW: 100, viewportH: 100 });
  assert.equal(r.scale, 2);
  assert.equal(r.tx, 0);
  assert.equal(r.ty, 0);
});
