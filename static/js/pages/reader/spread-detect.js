// @ts-check
// Pure spread-detection math for the reader. No DOM, no closure state — all
// inputs are passed in. Extracted from reader.js so the ratio gating and the
// edge-match pixel comparison can be unit-tested.

/** @typedef {{ w: number, h: number }} Dims */

/** Landscape threshold: width/height at or above this reads as a wide spread. */
const WIDE_RATIO = 1.2;

/**
 * True if a page is a pre-combined wide spread (landscape). When the server has
 * analysed the chapter, only pages it flagged as doubles are considered.
 * @param {Dims | undefined | null} dims
 * @param {{ hasServerAnalysis?: boolean, isServerDouble?: boolean }} [opts]
 */
export function isWideImage(dims, { hasServerAnalysis = false, isServerDouble = false } = {}) {
  if (!dims) return false;
  if (hasServerAnalysis && !isServerDouble) return false;
  return dims.w / dims.h >= WIDE_RATIO;
}

/**
 * Verdict for whether pages A and A+1 are two halves of one original spread and
 * should be composited. Ratio gating only — the caller handles the async
 * edge-match sampling when the result is 'needs-edge-check'.
 *
 * `edgeMatch` reflects the cached result of that sampling:
 *   true  → confirmed match, false → confirmed not, null → check in flight,
 *   undefined → never checked.
 *
 * @param {Dims | undefined | null} a
 * @param {Dims | undefined | null} b
 * @param {{ hasServerAnalysis?: boolean, isServerDoubleA?: boolean, edgeMatch?: boolean | null }} [opts]
 * @returns {'pair' | 'not-pair' | 'pending' | 'needs-edge-check'}
 */
export function spreadPairVerdict(a, b, { hasServerAnalysis = false, isServerDoubleA = false, edgeMatch } = {}) {
  if (!a || !b) return 'not-pair';

  if (hasServerAnalysis) {
    if (!isServerDoubleA) return 'not-pair';
    if (a.w / a.h >= WIDE_RATIO) return 'not-pair';
    return 'pair';
  }

  if (a.w >= a.h * 0.95 || b.w >= b.h * 0.95) return 'not-pair';
  const ratio = (a.w + b.w) / Math.max(a.h, b.h);
  if (ratio < WIDE_RATIO || ratio > 2.5) return 'not-pair';

  if (edgeMatch === true) return 'pair';
  if (edgeMatch === false) return 'not-pair';
  if (edgeMatch === null) return 'pending';
  return 'needs-edge-check';
}

/** Rec. 601 luma of an 8-bit RGB pixel. */
function luma(r, g, b) {
  return (r * 299 + g * 587 + b * 114) / 1000;
}

/**
 * Variance of luma across an RGBA pixel buffer. A near-flat strip (solid colour
 * gutter) has low variance and must not be treated as a content match.
 * @param {ArrayLike<number>} data — RGBA, 4 bytes per pixel
 */
export function lumaVariance(data) {
  let sum = 0, sumSq = 0, n = 0;
  for (let i = 0; i < data.length; i += 4) {
    const l = luma(data[i], data[i + 1], data[i + 2]);
    sum += l; sumSq += l * l; n++;
  }
  if (n === 0) return 0;
  const mean = sum / n;
  return (sumSq / n) - (mean * mean);
}

/**
 * Compare the adjoining edges of two sampled strips (page A's right edge vs page
 * B's left edge, already drawn into equal STRIP_W × SAMPLE_H buffers). Returns
 * whether they match closely enough to be one scan split in two.
 *
 * `pxA` columns are mirrored against `pxB` (x ↔ STRIP_W-1-x) because they meet at
 * the seam.
 *
 * @param {ArrayLike<number>} pxA — RGBA buffer, STRIP_W × SAMPLE_H
 * @param {ArrayLike<number>} pxB — RGBA buffer, STRIP_W × SAMPLE_H
 * @param {{ stripW: number, sampleH: number, minVariance?: number, maxAvgDiff?: number }} opts
 * @returns {{ flat: boolean, avgDiff: number | null, isMatch: boolean }}
 */
export function edgeMatchResult(pxA, pxB, { stripW, sampleH, minVariance = 200, maxAvgDiff = 20 }) {
  if (lumaVariance(pxA) < minVariance || lumaVariance(pxB) < minVariance) {
    return { flat: true, avgDiff: null, isMatch: false };
  }
  let diff = 0;
  for (let y = 0; y < sampleH; y++) {
    for (let x = 0; x < stripW; x++) {
      const iA = (y * stripW + (stripW - 1 - x)) * 4;
      const iB = (y * stripW + x) * 4;
      diff += Math.abs(pxA[iA] - pxB[iB])
        + Math.abs(pxA[iA + 1] - pxB[iB + 1])
        + Math.abs(pxA[iA + 2] - pxB[iB + 2]);
    }
  }
  const avgDiff = diff / (sampleH * stripW * 3);
  return { flat: false, avgDiff, isMatch: avgDiff < maxAvgDiff };
}
