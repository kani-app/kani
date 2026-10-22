// @ts-check
import { t } from '../i18n.js';

const MAX_RETRIES = 3;

/**
 * Retries failed image loads before rendering the no-cover fallback.
 * @param {{ url?: string | null, alt?: string, loading?: 'lazy' | 'eager', fetchpriority?: 'high' | 'low' | 'auto' }} props
 * @returns {HTMLElement}
 */
export function createCoverImage({ url, alt = '', loading = 'lazy', fetchpriority = 'auto' }) {
  const wrap = document.createElement('div');
  wrap.className = 'block relative w-full h-full overflow-hidden bg-surface-2';

  if (url) {
    const img = document.createElement('img');
    img.alt = alt;
    img.loading = loading;
    img.fetchPriority = fetchpriority;
    img.decoding = 'async';
    img.className = 'absolute inset-0 w-full h-full object-cover';

    let retries = 0;
    img.addEventListener('error', () => {
      if (retries < MAX_RETRIES) {
        const delay = 1000 * Math.pow(2, retries) + Math.random() * 1000;
        retries++;
        setTimeout(() => { img.src = url; }, delay);
      } else {
        img.remove();
        const fallback = document.createElement('div');
        fallback.className = 'absolute inset-0 flex items-center justify-center text-xs text-text-muted';
        fallback.textContent = t('common.no_cover');
        wrap.appendChild(fallback);
      }
    });

    img.src = url;
    wrap.appendChild(img);
  } else {
    const fallback = document.createElement('div');
    fallback.className = 'absolute inset-0 flex items-center justify-center text-xs text-text-muted';
    fallback.textContent = t('common.no_cover');
    wrap.appendChild(fallback);
  }

  return wrap;
}
