# Trackers

Kani integrates with AniList and MyAnimeList for manga mappings and reading-progress sync.

## Configure credentials

Open **Settings → Trackers**. Tracker client credentials may come from the settings database or
deployment-provided client IDs and secrets, depending on the tracker and installation. Stored
secrets rely on Kani's credential-encryption key.

Redirect URLs must match the public Kani origin and the value registered with the tracker. A
reverse proxy must preserve the public scheme and host.

## Link an account

Start the OAuth flow from the tracker card and complete authorisation on the tracker. Returning to
Kani links the tracker account to the current Kani user. Tracker links are per user, not global
proof that every account is authorised.

Unlinking removes Kani's stored credentials and stops future sync. It does not delete history from
the tracker.

## Map and sync manga

A local manga can be searched and mapped to a tracker entry. Review similarly named results before
saving the mapping. Sync can run for one manga or through tracked background work for eligible
entries.

Progress conflicts are resolved by the current tracker-sync policy and the action shown in the UI;
do not assume a blind two-way merge. Check both services after the first sync and before running a
large manual sync.

Failures appear in job history and diagnostics. Rate limits, expired OAuth credentials, deleted
tracker entries, and incorrect mappings require different fixes, so inspect the recorded error
before reconnecting.
