// @ts-check
// Pure crop math for the reader. Crop prefs are 0–50 (percent per side).
// No DOM: the caller applies the returned styles / source rect.

/** @typedef {{ cropTop?: number, cropBottom?: number, cropLeft?: number, cropRight?: number }} CropPrefs */

/** Cropped natural width after trimming left/right percentages. Min 1px. */
export function croppedWidth(naturalW, cropLeft = 0, cropRight = 0) {
  return Math.max(1, naturalW * (1 - (cropLeft + cropRight) / 100));
}

/** Cropped natural height after trimming top/bottom percentages. Min 1px. */
export function croppedHeight(naturalH, cropTop = 0, cropBottom = 0) {
  return Math.max(1, naturalH * (1 - (cropTop + cropBottom) / 100));
}

/** True if any side has a non-zero crop. */
export function hasCrop({ cropTop = 0, cropBottom = 0, cropLeft = 0, cropRight = 0 } = {}) {
  return !!(cropTop || cropBottom || cropLeft || cropRight);
}

/**
 * Inline styles that clip an image to the crop rect and pull the surrounding box
 * in with negative margins. Vertical margins are percentages of the containing
 * block's *width*, so top/bottom are scaled by the image aspect ratio (h/w) to
 * approximate a height-relative inset. Returns null when there is no crop.
 *
 * @param {CropPrefs} prefs
 * @param {number} [ratio] — natural height / width; defaults to a 2:3 page
 * @returns {{ clipPath: string, marginTop: string, marginBottom: string, marginLeft: string, marginRight: string } | null}
 */
export function cropStyles(prefs, ratio = 1.5) {
  const { cropTop: ct = 0, cropBottom: cb = 0, cropLeft: cl = 0, cropRight: cr = 0 } = prefs ?? {};
  if (!ct && !cb && !cl && !cr) return null;
  const r = ratio > 0 ? ratio : 1.5;
  return {
    clipPath: `inset(${ct}% ${cr}% ${cb}% ${cl}%)`,
    marginTop: `-${ct * r}%`,
    marginBottom: `-${cb * r}%`,
    marginLeft: `-${cl}%`,
    marginRight: `-${cr}%`,
  };
}

/**
 * Source rectangle for drawImage so a canvas composite is trimmed to the crop.
 * `sx`/`sy` intentionally use the raw crop percentages as a small pixel offset —
 * this preserves the original reader behaviour exactly (the crop is dominated by
 * the trimmed width/height; the tiny origin nudge is inherited, not corrected).
 * @param {number} naturalW
 * @param {number} naturalH
 * @param {CropPrefs} prefs
 * @returns {{ sx: number, sy: number, sw: number, sh: number }}
 */
export function cropSourceRect(naturalW, naturalH, prefs) {
  const { cropTop: ct = 0, cropLeft: cl = 0, cropRight: cr = 0, cropBottom: cb = 0 } = prefs ?? {};
  return {
    sx: cl,
    sy: ct,
    sw: croppedWidth(naturalW, cl, cr),
    sh: croppedHeight(naturalH, ct, cb),
  };
}
