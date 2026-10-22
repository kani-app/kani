// @ts-check

const AUTO_MATCH = 0.9;

/**
 * @typedef {{ id: string, title: string, cover_url: string | null, score: number, in_library: boolean }} Candidate
 * @typedef {{ manga_id: number, title: string, candidates: Candidate[], best: string | null, error: string | null }} Match
 * @typedef {'searching'|'error'|'none'|'unsure'|'skipped'|'in_library'|'duplicate'|'review'|'matched'} RowStatus
 */

/**
 * @template T
 * @param {T[]} list
 * @param {number} size
 * @returns {T[][]}
 */
export function chunk(list, size) {
  const out = [];
  for (let i = 0; i < list.length; i += size) out.push(list.slice(i, i + size));
  return out;
}

/**
 * @param {number[]} order
 * @param {Map<number, Match>} matches
 * @param {Map<number, string>} choices
 */
export function planBatch(order, matches, choices) {
  /** @type {Map<number, RowStatus>} */
  const statuses = new Map();
  /** @type {{ manga_id: number, target_source_manga_id: string }[]} */
  const items = [];
  /** @type {Set<string>} */
  const claimed = new Set();

  for (const mangaId of order) {
    const match = matches.get(mangaId);
    if (!match) { statuses.set(mangaId, 'searching'); continue; }
    if (match.error) { statuses.set(mangaId, 'error'); continue; }

    const choice = choices.get(mangaId) ?? match.best ?? '';
    const candidate = match.candidates.find(c => c.id === choice);
    if (!candidate) {
      const top = match.candidates[0];
      statuses.set(mangaId, choices.has(mangaId) ? 'skipped'
        : !top ? 'none'
        : top.in_library ? 'in_library'
        : 'unsure');
      continue;
    }
    if (candidate.in_library) { statuses.set(mangaId, 'in_library'); continue; }
    if (claimed.has(candidate.id)) { statuses.set(mangaId, 'duplicate'); continue; }

    claimed.add(candidate.id);
    statuses.set(mangaId, candidate.score >= AUTO_MATCH ? 'matched' : 'review');
    items.push({ manga_id: mangaId, target_source_manga_id: candidate.id });
  }

  return { statuses, items };
}

/**
 * @param {unknown} error
 * @returns {string | null}
 */
export function jobErrorMessage(error) {
  let message = null;
  if (typeof error === 'string') message = error;
  else if (error && typeof error === 'object') {
    message = Object.values(error).find(v => typeof v === 'string') ?? null;
  }
  if (!message) return null;
  return message.replace(/^(?:[A-Z][A-Za-z ]*(?:error)?: )+/, '');
}
