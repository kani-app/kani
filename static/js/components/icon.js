// @ts-check
import { h } from 'preact';
import htm from 'htm';
const html = htm.bind(h);

/**
 * Renders a trusted SVG icon string as an inline element.
 * The wrapper span uses `display: contents` so it has no layout impact.
 *
 * Pass `label` for meaningful icons (e.g. standalone icon buttons).
 * Omit `label` for decorative icons next to text or inside labelled elements.
 *
 * @param {{ svg: string, label?: string }} props
 * @returns {any}
 */
export function Icon({ svg, label }) {
  return label
    ? html`<span class="contents" role="img" aria-label=${label} dangerouslySetInnerHTML=${{ __html: svg }}></span>`
    : html`<span class="contents" aria-hidden="true" dangerouslySetInnerHTML=${{ __html: svg }}></span>`;
}
