-- Generated squash of every migration through 20260818000001. Regenerate with
-- `scripts/squash-migrations.py`; do not hand-edit, or the schema this produces
-- stops matching the history it replaces.

CREATE TABLE sources (
    id INTEGER PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT 0,
    base_url TEXT NOT NULL DEFAULT '',
    favourited BOOLEAN NOT NULL DEFAULT 0
, unrestricted_http BOOLEAN NOT NULL DEFAULT 0, deleted_at DATETIME, mihon_source_id INTEGER, download_concurrency INTEGER, icon TEXT, description TEXT, languages TEXT, schema_version INTEGER NOT NULL DEFAULT 1, load_error TEXT, browser_enabled BOOLEAN NOT NULL DEFAULT 1);

CREATE TABLE chapters (
    id INTEGER PRIMARY KEY NOT NULL,
    manga_id INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    source_chapter_id TEXT NOT NULL,
    name TEXT,
    chapter_number REAL NOT NULL,
    language TEXT NOT NULL,
    volume INTEGER,
    scanlator TEXT,
    uploaded_at DATETIME,
    download_status INTEGER NOT NULL CHECK (download_status IN (0, 1, 2)) DEFAULT 0,
    discovered_at DATETIME, is_orphaned BOOLEAN NOT NULL DEFAULT 0, page_count INTEGER, downloaded_at DATETIME, resume_offset INTEGER NOT NULL DEFAULT 0, download_error TEXT, delete_status TEXT, volume_id INTEGER REFERENCES volumes(id) ON DELETE SET NULL, file_path TEXT, content_hash TEXT, manifest_json TEXT, file_verified_at INTEGER, quality_long_edge INTEGER, quality_bytes_per_mp REAL, upgrade_available TEXT, source_page_count INTEGER, quality_encoder INTEGER, quality_colour TEXT,
    UNIQUE (manga_id, source_chapter_id)
);

CREATE TABLE settings (
    id TEXT NOT NULL DEFAULT 'singleton' PRIMARY KEY CHECK (id = 'singleton'),
    flaresolverr_url TEXT NOT NULL DEFAULT '',
    library_path TEXT NOT NULL DEFAULT './library',
    wasm_storage_path TEXT NOT NULL DEFAULT './wasm_sources',
    concurrent_page_downloads INTEGER NOT NULL DEFAULT 4,
    max_retries INTEGER NOT NULL DEFAULT 3,
    initial_retry_delay_ms INTEGER NOT NULL DEFAULT 100,
    max_wasm_instances INTEGER NOT NULL DEFAULT 1000,
    auto_scan BOOLEAN NOT NULL DEFAULT 0,
    scan_interval_minutes INTEGER NOT NULL DEFAULT 60,
    default_tracking_enabled BOOLEAN NOT NULL DEFAULT TRUE, http_request_logging BOOLEAN NOT NULL DEFAULT 0, registration_enabled BOOLEAN NOT NULL DEFAULT 0, scan_exclude_completed BOOLEAN NOT NULL DEFAULT FALSE, cover_max_dimension INTEGER, auto_download_category_ids TEXT NOT NULL DEFAULT '[]', email_enabled BOOLEAN NOT NULL DEFAULT FALSE, email_provider TEXT NOT NULL DEFAULT 'smtp', email_provider_config TEXT NOT NULL DEFAULT '{}', email_from_address TEXT NOT NULL DEFAULT '', app_url TEXT NOT NULL DEFAULT '', password_reset_enabled BOOLEAN NOT NULL DEFAULT TRUE, email_verification_required BOOLEAN NOT NULL DEFAULT FALSE, v8_debug_logging BOOLEAN NOT NULL DEFAULT 0, first_run_complete BOOLEAN NOT NULL DEFAULT 0, scan_concurrency INTEGER NOT NULL DEFAULT 2, per_source_download_concurrency INTEGER NOT NULL DEFAULT 1, job_max_history INTEGER NOT NULL DEFAULT 1000, job_shutdown_timeout_secs INTEGER NOT NULL DEFAULT 30, backup_schedule_json TEXT, trash_retention_days INTEGER NOT NULL DEFAULT 30, audit_retention_days INTEGER NOT NULL DEFAULT 365, audit_security_retention_days INTEGER NOT NULL DEFAULT 0, disk_warn_threshold REAL NOT NULL DEFAULT 0.10, thumbnail_formats TEXT NOT NULL DEFAULT 'jpeg', max_login_attempts INTEGER NOT NULL DEFAULT 5, max_ip_attempts INTEGER NOT NULL DEFAULT 20, login_lockout_seconds INTEGER NOT NULL DEFAULT 900, session_timeout_secs INTEGER NOT NULL DEFAULT 2592000, tracker_auto_sync_enabled BOOLEAN NOT NULL DEFAULT FALSE, tracker_sync_interval_hours INTEGER NOT NULL DEFAULT 24, max_concurrent_jobs INTEGER NOT NULL DEFAULT 10, db_maintenance_interval_hours INTEGER NOT NULL DEFAULT 24, db_vacuum_interval_hours INTEGER NOT NULL DEFAULT 168, audit_prune_interval_hours INTEGER NOT NULL DEFAULT 168, trash_purge_interval_hours INTEGER NOT NULL DEFAULT 168, v8_max_memory_mb INTEGER NOT NULL DEFAULT 512, v8_idle_timeout_s INTEGER NOT NULL DEFAULT 300, update_check_enabled BOOLEAN NOT NULL DEFAULT 1, integrity_quick_scrub_interval_hours INTEGER NOT NULL DEFAULT 24, integrity_deep_scrub_interval_hours INTEGER NOT NULL DEFAULT 168, scrub_on_startup BOOLEAN NOT NULL DEFAULT 0, upgrade_detection_enabled BOOLEAN NOT NULL DEFAULT 1, upgrade_min_res_gain REAL NOT NULL DEFAULT 1.2, upgrade_confirm_fetches INTEGER NOT NULL DEFAULT 3, upgrade_axis_resolution TEXT NOT NULL DEFAULT 'both', upgrade_axis_colour TEXT NOT NULL DEFAULT 'both', upgrade_axis_encoder TEXT NOT NULL DEFAULT 'both', upgrade_axis_bitrate TEXT NOT NULL DEFAULT 'gain', upgrade_show_downgrades BOOLEAN NOT NULL DEFAULT 0, upgrade_auto_replace_reasons TEXT NOT NULL
    DEFAULT 'preferred_scanlator,resolution,colour', integrity_revalidate_after_days INTEGER NOT NULL DEFAULT 30, opds_page_index_zero_based BOOLEAN NOT NULL DEFAULT 0, scan_barren_page_tolerance INTEGER NOT NULL DEFAULT 3, global_search_timeout_secs INTEGER NOT NULL DEFAULT 6);

CREATE TABLE tags (
    id INTEGER PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE manga_tags (
    manga_id INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    tag_id   INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (manga_id, tag_id)
);

CREATE TABLE people (
    id   INTEGER PRIMARY KEY NOT NULL,
    name TEXT    NOT NULL UNIQUE
);

CREATE TABLE manga_people (
    manga_id  INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    person_id INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('author', 'artist')),
    PRIMARY KEY (manga_id, person_id, role)
);

CREATE TABLE categories (
    id         INTEGER PRIMARY KEY NOT NULL,
    name       TEXT NOT NULL UNIQUE,
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE manga_categories (
    manga_id    INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
    PRIMARY KEY (manga_id, category_id)
);

CREATE TABLE source_preferences (
    source_id  INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    key        TEXT    NOT NULL,
    value      TEXT    NOT NULL DEFAULT 'null',
    PRIMARY KEY (source_id, key)
);

CREATE TABLE users (
    id            INTEGER PRIMARY KEY NOT NULL,
    username      TEXT    NOT NULL UNIQUE,
    email         TEXT    NOT NULL UNIQUE,
    password_hash TEXT    NOT NULL,
    change_id     BLOB    NOT NULL,
    is_active     BOOLEAN NOT NULL DEFAULT TRUE,
    created_at    DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_login    DATETIME
, email_verified_at DATETIME);

CREATE TABLE roles (
    slug        TEXT PRIMARY KEY,
    parent      TEXT REFERENCES roles(slug) ON DELETE CASCADE,
    description TEXT
);

CREATE TABLE user_roles (
    user_id    INTEGER  NOT NULL REFERENCES users(id)   ON DELETE CASCADE,
    role_slug  TEXT     NOT NULL REFERENCES roles(slug) ON DELETE CASCADE,
    granted_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    granted_by INTEGER  REFERENCES users(id),
    PRIMARY KEY (user_id, role_slug)
);

CREATE TABLE role_permissions (
    role_slug   TEXT NOT NULL REFERENCES roles(slug) ON DELETE CASCADE,
    permission  TEXT NOT NULL,
    PRIMARY KEY (role_slug, permission)
);

CREATE TABLE audit_log (
    id         INTEGER  PRIMARY KEY NOT NULL,
    user_id    INTEGER  REFERENCES users(id) ON DELETE SET NULL,
    action     TEXT     NOT NULL,
    target     TEXT,
    details    TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE user_chapter_tracking (
    user_id        INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    chapter_id     INTEGER NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    is_read        BOOLEAN NOT NULL DEFAULT FALSE,
    last_page_read INTEGER NOT NULL DEFAULT 0,
    last_read_at   DATETIME,
    PRIMARY KEY (user_id, chapter_id)
);

CREATE TABLE user_manga_tracking (
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    manga_id INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    status   INTEGER NOT NULL DEFAULT 0 CHECK (status IN (0, 1, 2, 3, 4, 5)),
    score    REAL, tracking_enabled BOOLEAN NOT NULL DEFAULT TRUE, last_seen_at DATETIME, reading_direction TEXT NOT NULL DEFAULT 'rtl', notify_new_chapters BOOLEAN NOT NULL DEFAULT TRUE, reader_prefs TEXT,
    PRIMARY KEY (user_id, manga_id)
);

CREATE TABLE trackers (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE user_tracker_credentials (
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tracker_id    INTEGER NOT NULL REFERENCES trackers(id) ON DELETE CASCADE,
    access_token  TEXT,
    refresh_token TEXT,
    expires_at    DATETIME, needs_reauth BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (user_id, tracker_id)
);

CREATE TABLE tracker_manga_mappings (
    user_id          INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tracker_id       INTEGER NOT NULL REFERENCES trackers(id) ON DELETE CASCADE,
    manga_id         INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    tracker_manga_id TEXT NOT NULL, last_synced_at DATETIME,
    PRIMARY KEY (user_id, tracker_id, manga_id)
);

CREATE TABLE tracker_app_config (
    tracker_id     INTEGER NOT NULL PRIMARY KEY REFERENCES trackers(id) ON DELETE CASCADE,
    client_id      TEXT    NOT NULL,
    client_secret  TEXT            -- NULL for PKCE-only providers (MAL)
);

CREATE TABLE oauth_pkce_state (
    state         TEXT     NOT NULL PRIMARY KEY,
    code_verifier TEXT,            -- NULL for non-PKCE providers (AniList)
    tracker_id    INTEGER  NOT NULL,
    redirect_uri  TEXT     NOT NULL,
    created_at    DATETIME NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE "download_rules" (
    id        INTEGER PRIMARY KEY NOT NULL,
    manga_id  INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    rule_type TEXT NOT NULL CHECK (rule_type IN (
        'scanlator_include', 'scanlator_exclude',
        'language_include',  'language_exclude',
        'title_contains',    'title_excludes',
        'chapter_number_min', 'chapter_number_max',
        'exclude_fractional', 'max_age_days',
        'published_after'
    )),
    value     TEXT NOT NULL
, priority INTEGER NOT NULL DEFAULT 0);

CREATE TABLE "manga" (
    id                          INTEGER  PRIMARY KEY NOT NULL,
    source_id                   INTEGER  NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    source_manga_id             TEXT     NOT NULL,
    name                        TEXT     NOT NULL,
    cover_url                   TEXT,
    local_cover_path            TEXT,
    description                 TEXT,
    status                      INTEGER  NOT NULL CHECK (status IN (0, 1, 2, 3, 4)),
    auto_download               BOOLEAN  NOT NULL DEFAULT 0,
    created_at                  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at                  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    scanlator_mode              TEXT     NOT NULL DEFAULT 'priority',
    download_all_preferred_only BOOLEAN  NOT NULL DEFAULT 1, auto_scan BOOLEAN NOT NULL DEFAULT TRUE, notes TEXT, local_name        TEXT, local_description TEXT, local_status      INTEGER, cover_overridden  BOOLEAN NOT NULL DEFAULT FALSE, is_orphaned BOOLEAN NOT NULL DEFAULT FALSE, cover_hash TEXT, deleted_at DATETIME, upgrade_auto_replace BOOLEAN NOT NULL DEFAULT 0, suppressed_chapter_count INTEGER NOT NULL DEFAULT 0,
    UNIQUE (source_id, source_manga_id)
);

CREATE TABLE captcha_challenges (
    id TEXT PRIMARY KEY,
    answer INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE TABLE source_health (
  source_id INTEGER PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
  last_success_at DATETIME,
  last_error_at DATETIME,
  consecutive_error_count INTEGER NOT NULL DEFAULT 0,
  avg_response_ms REAL
);

CREATE TABLE manga_local_authors (
    id       INTEGER PRIMARY KEY NOT NULL,
    manga_id INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    name     TEXT    NOT NULL,
    role     TEXT    NOT NULL CHECK (role IN ('author', 'artist'))
);

CREATE TABLE manga_local_tags (
    id       INTEGER PRIMARY KEY NOT NULL,
    manga_id INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    name     TEXT NOT NULL
);

CREATE TABLE pending_imports (
    id                    INTEGER PRIMARY KEY,
    user_id               INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    origin                TEXT NOT NULL,
    title                 TEXT NOT NULL,
    source_hint           TEXT,
    source_manga_id       TEXT,
    description           TEXT,
    cover_url             TEXT,
    authors               TEXT,
    tags                  TEXT,
    status                INTEGER,
    tracking              TEXT,
    chapter_progress      TEXT,
    possible_duplicate_of INTEGER REFERENCES manga(id) ON DELETE SET NULL,
    duplicate_similarity  REAL,
    resolved              BOOLEAN NOT NULL DEFAULT FALSE,
    created_at            DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE duplicate_pairs (
    manga_a_id INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    manga_b_id INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    similarity REAL NOT NULL,
    author_match BOOLEAN NOT NULL DEFAULT FALSE,
    dismissed  BOOLEAN NOT NULL DEFAULT FALSE,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (manga_a_id, manga_b_id),
    CHECK (manga_a_id < manga_b_id)
);

CREATE TABLE password_reset_tokens (
    id          INTEGER PRIMARY KEY NOT NULL,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT    NOT NULL UNIQUE,
    expires_at  DATETIME NOT NULL,
    used_at     DATETIME,
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE email_verification_tokens (
    id          INTEGER PRIMARY KEY NOT NULL,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT    NOT NULL UNIQUE,
    expires_at  DATETIME NOT NULL,
    used_at     DATETIME,
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE webhooks (
    id         INTEGER PRIMARY KEY NOT NULL,
    url        TEXT NOT NULL,
    secret     TEXT,
    events     TEXT NOT NULL DEFAULT '["*"]',
    enabled    BOOLEAN NOT NULL DEFAULT TRUE,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE webhook_manga_overrides (
    webhook_id INTEGER NOT NULL,
    manga_id   INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    enabled    BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (webhook_id, manga_id)
);

CREATE TABLE webhook_deliveries (
    id           INTEGER PRIMARY KEY NOT NULL,
    webhook_id   INTEGER NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    event_type   TEXT NOT NULL,
    payload      TEXT NOT NULL,
    http_status  INTEGER,
    error        TEXT,
    delivered_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE user_page_bookmarks (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    chapter_id INTEGER NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    page_index INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, chapter_id, page_index)
);

CREATE TABLE user_chapter_notes (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    chapter_id INTEGER NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    note       TEXT    NOT NULL DEFAULT '',
    updated_at TEXT    NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, chapter_id)
);

CREATE TABLE login_attempts (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    identity_hash TEXT    NOT NULL,
    ip_addr       TEXT    NOT NULL,
    succeeded     BOOLEAN NOT NULL,
    attempted_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE user_sessions (
    id           TEXT    PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   INTEGER NOT NULL DEFAULT (unixepoch()),
    last_seen_at INTEGER NOT NULL DEFAULT (unixepoch()),
    user_agent   TEXT,
    ip_addr      TEXT,
    revoked_at   INTEGER
);

CREATE TABLE user_totp (
    user_id     INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    secret      TEXT    NOT NULL,
    verified_at INTEGER,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE user_backup_codes (
    id        TEXT    PRIMARY KEY DEFAULT (lower(hex(randomblob(8)))),
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash TEXT    NOT NULL,
    used_at   INTEGER
);

CREATE TABLE extension_cache (
    namespace TEXT NOT NULL,
    key       TEXT NOT NULL,
    value     BLOB NOT NULL,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY (namespace, key)
);

CREATE TABLE manga_external_ids (
    manga_id    INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    provider    TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY (manga_id, provider)
);

CREATE TABLE jobs (
    id              TEXT        NOT NULL PRIMARY KEY,
    job_type        TEXT        NOT NULL,
    status          TEXT        NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending','running','paused','completed','failed','cancelled')),
    priority        INTEGER     NOT NULL DEFAULT 50,
    description     TEXT        NOT NULL DEFAULT '',
    created_at      INTEGER     NOT NULL DEFAULT (unixepoch()),
    started_at      INTEGER,
    completed_at    INTEGER,
    user_id         INTEGER     REFERENCES users(id) ON DELETE SET NULL,
    progress_json   TEXT,
    error_json      TEXT,
    params_json     TEXT,
    result_json     TEXT
);

CREATE TABLE source_circuit_breakers (
    source_id INTEGER NOT NULL PRIMARY KEY,
    state TEXT NOT NULL DEFAULT 'closed',
    failure_count INTEGER NOT NULL DEFAULT 0,
    last_failure_at INTEGER,
    next_retry_at INTEGER,
    FOREIGN KEY (source_id) REFERENCES sources(id) ON DELETE CASCADE
);

CREATE TABLE repo_trust (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    maintainer_key TEXT NOT NULL,
    trusted_level TEXT NOT NULL DEFAULT 'community',
    last_refreshed_at TEXT,
    index_cache TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE blocked_repos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url TEXT NOT NULL UNIQUE,
    reason TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE VIRTUAL TABLE manga_fts USING fts5(
    manga_id UNINDEXED,
    name,
    local_name,
    description,
    authors,
    tokenize = 'unicode61'
);

CREATE TABLE cover_thumbnails (
    manga_id  INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    size      TEXT    NOT NULL,
    format    TEXT    NOT NULL,
    path      TEXT    NOT NULL,
    file_size INTEGER NOT NULL,
    created_at TEXT   NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (manga_id, size, format)
);

CREATE TABLE volumes (
    id         INTEGER PRIMARY KEY,
    manga_id   INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    name       TEXT,
    volume_num REAL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE smart_collections (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    rule_json  TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE saved_searches (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    query_json TEXT NOT NULL
);

CREATE TABLE recurring_jobs (
    kind        TEXT NOT NULL PRIMARY KEY,
    last_run_at DATETIME,
    next_due_at DATETIME NOT NULL
);

CREATE TABLE storage_history (
    id                  INTEGER PRIMARY KEY,
    captured_at         DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    library_used_bytes  INTEGER NOT NULL DEFAULT 0,
    cover_used_bytes    INTEGER NOT NULL DEFAULT 0,
    chapter_used_bytes  INTEGER NOT NULL DEFAULT 0,
    data_used_bytes     INTEGER NOT NULL DEFAULT 0,
    free_bytes          INTEGER NOT NULL DEFAULT 0,
    total_manga         INTEGER NOT NULL DEFAULT 0,
    total_chapters      INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE api_tokens (
    id           TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    scopes       TEXT NOT NULL DEFAULT 'opds:read opds:progress',
    created_at   INTEGER NOT NULL DEFAULT (unixepoch()),
    last_used_at INTEGER,
    expires_at   INTEGER,
    revoked_at   INTEGER
, kind TEXT NOT NULL DEFAULT 'opds');

CREATE TABLE scrub_reports (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    depth       TEXT NOT NULL,
    report_json TEXT NOT NULL,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE "scanlator_preferences" (
    id         INTEGER PRIMARY KEY NOT NULL,
    manga_id   INTEGER REFERENCES manga(id) ON DELETE CASCADE,
    scanlator  TEXT NOT NULL,
    priority   INTEGER NOT NULL DEFAULT 0,
    blocked    BOOLEAN NOT NULL DEFAULT 0,
    UNIQUE (manga_id, scanlator)
);

CREATE TABLE ui_themes (
    id          TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(8)))),
    user_id     INTEGER REFERENCES users(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    tokens_json TEXT NOT NULL,
    custom_css  TEXT,
    is_active   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX idx_chapters_manga_number ON chapters(manga_id, chapter_number DESC);

CREATE INDEX idx_manga_tags_tag_id ON manga_tags(tag_id);

CREATE INDEX idx_manga_people_person_id ON manga_people(person_id, role);

CREATE INDEX idx_chapters_download_status ON chapters(download_status);

CREATE INDEX idx_user_roles_user ON user_roles(user_id);

CREATE INDEX idx_users_username
    ON users(username);

CREATE INDEX idx_source_preferences_source_id
    ON source_preferences(source_id);

CREATE INDEX idx_audit_log_user_created
    ON audit_log(user_id, created_at DESC);

CREATE INDEX idx_audit_log_action_created
    ON audit_log(action, created_at DESC);

CREATE INDEX idx_user_chapter_recent ON user_chapter_tracking(user_id, last_read_at DESC);

CREATE INDEX idx_download_rules_manga ON download_rules(manga_id);

CREATE INDEX idx_manga_name    ON manga(name);

CREATE INDEX idx_manga_updated ON manga(updated_at DESC);

CREATE INDEX idx_manga_source  ON manga(source_id);

CREATE INDEX idx_mla_manga ON manga_local_authors(manga_id);

CREATE INDEX idx_mlt_manga ON manga_local_tags(manga_id);

CREATE INDEX idx_duplicate_pairs_b ON duplicate_pairs(manga_b_id);

CREATE INDEX idx_prt_user       ON password_reset_tokens(user_id);

CREATE INDEX idx_prt_token_hash ON password_reset_tokens(token_hash);

CREATE INDEX idx_evt_user       ON email_verification_tokens(user_id);

CREATE INDEX idx_evt_token_hash ON email_verification_tokens(token_hash);

CREATE INDEX idx_webhook_deliveries_webhook ON webhook_deliveries(webhook_id, delivered_at DESC);

CREATE INDEX idx_sources_mihon_source_id ON sources (mihon_source_id) WHERE mihon_source_id IS NOT NULL;

CREATE INDEX idx_login_attempts_identity ON login_attempts (identity_hash, attempted_at);

CREATE INDEX idx_login_attempts_ip       ON login_attempts (ip_addr,       attempted_at);

CREATE INDEX idx_user_sessions_user_id ON user_sessions (user_id);

CREATE INDEX idx_user_sessions_active  ON user_sessions (user_id, revoked_at)
    WHERE revoked_at IS NULL;

CREATE INDEX idx_backup_codes_user ON user_backup_codes (user_id, used_at)
    WHERE used_at IS NULL;

CREATE INDEX idx_extension_cache_expires
    ON extension_cache (expires_at)
    WHERE expires_at > 0;

CREATE INDEX idx_jobs_status_priority_created
    ON jobs (status, priority DESC, created_at ASC);

CREATE INDEX idx_jobs_completed_at
    ON jobs (completed_at)
    WHERE status IN ('completed', 'failed', 'cancelled');

CREATE INDEX idx_jobs_user_id
    ON jobs (user_id, created_at DESC);

CREATE UNIQUE INDEX idx_sources_name_unique ON sources(name);

CREATE INDEX idx_manga_auto_scan
    ON manga(auto_scan) WHERE auto_scan = 1;

CREATE INDEX idx_manga_categories_category_id
    ON manga_categories(category_id, manga_id);

CREATE INDEX idx_chapters_orphaned
    ON chapters(is_orphaned) WHERE is_orphaned = 1;

CREATE INDEX idx_source_health_source_id
    ON source_health(source_id, last_error_at DESC);

CREATE INDEX idx_chapters_pending_download
    ON chapters(download_status) WHERE download_status != 2;

CREATE INDEX idx_volumes_manga_id ON volumes(manga_id);

CREATE INDEX idx_saved_searches_user_id ON saved_searches(user_id);

CREATE INDEX idx_storage_history_captured_at ON storage_history(captured_at);

CREATE INDEX idx_api_tokens_user ON api_tokens(user_id) WHERE revoked_at IS NULL;

CREATE INDEX idx_chapters_content_hash ON chapters(content_hash)
    WHERE content_hash IS NOT NULL;

CREATE INDEX idx_scrub_reports_created ON scrub_reports(created_at DESC);

CREATE INDEX idx_api_tokens_kind ON api_tokens(kind)
    WHERE revoked_at IS NULL;

CREATE UNIQUE INDEX idx_scanlator_prefs_global
    ON scanlator_preferences(scanlator) WHERE manga_id IS NULL;

CREATE INDEX idx_scanlator_prefs_manga
    ON scanlator_preferences(manga_id);

CREATE INDEX idx_chapters_source_page_count
    ON chapters (manga_id)
    WHERE source_page_count IS NOT NULL;

CREATE INDEX idx_ui_themes_user ON ui_themes(user_id);

CREATE TRIGGER manga_updated_at
AFTER UPDATE ON manga
FOR EACH ROW
BEGIN
    UPDATE manga SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
END;

CREATE TRIGGER manga_fts_insert AFTER INSERT ON manga BEGIN
    INSERT INTO manga_fts(manga_id, name, local_name, description, authors)
    VALUES (NEW.id, NEW.name, NEW.local_name, NEW.description, '');
END;

CREATE TRIGGER manga_fts_update AFTER UPDATE OF name, local_name, description ON manga BEGIN
    DELETE FROM manga_fts WHERE manga_id = OLD.id;
    INSERT INTO manga_fts(manga_id, name, local_name, description, authors)
    VALUES (
        NEW.id,
        NEW.name,
        NEW.local_name,
        NEW.description,
        COALESCE((
            SELECT GROUP_CONCAT(n, ' ')
            FROM (
                SELECT p.name AS n FROM manga_people mp JOIN people p ON mp.person_id = p.id WHERE mp.manga_id = NEW.id
                UNION ALL
                SELECT name AS n FROM manga_local_authors WHERE manga_id = NEW.id
            )
        ), '')
    );
END;

CREATE TRIGGER manga_fts_delete AFTER DELETE ON manga BEGIN
    DELETE FROM manga_fts WHERE manga_id = OLD.id;
END;

INSERT INTO roles (slug, parent, description) VALUES ('user', NULL, 'Standard authenticated user');
INSERT INTO roles (slug, parent, description) VALUES ('admin', 'user', 'Full access to all resources');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'library:view');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'library:add');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'library:delete');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'chapter:download');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'chapter:delete');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'source:browse');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'settings:view');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'settings:edit_download');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'settings:edit_scan');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'library:refresh');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'library:manage');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'source:configure');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'source:toggle_enabled');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'source:install');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'source:delete');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'settings:edit_advanced');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'user:manage');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'server:manage');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'admin:view_logs');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'admin:view_audit');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'admin:manage');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'admin:jobs');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'repo:add');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'repo:remove');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'repo:trust');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'repo:refresh');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'opds:read');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'opds:progress');
INSERT INTO role_permissions (role_slug, permission) VALUES ('user', 'token:create_opds');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'token:create_api');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'metrics:read');
INSERT INTO role_permissions (role_slug, permission) VALUES ('admin', 'theme:publish');
INSERT INTO settings (id, flaresolverr_url, library_path, wasm_storage_path, concurrent_page_downloads, max_retries, initial_retry_delay_ms, max_wasm_instances, auto_scan, scan_interval_minutes, default_tracking_enabled, http_request_logging, registration_enabled, scan_exclude_completed, cover_max_dimension, auto_download_category_ids, email_enabled, email_provider, email_provider_config, email_from_address, app_url, password_reset_enabled, email_verification_required, v8_debug_logging, first_run_complete, scan_concurrency, per_source_download_concurrency, job_max_history, job_shutdown_timeout_secs, backup_schedule_json, trash_retention_days, audit_retention_days, audit_security_retention_days, disk_warn_threshold, thumbnail_formats, max_login_attempts, max_ip_attempts, login_lockout_seconds, session_timeout_secs, tracker_auto_sync_enabled, tracker_sync_interval_hours, max_concurrent_jobs, db_maintenance_interval_hours, db_vacuum_interval_hours, audit_prune_interval_hours, trash_purge_interval_hours, v8_max_memory_mb, v8_idle_timeout_s, update_check_enabled, integrity_quick_scrub_interval_hours, integrity_deep_scrub_interval_hours, scrub_on_startup, upgrade_detection_enabled, upgrade_min_res_gain, upgrade_confirm_fetches, upgrade_axis_resolution, upgrade_axis_colour, upgrade_axis_encoder, upgrade_axis_bitrate, upgrade_show_downgrades, upgrade_auto_replace_reasons, integrity_revalidate_after_days, opds_page_index_zero_based, scan_barren_page_tolerance, global_search_timeout_secs) VALUES ('singleton', '', './library', './wasm_sources', 4, 3, 100, 1000, 0, 60, 1, 0, 0, 0, NULL, '[]', 0, 'smtp', '{}', '', '', 1, 0, 0, 0, 2, 1, 1000, 30, NULL, 30, 365, 0, 0.1, 'jpeg', 5, 20, 900, 2592000, 0, 24, 10, 24, 168, 168, 168, 512, 300, 1, 24, 168, 0, 1, 1.2, 3, 'both', 'both', 'both', 'gain', 0, 'preferred_scanlator,resolution,colour', 30, 0, 3, 6);
