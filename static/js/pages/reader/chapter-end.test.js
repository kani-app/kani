// @ts-check
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { finishChapter } from './chapter-end.js';

function harness({ nextChapterId = /** @type {number|null} */ (null), showEndCard = false } = {}) {
  /** @type {string[]} */
  const calls = [];
  /** @type {() => void} */
  let release = () => {};
  const api = {
    setChapterProgress: async (/** @type {number} */ id, /** @type {number} */ page) => {
      calls.push(`progress:${id}:${page}`);
    },
    setChapterReadStatus: (/** @type {number[]} */ ids, /** @type {boolean} */ isRead) => {
      calls.push(`read:${ids.join(',')}:${isRead}`);
      return new Promise((resolve) => { release = () => { calls.push('read-saved'); resolve(undefined); }; });
    },
  };
  const done = finishChapter({
    api, chapterId: 7, nextChapterId, showEndCard,
    onEndCard: () => calls.push('end-card'),
    navigateChapter: (id) => calls.push(`chapter:${id}`),
    navigateToManga: () => calls.push('manga'),
  });
  return { calls, release: () => release(), done };
}

test('finishing a chapter marks it read rather than resetting it to page 0', async () => {
  const h = harness();
  h.release();
  await h.done;
  assert.ok(h.calls.includes('read:7:true'), `expected a read-status write, got ${h.calls}`);
  assert.ok(!h.calls.includes('progress:7:0'), 'page 0 is saved as unread, undoing the finish');
});

test('the last chapter opens the manga page only after the read status is saved', async () => {
  const h = harness();
  await Promise.resolve();
  assert.ok(!h.calls.includes('manga'), 'navigated before the write was saved');
  h.release();
  await h.done;
  assert.deepEqual(h.calls.slice(-2), ['read-saved', 'manga']);
});

test('a failed write still leaves the reader', async () => {
  const calls = /** @type {string[]} */ ([]);
  await finishChapter({
    api: {
      setChapterReadStatus: async () => { throw new Error('offline'); },
    },
    chapterId: 7, nextChapterId: null, showEndCard: false,
    onEndCard: () => {}, navigateChapter: () => {},
    navigateToManga: () => calls.push('manga'),
  });
  assert.deepEqual(calls, ['manga']);
});

test('a next chapter opens without waiting on the write', () => {
  const h = harness({ nextChapterId: 8 });
  assert.ok(h.calls.includes('chapter:8'));
  assert.ok(h.calls.includes('read:7:true'));
});

test('the end card shows instead of navigating, with the chapter marked read', () => {
  const h = harness({ nextChapterId: 8, showEndCard: true });
  assert.ok(h.calls.includes('end-card'));
  assert.ok(!h.calls.some((c) => c === 'manga' || c.startsWith('chapter:')));
  assert.ok(h.calls.includes('read:7:true'));
});
