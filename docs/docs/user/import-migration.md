# Importing and Migration

Kani supports three related but different workflows: restoring a Kani backup, importing a
Tachiyomi/Mihon backup, and migrating an existing manga to another source.

## Restore a Kani backup

Under **Settings → Library**, choose a Kani backup ZIP and preview it. The preview identifies its
creation date, contents, source availability, and selectable data groups. Restore can merge manga,
categories, download rules, tracker state, chapter progress, settings, and repository trust data
according to the options shown.

Use this workflow for logical application data. A full disaster recovery also needs the generated
key files and any chapter files that were not included in the archive. See
[Backup and restore](../admin/backup-restore.md).

## Import Tachiyomi or Mihon

Upload a `.tachibk` backup under **Settings → Library**. Kani previews recognised sources and
titles before import. Installed sources can be matched through declared source identifiers;
unmatched titles become pending imports rather than being silently discarded.

Resolve a pending import under **Settings → Manga management** by searching installed sources and
choosing the corresponding title. Delete a pending item only when it should not enter the library.

## Duplicates and orphaned titles

The manga-management section can scan for likely duplicates, merge an accepted pair, dismiss a
false positive, and list titles whose source is no longer installed. Matching is advisory: inspect
the titles, sources, and chapters before merging.

An orphaned title remains locally readable when its downloaded files are intact. Install its
source again or migrate it to restore remote refresh and download capabilities.

## Migrate one title

From a manga page, search another installed source and preview migration. The preview reports how
the destination title and chapters map to the current local entry. Confirm only after reviewing
the effect on source identity, chapter matches, downloads, and progress.

Migration is not a backup and should not be used as a rollback mechanism.
