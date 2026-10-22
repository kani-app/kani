// @ts-check

/**
 * @typedef {{
 *   setChapterReadStatus: (chapterIds: number[], isRead: boolean) => Promise<unknown>,
 * }} ChapterEndApi
 */

/**
 * @param {{
 *   api: ChapterEndApi,
 *   chapterId: number,
 *   nextChapterId: number|null|undefined,
 *   showEndCard: boolean,
 *   onEndCard: () => void,
 *   navigateChapter: (chId: number) => void,
 *   navigateToManga: () => void,
 * }} deps
 * @returns {Promise<void>}
 */
export function finishChapter({ api, chapterId, nextChapterId, showEndCard, onEndCard, navigateChapter, navigateToManga }) {
  const saved = api.setChapterReadStatus([chapterId], true).catch(() => {});
  if (showEndCard) {
    onEndCard();
  } else if (nextChapterId) {
    navigateChapter(nextChapterId);
  } else {
    return saved.then(() => navigateToManga());
  }
  return saved.then(() => {});
}
