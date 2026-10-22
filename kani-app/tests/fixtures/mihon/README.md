# Mihon and Suwayomi Backup Fixtures

Both fixtures derive from a Suwayomi `.tachibk` backup (`org.suwayomi.tachidesk`, 2026-05-07) and
were produced by `scripts/anonymise-tachibk.py`.

The script processes the protobuf wire format directly rather than decoding through
`kani-app/proto/tachiyomi_backup.proto`. This preserves unsupported blocks byte-for-byte. The
source backup contains top-level fields `101` (source list), `9000` (WebUI preferences), and
`9001` (server settings), for which Kani has no message definitions. Re-encoding through Kani's
schema would remove those fields and weaken the import tests.

## What was changed

- Titles, series and chapter URLs, authors, artists, descriptions, cover URLs,
  chapter names, scanlator names, category names, and source display names are
  replaced with synthetic values of the same shape.
- Host, URL, and account-like values in the `9001` server-settings block are redacted.
- Genres, timestamps, source IDs, status codes, field order, and wire types
  are untouched.
- Truncated to the first 5 series (6 chapters each; 2 for the hostile variant).

## suwayomi-anonymised.tachibk

General-purpose fixture containing five series across three source IDs
(`2499283573021220255`, `2131019126180322627`, `2292947733994124621`), one
category, and chapters with read and last-page-read state.

The `--augment-first` option adds two synthetic fields because the source backup had no tracking
entries or category assignments. The first series receives `categories: [0]` and one
`BackupTracking` entry (`syncId: 2`, AniList). Tests for these fields validate Kani's schema rather
than observed Mihon output. Replace them with anonymised source data if a suitable backup becomes
available.

## hostile-titles.tachibk

Contains the same series with titles replaced by path-traversal, NUL, RTL-override, and
astral-plane cases (`--hostile-titles`). It verifies that imported titles cannot escape the
library directory or corrupt database rows.
