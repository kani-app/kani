# Frontend Design System

This guide defines Kani's visual and interaction standards. Tokens live in the `@theme` block of
`static/css/app.css`. CI checks token use with `node scripts/audit-tokens.mjs --check --max 0`,
i18n keys with `scripts/check-i18n-keys.js`, and untranslated literals with
`scripts/check-untranslated-strings.js`.

## Design Identity: Ink and Stamp

Kani's visual identity is **ink and stamp**. Built-in themes use near-monochrome ink-on-paper
surfaces. The vermilion accent represents a hanko stamp and must remain rare, compact, and
meaningful.

Rules that follow from it:

- Use red only for unread or new indicators, reading progress, and the single primary action in a
  view. Do not use it decoratively, on ghost buttons, on log-level badges, or on multiple header
  actions.
- Primary and destructive are distinct semantics. Destructive actions do not
  reuse the accent fill; they get their own treatment (outline + danger text,
  confirmation weight). A screen must never present "save" and "delete
  everything" as visually identical buttons.
- Establish hierarchy with typography before borders. Reserve the display face for page titles
  and group headings. Use boxes and dividers only when typography and spacing are insufficient.
- Resolve competing emphasis before adding accent colour.

### Repeated Actions

Controls repeated in rows use `btn-secondary`, not `btn-primary`. Reserve `btn-primary` for the
view's main action or a dialog's single confirm action. A control that opens a form is secondary;
the form's confirm action may be primary. Dialog close actions use `btn-ghost`.

## Design tokens

All colour, radius, shadow, z-index, and motion values come from the `@theme` block in
`static/css/app.css`. Do not hard-code them in JavaScript or CSS. The runtime theme system in
`static/js/theme.js` updates these custom properties for built-in and custom themes.

| Group | Tokens | Notes |
|-------|--------|-------|
| Surfaces | `--color-bg`, `--color-surface`, `--color-surface-2/3`, `--color-surface-alt` | Elevation ladder: bg → surface → surface-2 → surface-3 |
| Borders | `--color-border`, `--color-border-subtle` | `subtle` for internal dividers, `border` for control outlines |
| Text | `--color-text`, `--color-text-muted`, `--color-text-faint` | Strictly ordered by prominence: text > muted > faint. `faint` is for tertiary chrome only (section labels, separators), never body copy |
| Accent | `--color-accent`, `--color-accent-hover`, `--color-accent-dim`, `--color-on-accent`, `--color-brand-2` | `on-accent` for any text/icon sitting on an accent or status fill. `brand-2` is the sidebar-mark gradient endpoint |
| Status | `--color-success`, `--color-warn`, `--color-danger`, `--color-danger-dim` | Tint backgrounds with `color-mix(in srgb, var(--color-X) 15%, transparent)` — never a separate literal. `btn-danger` is an outline treatment, never an accent-style fill |
| Type scale | `--text-2xs` … `--text-3xl` | Use the scale, not px values |
| Fonts | `--font-body`, `--font-mono`, `--font-display` | Display face (Zen Kaku Gothic New) applies to `h1`/`h2` via base styles and the `font-display` utility — page and group titles only, never row labels or body copy |
| Radii | `--radius-sm/md/lg/xl/full` | |
| Shadows | `--shadow-sm/md/lg/card/popover/focus-ring` | |
| Motion | `--motion-fast/base/slow` + easings | Every animation gates on `prefers-reduced-motion` (a global catch-all also exists) |
| Z-stack | `--z-sidebar/header/modal/modal-stack/toast/popover/top` + reader/banner layers | Never invent a raw z-index |
| Layout | `--header-h`, `--header-h-mobile`, `--sidebar-w`, `--container-page/narrow` | |
| Icon sizes | `--icon-xs` … `--icon-3xl` via `.icon-*` helper classes | |
| Charts | `--chart-1` … `--chart-11` | Per-theme overrides exist for light |

Permitted literal exceptions (mark JS ones with `// audit-ignore`):

- Reader / cover-viewer pure-black backdrop.
- `theme.js` token *source* values.
- Theme-independent text over image scrims, such as manga-card title gradients.

## Icons

One icon set: **Heroicons v2 outline, `stroke-width="1.5"`, `viewBox="0 0 24 24"`**,
exported as strings from `static/js/icons.js` with explicit
`width="18" height="18"` and `aria-hidden="true"`.

- Never paste an inline `<svg>` into a page or component. If a glyph is
  missing, add it to `icons.js` (Heroicons outline style) and import it.
- The only non-Heroicons entry is `iconDragHandle` (six-dot, fill-based);
  reuse it for every drag grip.
- The default size is 18px in `.btn-icon` and navigation items. To resize an icon, place an
  `.icon-*` class on its wrapper. The helper uses a descendant selector such as
  `.icon-sm svg { … }`; do not apply the class directly to the `<svg>`.
- Progress rings use `viewBox="0 0 32 32"` deliberately; that is not a drift.

## Components

Components in `static/js/components/` cover common patterns:
empty/error states, list items, toasts, modals (`modal.js` is the *only*
dialog implementation — `showConfirm`/`showAlert`), pagination, filters,
master-detail, skeletons, sortable lists (`sortable-list.js` owns drag
handles), breadcrumbs, tabs, menus, combobox, chip groups. Settings pages use
the `_shared.js` builders. Extract any pattern used twice into a component.

Auth pages with centred cards, alerts, and labelled fields use `components/auth-card.js`
(`AuthCard`, `AuthError`, `AuthSuccess`, and `AuthField`).

New components are Preact/htm; the vanilla-DOM escape hatches are limited to
the established legacy and performance-sensitive surfaces. Never mix both
styles inside one component's render path.

Search `static/js/components/` before creating a widget. Several components provide both Preact
and vanilla variants from the same file
(`Tabs`/`renderTabs`, `Pagination`/`renderPagination`, `EmptyState`/
`createEmptyState`, `Callout`/`createCallout`).

Available components and styles include:

- `form/` — `Select` (token-styled popover; sizes to its widest option, not to
  the trigger), `NumberInput` (steppers only for small bounded ranges),
  `DateInput` (labelled), `Callout` (info/warn/danger).
- `modal.js` provides a bottom-anchored `sheet` variant for mobile action sheets.
- `EmptyState` provides a `compact` variant for card sub-lists and table bodies.
- `chip-group.js` exposes `selected()`; do not duplicate its state in the page.
- `.data-table` in `app.css` defines table presentation. Mark numeric columns `.num`.
- `.prose-kani` (app.css) renders server-sanitised markdown (changelog, manga
  descriptions). The client never parses markdown; `render_description()` in
  `kani-web/src/utils.rs` does it, through ammonia.

**Use modals for drill-in flows.** When a row opens a detail editor, wrap the detail view in a
`Modal` at every breakpoint. Do not replace the list inline on desktop while using a modal on
mobile.

Render potentially empty feature tabs as disabled until data confirms that they apply. Do not add
or remove tabs after loading.

## Responsive rules

Breakpoints: `sm` 480 / `md` 768 / `lg` 1024 / `xl` 1400 (Tailwind screens,
overridden in `@theme`).

- **Shell**: fixed sidebar (234px, 200px on md–lg tablets, hidden < md);
  mobile gets the bottom tab nav. The header is `--header-h-mobile` (48px) under
  md, sized so a 40px touch target fits inside it.
- **Bottom reserve**: `.pb-nav-safe` reserves
  `4rem + 1rem + env(safe-area-inset-bottom)` under md — nav height, a gutter,
  and the gesture area. Put it on the element that *scrolls*: several surfaces
  scroll inside `#page-content` rather than being it, and padding an ancestor of
  the scroller reserves nothing. The 1rem is not decoration; nav height alone
  leaves content flush against the bar.
- **Page actions**: under md a page's header actions render in the
  `.page-action-bar` beneath the header, not in the bar itself. Do not hide
  actions behind an unlabelled trigger, and never put navigation in them.
- **Viewport units**: use `dvh`/`svh`, never bare `vh`, for anything sized to
  the viewport (mobile URL-bar collapse). Pattern: `max-height: 90vh;` fallback
  line followed by `max-height: 90dvh;` in CSS; plain `dvh` is fine in inline
  styles.
- **Tables**: every `<table>` is wrapped in a `div.overflow-x-auto`. The page
  body never scrolls horizontally.
- **Shells**: `static/index.html` and `static/index.prod.html` must state the
  same `<meta>` tags. `viewport-fit=cover` is required or every
  `env(safe-area-inset-*)` silently resolves to zero.
- **Touch targets**: interactive controls reach ≥ 40px on touch — `--touch-target`
  — via `@media (hover: none)` bumps. The base button block, `.btn-sm`, `.btn-xs`,
  `.btn-icon`, `.dl-btn`, `.tile-btn`, `.tab-btn`, `.chip` and `.input-sm` carry
  one. A control whose visual size must stay smaller takes `.touch-min` (both
  axes) or `.touch-min-h` (height only, for controls sitting in a text row).
  Hover-only affordances (card menu reveal, row nav arrows) must have an
  always-visible touch equivalent.
- **Hover**: wrap all hover styles in `@media (hover: hover)`.
- **Modals**: use `components/modal.js` for a bottom sheet on mobile
  (`items-end rounded-t-2xl`) and a centred card from `sm:` upward.
- **Grids**: `.manga-grid` = 2 columns under 480px, then
  `auto-fill minmax(140px, 1fr)` (180px for `--large`).

## Client-side state

Use the module that owns the required state. Each module provides the same
`getState`/`setState`/`updateState`/`subscribe` interface:

- `static/js/session.js` — auth/identity: `permissions`, `bootId`, `user`
  (populated from `getCurrentUser()` at boot and after a server restart),
  plus `hasPermission`/`initPermissions`.
- `static/js/cache.js` — SSE-fed server state: `chaptersProgress`,
  `scanNotifications`, `refreshState`, `libraryInvalidation`,
  `sourcesInvalidation`, `scanResult`, `scanningMangaIds`. A few keys
  cross-tab broadcast (see `_BROADCAST_KEYS`).
- `static/js/ui-state.js` — UI-local, not server-derived, not cross-tab:
  `inFlightChapters`, `mangaNotifyPrefs`, `sourcePreferenceVersion`.

Do not add a generic state barrel. If a file needs keys from more than one module, import each
under an alias
(`subscribe as subscribeCache`, `subscribe as subscribeUiState`, etc.) rather
than introducing a re-export layer. See `source-details.js` and `sse.js` for examples.

## Settings search

The settings page and command palette search individual settings through a shared index:
`static/js/settings-search-index.js` exports `SECTION_SEARCH_PREFIXES` (i18n
key prefixes per section) and `buildSettingsSearchIndex()`. Register every new settings section or
key prefix there. A palette hit navigates to `/settings?section=X&q=<text>`;
`settings/index.js`
reads the `q` param on load and re-runs its own search so the matching row
gets the `.search-hit` highlight and scrolls into view.

Use `_confirmDiscard()` in `settings/index.js` for unsaved-change prompts. Do not call
`showConfirm()` directly.

The active section displays `.dirty-dot` while it has unsaved changes. `_startDirtyPoll` in
`settings/index.js` polls `isDirty()` every 400ms so programmatic state changes are detected.

## Density

Compact mode scales the **vertical rhythm of repeated content only** — rows,
list items, table cells — via the `--density` factor (`1` comfortable, `0.55`
compact) and `--chapter-row-h` for the virtualised chapter list. Consumers:
`.li-row`, `.kv`, `.data-table` cells, `[data-settings-row]`, the updates
timeline rows. Nothing else may read `--density`: control heights, tap
targets, cover art and nav chrome must be identical in both modes.

Do not override `--spacing` for density. Tailwind padding, gap, width, and height utilities derive
from this value, while component CSS does not. If a new row surface should be density-aware,
move its vertical padding out of a `py-*` utility into a class that multiplies
by `var(--density)`.

## Dates crossing the API boundary

Check both serialization and selection when a date crosses the API boundary:

1. **`time`'s default serde emits `OffsetDateTime` as a JSON *array* of
   components**, not a string — `new Date(...)` cannot parse it. Any
   `OffsetDateTime` that reaches the client needs
   `#[serde(with = "time::serde::rfc3339")]` (or `::option`).
2. **Select the column.** An omitted timestamp and a malformed timestamp can produce the same
   frontend result.

Emit SQLite text timestamps as RFC3339 in SQL with
`strftime('%Y-%m-%dT%H:%M:%SZ', col)` instead of passing through an ambiguous format.

## Accessibility floor

- Visible focus: global `:focus-visible` outline; components may substitute
  `box-shadow: var(--shadow-focus-ring)`. Hidden inputs (toggle, star) forward
  focus rings to their visual element.
- Icon-only buttons always carry `aria-label` (translated).
- Modals: `role="dialog"`, `aria-modal`, labelled title, focus trap, focus
  restore, Escape to close.
- `prefers-reduced-motion: reduce` collapses all animation.

## Copy

- Every user-visible string goes through `t("key")` (`static/js/i18n.js`),
  catalog in `static/locales/en.js`. No inline literals — including default
  parameter values in components (the historical `'Notice'` / `'OK'` trap).
- Buttons name the action ("Save changes", not "Submit"); the same verb
  follows through to the toast ("Published" after "Publish").
- Errors say what went wrong and what to do next; empty states invite the
  first action. Follow the async-feedback contract below.
- CI runs `scripts/check-untranslated-strings.js` on every push, scanning
  `html\`...\`` text nodes and `.textContent`/`.innerHTML =` string literals.
  A non-translatable technical string (a version prefix `v${...}`,
  an HTTP header name, a CLI command) should be pulled into a local `const`
  and interpolated via `${...}` — that removes it from the lint's scan
  surface without embedding a `// i18n-ignore` comment inside literal HTML
  output (which would render as visible text). `// i18n-ignore` only works
  on a genuine single-line JS statement, never inside a multi-line template
  literal.

## Async feedback contract (summary)

| Situation | Feedback |
|-----------|----------|
| Instant, synchronous | Inline update only |
| Async, result visible in UI | `withBusy`/`useBusy` disable + inline error |
| Async, result not visible | Success toast; `showApiError(e)` on failure |
| Destructive | `showConfirm` first; trigger disabled in flight |
