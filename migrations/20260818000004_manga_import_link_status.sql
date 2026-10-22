-- An imported id is not one this source can address until the resolve job
-- proves a replacement. IF NOT EXISTS keeps the migration re-runnable when a
-- pre-squash database is adopted.
CREATE TABLE IF NOT EXISTS manga_import_links (
    manga_id INTEGER PRIMARY KEY REFERENCES manga(id) ON DELETE CASCADE,
    status   TEXT NOT NULL CHECK (status IN ('pending', 'unlinked'))
);
