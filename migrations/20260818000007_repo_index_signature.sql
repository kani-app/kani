-- The detached signature over index_cache, which now holds the exact signed index bytes. Both
-- are re-verified against the pinned maintainer_key whenever the cache is used. Rows cached before
-- this have no signature and must be refreshed.
ALTER TABLE repo_trust ADD COLUMN index_sig TEXT;
