// @ts-check

/**
 * Grid of manga card skeletons.
 * @param {number} count
 * @returns {string}
 */
export function skeletonGrid(count = 24) {
  return `<div class="manga-grid">${skeletonGridItems(count)}</div>`;
}

/**
 * Card skeletons for a grid element that already exists.
 * @param {number} count
 * @returns {string}
 */
export function skeletonGridItems(count = 24) {
  const card = `
    <div class="flex flex-col gap-2">
      <div class="skeleton w-full rounded-sm" style="aspect-ratio:2/3"></div>
      <div class="skeleton h-3 w-4/5 rounded"></div>
      <div class="skeleton h-3 w-3/5 rounded"></div>
    </div>`;
  return card.repeat(count);
}

/**
 * Grid of source card skeletons.
 * @param {number} count
 * @returns {string}
 */
export function skeletonSourceList(count = 6) {
  const card = `
    <div class="card flex flex-col gap-3">
      <div class="skeleton h-4 w-2/3 rounded"></div>
      <div class="skeleton h-3 w-1/2 rounded"></div>
      <div class="skeleton h-8 w-24 rounded-md"></div>
    </div>`;
  // The auto-fill track depends on viewport width and has no fixed grid token.
  return `<div class="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4">${card.repeat(count)}</div>`;
}

/**
 * Manga hero + chapter list skeleton.
 * Mirrors the two-column layout of manga-details.js:
 *   Mobile  — cover (35%) + meta side-by-side in a row, then buttons full-width, then chapters
 *   Desktop — left col (md:w-1/4): full-width cover with meta + buttons below;
 *             right col (md:flex-1): tag chips then padded chapter rows
 * @returns {string}
 */
export function skeletonMangaHero() {
  const metaRows = [
    'h-5 w-3/4',
    'h-3 w-full',
    'h-3 w-4/5',
    'h-3 w-full',
    'h-3 w-3/5',
    'h-3 w-full',
  ].map(c => `<div class="skeleton ${c} rounded"></div>`).join('');

  const tagChips = [56, 80, 64, 48, 72, 60].map(w =>
    `<div class="skeleton h-6 rounded-full" style="width:${w}px"></div>`
  ).join('');

  const listRows = Array.from({ length: 10 }, () =>
    `<div class="skeleton h-14 w-full rounded-md"></div>`
  ).join('');

  return `
    <div class="max-w-page w-full mx-auto px-4 md:px-6 py-4 md:py-6 flex flex-col gap-6 md:gap-8">
      <div class="skeleton h-4 w-44 rounded"></div>
      <div class="flex flex-col md:flex-row gap-6 md:gap-8 md:items-start">

        <!-- Left column: hero -->
        <div class="w-full flex flex-col gap-3 md:w-1/4 md:shrink-0">
          <!-- Mobile: cover(35%) + meta side by side; Desktop: cover full-width above meta -->
          <div class="flex flex-row items-start gap-3 md:flex-col md:gap-3">
            <div class="skeleton rounded-xl w-1/3 md:w-full shrink-0 md:shrink"
                 style="aspect-ratio:2/3"></div>
            <div class="flex-1 md:flex-none min-w-0 flex flex-col gap-2 pt-1 md:pt-0 md:w-full">
              ${metaRows}
            </div>
          </div>
          <!-- CTA buttons -->
          <div class="flex flex-col gap-2">
            <div class="skeleton h-9 w-full rounded-lg"></div>
            <div class="skeleton h-9 w-full rounded-lg"></div>
          </div>
        </div>

        <!-- Right column: tags + chapter list -->
        <div class="w-full min-w-0 flex flex-col gap-4 md:flex-1">
          <div class="flex flex-wrap gap-2">${tagChips}</div>
          <div class="flex flex-col gap-2">${listRows}</div>
        </div>

      </div>
    </div>`;
}

/**
 * Recent updates group skeletons.
 * @param {number} count
 * @returns {string}
 */
export function skeletonUpdateList(count = 4) {
  const rows = Array.from({ length: 3 }, () =>
    `<div class="skeleton h-10 w-full rounded"></div>`
  ).join('');
  const group = `
    <div class="flex flex-col gap-2 bg-surface border border-border rounded-xl p-4">
      <div class="flex items-center gap-3">
        <div class="skeleton w-16 h-24 rounded-md shrink-0"></div>
        <div class="skeleton h-4 w-48 rounded"></div>
      </div>
      <div class="flex flex-col gap-0 mt-2">${rows}</div>
    </div>`;
  return `<div class="flex flex-col gap-4">${group.repeat(count)}</div>`;
}

/**
 * Settings source card skeletons.
 * @param {number} count
 * @returns {string}
 */
export function skeletonSettingsCards(count = 3) {
  const card = `
    <div class="card flex items-center justify-between gap-4">
      <div class="flex flex-col gap-2 flex-1 min-w-0">
        <div class="skeleton h-4 w-40 rounded"></div>
        <div class="skeleton h-3 w-24 rounded"></div>
      </div>
      <div class="skeleton h-5 w-10 rounded-full"></div>
    </div>`;
  return `<div class="flex flex-col gap-3">${card.repeat(count)}</div>`;
}

/**
 * Rows of label + value placeholders inside a single divided card,
 * e.g. a source health/status summary.
 * @param {number} count
 * @returns {string}
 */
export function skeletonKeyValueRows(count = 4) {
  const row = `
    <div class="flex items-center justify-between gap-4 py-3">
      <div class="skeleton h-4 w-32 rounded"></div>
      <div class="skeleton h-4 w-16 rounded"></div>
    </div>`;
  return `<div class="flex flex-col divide-y divide-border-subtle">${row.repeat(count)}</div>`;
}
