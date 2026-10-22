-- Read progress arrives with a backup before the chapters it refers to exist,
-- so it is parked here until a chapter list lands. IF NOT EXISTS keeps the
-- migration re-runnable when a pre-squash database is adopted.
CREATE TABLE IF NOT EXISTS manga_import_progress (
    manga_id          INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    user_id           INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source_chapter_id TEXT    NOT NULL,
    chapter_number    REAL    NOT NULL,
    is_read           BOOLEAN NOT NULL,
    last_page_read    INTEGER NOT NULL,
    PRIMARY KEY (manga_id, user_id, source_chapter_id)
);
