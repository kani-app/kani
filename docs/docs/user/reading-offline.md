# Reading and Offline Use

## Web reader

Open a chapter from its manga page, the continue-reading shelf, recent updates, or download
history. The reader stores page progress and can continue into adjacent chapters when they are
available.

Reader controls cover reading direction and layout, fit and scaling, single-page or spread
presentation, slideshow behavior, and interface visibility. The available controls adapt to image
shape, viewport size, and browser capabilities. Keyboard shortcuts are listed in the in-app
shortcut sheet.

Bookmarks and chapter notes are stored on the server. Display preferences that are personal to a
browser may remain local to that browser.

## Downloaded versus remote pages

A downloaded chapter is served from the library storage. If local data is absent and the source is
available, Kani may resolve pages through the extension at read time. Source outages therefore
affect remote-only chapters but not complete local downloads.

The reader's pure-black image backdrop is intentional and independent of the selected application
theme.

## Browser offline cache

Under **Settings → Offline**, configure the browser's chapter cache and inspect its storage use.
Caching is per browser profile and is distinct from a server-side chapter download:

- A server download is available to authorised clients and survives browser data clearing.
- An offline cache belongs to one browser and can be evicted by that browser.

Sign-out clears account-sensitive cached state. Do not rely on browser cache as a backup.

## OPDS clients

Kani exposes an OPDS catalogue for compatible reader applications. Create an OPDS client token
under **Settings → Clients** and use the URL shown there. Reader tokens are deliberately distinct
from general API tokens and are limited to OPDS use.

An OPDS token's effective access is intersected with the permissions its owner currently holds.
Revoking library access or the token removes access without changing the reader application's
saved URL.

OPDS 1.2 and the PSE page-streaming extension are supported surfaces. Client behaviour still
varies, so confirm progress updates and downloads with the reader you intend to use.
