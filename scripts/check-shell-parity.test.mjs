// @ts-check
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { elementIds, compareShells, metaTags, compareMeta } from './check-shell-parity.mjs';

test('elementIds: collects ids and ignores commented-out markup', () => {
  const html = `
    <div id="modal-root"></div>
    <!-- <div id="ghost-root"></div> -->
    <div id='popover-root'></div>
    <main class="shell" id="app"></main>`;
  assert.deepEqual([...elementIds(html)].sort(), ['app', 'modal-root', 'popover-root']);
});

test('elementIds: ignores attributes that merely end in id', () => {
  assert.deepEqual([...elementIds('<div aria-labelledby="x" data-id="y" id="real"></div>')], ['real']);
});

test('compareShells: agreeing shells report nothing', () => {
  const a = new Set(['app', 'modal-root']);
  const b = new Set(['modal-root', 'app']);
  assert.deepEqual(compareShells(a, b), { missingFromProd: [], missingFromDev: [] });
});

test('compareShells: names the mount point each shell lacks', () => {
  const dev = new Set(['app', 'popover-root']);
  const prod = new Set(['app', 'toast-root']);
  assert.deepEqual(compareShells(dev, prod), {
    missingFromProd: ['popover-root'],
    missingFromDev: ['toast-root'],
  });
});

test('metaTags: keys on name plus any media qualifier', () => {
  const html = `
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <meta name="theme-color" content="#111113" media="(prefers-color-scheme: dark)">
    <meta name="theme-color" content="#f5f4f0" media="(prefers-color-scheme: light)">`;
  assert.deepEqual([...metaTags(html).entries()].sort(), [
    ['theme-color@(prefers-color-scheme: dark)', '#111113'],
    ['theme-color@(prefers-color-scheme: light)', '#f5f4f0'],
    ['viewport', 'width=device-width, initial-scale=1'],
  ]);
});

test('metaTags: ignores commented-out tags and tags with no name', () => {
  const html = `
    <meta charset="utf-8">
    <meta http-equiv="content-security-policy" content="default-src 'self'">
    <!-- <meta name="ghost" content="x"> -->
    <meta name="real" content="y">`;
  assert.deepEqual([...metaTags(html).keys()], ['real']);
});

test('compareMeta: agreeing shells report nothing', () => {
  const a = new Map([['viewport', 'width=device-width'], ['theme-color', '#000']]);
  const b = new Map([['theme-color', '#000'], ['viewport', 'width=device-width']]);
  assert.deepEqual(compareMeta(a, b), { missingFromProd: [], missingFromDev: [], differing: [] });
});

test('compareMeta: separates a missing tag from one whose content drifted', () => {
  const dev = new Map([['viewport', 'width=device-width'], ['theme-color', '#111113']]);
  const prod = new Map([['viewport', 'width=device-width, viewport-fit=cover'], ['apple-x', 'yes']]);
  assert.deepEqual(compareMeta(dev, prod), {
    missingFromProd: ['theme-color'],
    missingFromDev: ['apple-x'],
    differing: ['viewport'],
  });
});
