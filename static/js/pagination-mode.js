// @ts-check
import { getLocal } from './utils.js';

export const PHONE_LAYOUT_QUERY = '(max-width: 767px)';

export const PAGINATION_SETTINGS_PREFIX = 'settings.general.pagination.';

export function isPhoneLayout() {
  return typeof matchMedia === 'function' && matchMedia(PHONE_LAYOUT_QUERY).matches;
}

/** @param {string} key */
export function prefersInfiniteScroll(key) {
  if (isPhoneLayout()) return true;
  return getLocal(key) === 'infinite';
}
