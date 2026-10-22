// @ts-check
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { sourceIconSrc } from './source-icon.js';

const b64 = (/** @type {string} */ s) => Buffer.from(s, 'binary').toString('base64');

test('sourceIconSrc: labels a PNG icon image/png', () => {
  const png = b64('\x89PNG\r\n\x1a\n\0\0\0\rIHDR');
  assert.equal(sourceIconSrc(png), `data:image/png;base64,${png}`);
});

test('sourceIconSrc: labels a WebP icon image/webp', () => {
  const webp = b64('RIFF\x10\0\0\0WEBPVP8 ');
  assert.equal(sourceIconSrc(webp), `data:image/webp;base64,${webp}`);
});

test('sourceIconSrc: renders nothing for markup, garbage or no icon', () => {
  assert.equal(sourceIconSrc(b64("<svg xmlns='http://www.w3.org/2000/svg'/>")), null);
  assert.equal(sourceIconSrc('not base64!'), null);
  assert.equal(sourceIconSrc(null), null);
  assert.equal(sourceIconSrc(undefined), null);
});
