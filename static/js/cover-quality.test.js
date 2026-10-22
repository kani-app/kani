// @ts-check
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { applyCoverQuality } from './cover-quality.js';

test('applyCoverQuality: rewrites the size and keeps the cache hash', () => {
  assert.equal(
    applyCoverQuality('/rest/manga/7/cover?size=sm&h=abcd1234', 'lg'),
    '/rest/manga/7/cover?size=lg&h=abcd1234',
  );
});

test('applyCoverQuality: adds a size to a bare local cover', () => {
  assert.equal(applyCoverQuality('/rest/manga/7/cover', 'md'), '/rest/manga/7/cover?size=md');
});

test('applyCoverQuality: leaves proxied and missing covers untouched', () => {
  assert.equal(applyCoverQuality('/rest/image_proxy?token=abc', 'lg'), '/rest/image_proxy?token=abc');
  assert.equal(applyCoverQuality(null, 'lg'), null);
  assert.equal(applyCoverQuality(undefined, 'lg'), undefined);
});
