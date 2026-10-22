
import catalog from '../locales/en.js';

/** i18n key prefixes whose values are searchable settings text, per section. */
export const SECTION_SEARCH_PREFIXES = {
  general:            ['settings.general.', 'settings.display.'],
  library:            ['library.categories.', 'library.export.', 'library.import_export.', 'backup.'],
  collections:        ['collections.'],
  'manga-management': ['settings.manga_mgmt.'],
  trash:              ['trash.'],
  downloads:          ['settings.downloads.'],
  offline:            ['settings.offline.'],
  scan:               ['settings.scan.'],
  trackers:           ['settings.trackers.'],
  email:              ['settings.email.'],
  webhooks:           ['settings.webhooks.'],
  advanced:           ['settings.advanced.'],
  storage:            ['storage.'],
  maintenance:        ['settings.maintenance.', 'settings.performance.'],
  server:             ['settings.server.'],
  account:            ['settings.account.'],
  security:           ['settings.security.'],
};

const SEARCH_NOISE = /\.(toast|error|errors|confirm|placeholder|saving|saved|loading|empty|crumb|page_title|success|failed|fail|message|body|delivered|deliveries|delete|test)\b/;

/** Key suffixes that belong to the same logical settings row. */
const ROW_TITLE_SUFFIX = /\.(label|title|group|btn)$/;
const ROW_DESC_SUFFIX = /\.(desc|description|tooltip|subtitle|hint)$/;

/**
 * Builds a map of section id → row-level entries. Catalog keys sharing a base
 * (`x.label` + `x.desc` + `x.tooltip`) collapse into ONE row `{ key, label,
 * desc }`, so a query matching both the title and the description of a row
 * still returns a single result carrying the full row text.
 * @param {Array<{ id: string }>} sections — visible sections only
 * @param {{ exclude?: string[] }} [options] — key prefixes of rows the current layout hides
 * @returns {Map<string, Array<{ key: string, label: string, desc: string }>>}
 */
export function buildSettingsSearchIndex(sections, { exclude = [] } = {}) {
  const idx = new Map();
  const entries = Object.entries(catalog);
  for (const { id } of sections) {
    const prefixes = SECTION_SEARCH_PREFIXES[/** @type {keyof typeof SECTION_SEARCH_PREFIXES} */ (id)];
    if (!prefixes) continue;
    /** @type {Map<string, { key: string, label: string, desc: string }>} */
    const rows = new Map();
    for (const [key, value] of entries) {
      if (typeof value !== 'string') continue;
      if (SEARCH_NOISE.test(key)) continue;
      if (!prefixes.some(p => key.startsWith(p))) continue;
      if (exclude.some(p => key.startsWith(p))) continue;
      const isDesc = ROW_DESC_SUFFIX.test(key);
      const base = key.replace(ROW_DESC_SUFFIX, '').replace(ROW_TITLE_SUFFIX, '');
      let row = rows.get(base);
      if (!row) {
        row = { key: base, label: '', desc: '' };
        rows.set(base, row);
      }
      if (isDesc) {
        if (!row.desc) row.desc = value;
      } else if (!row.label) {
        row.label = value;
      }
    }
    const items = [];
    const seen = new Set();
    for (const row of rows.values()) {
      if (!row.label) continue;
      if (seen.has(row.label)) continue;
      seen.add(row.label);
      items.push(row);
    }
    idx.set(id, items);
  }
  return idx;
}
