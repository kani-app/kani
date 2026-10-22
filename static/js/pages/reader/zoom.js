// @ts-check
// Pure zoom/pan math for the reader (paged mode). No DOM: the caller maps
// pointer coordinates into content space and applies the returned transform.

/** @typedef {{ scale: number, tx: number, ty: number }} ZoomState */

export const ZOOM_MIN = 1;
export const ZOOM_MAX = 5;

/**
 * Clamp the pan offset so the scaled content can't be dragged past its edges.
 * At scale s the content is `viewport * s` wide; the smallest (most negative)
 * offset is `viewport * (1 - s)`, the largest is 0.
 * @returns {{ tx: number, ty: number }}
 */
export function clampPan(viewportW, viewportH, scale, tx, ty) {
  const minTx = Math.min(0, viewportW * (1 - scale));
  const minTy = Math.min(0, viewportH * (1 - scale));
  return {
    tx: Math.max(minTx, Math.min(0, tx)),
    ty: Math.max(minTy, Math.min(0, ty)),
  };
}

/**
 * Apply a zoom factor about a focal point (`cx`,`cy` in content space, i.e.
 * relative to the untranslated content origin) and return the new transform.
 * Zooming to ≤1 snaps back to the identity transform.
 *
 * @param {ZoomState} state
 * @param {number} factor — multiplier (>1 zoom in, <1 out)
 * @param {number} cx — focal x in content space
 * @param {number} cy — focal y in content space
 * @param {{ min?: number, max?: number, viewportW?: number, viewportH?: number }} [opts]
 * @returns {ZoomState}
 */
export function zoomStep({ scale, tx, ty }, factor, cx, cy, { min = ZOOM_MIN, max = ZOOM_MAX, viewportW = 0, viewportH = 0 } = {}) {
  const prev = scale;
  const next = Math.max(min, Math.min(max, scale * factor));
  if (next <= 1) return { scale: 1, tx: 0, ty: 0 };
  const ntx = cx - (next / prev) * (cx - tx);
  const nty = cy - (next / prev) * (cy - ty);
  const clamped = clampPan(viewportW, viewportH, next, ntx, nty);
  return { scale: next, tx: clamped.tx, ty: clamped.ty };
}
