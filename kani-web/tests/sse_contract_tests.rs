#![allow(clippy::unwrap_used)]

mod common;

use axum::body::Body;
use axum::http::Request;
use futures::StreamExt;
use kani_app::events::{AppEvent, RefreshProgressEvent};
use kani_app::ids::MangaId;
use kani_shared::DownloadProgressEvent;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

const HAND_ROLLED_FRAMES: &[&str] = &["state_snapshot"];

const FRONTEND_PREFIX_HANDLERS: &[&str] = &["job_"];

const NON_SSE_TYPE_DISCRIMINANTS: &[&str] = &[
    "CHAPTER_CACHED",
    "ExcludeFractional",
    "LanguageExclude",
    "LanguageInclude",
    "PublishedAfter",
    "download",
    "running",
    "tracker_linked",
    "weekly",
];

fn app_event_samples() -> Vec<AppEvent> {
    vec![
        AppEvent::NewChapters {
            manga_id: MangaId(1),
            manga_name: "Manga".into(),
            count: 2,
            chapter_ids: vec![1, 2],
            chapter_names: vec!["Ch. 1".into(), "Ch. 2".into()],
        },
        AppEvent::ImportStarted {
            origin: "mihon".into(),
            total: 3,
        },
        AppEvent::ImportProgress {
            origin: "mihon".into(),
            completed: 1,
            total: 3,
            title: "Manga".into(),
        },
        AppEvent::ImportCompleted {
            origin: "mihon".into(),
            imported: 2,
            skipped: 1,
            pending: 0,
        },
        AppEvent::PathMigrationStarted {
            field: "library_path".into(),
            total_bytes: 1024,
        },
        AppEvent::PathMigrationProgress {
            field: "library_path".into(),
            bytes_copied: 512,
            total_bytes: 1024,
        },
        AppEvent::PathMigrationCompleted {
            field: "library_path".into(),
            new_path: "/library".into(),
        },
        AppEvent::PathMigrationFailed {
            field: "library_path".into(),
            error: "denied".into(),
        },
        AppEvent::UpgradesFound {
            manga_id: 1,
            count: 4,
        },
        AppEvent::ChapterListPartial {
            manga_id: MangaId(1),
            received: 20,
        },
        AppEvent::ChapterListComplete {
            manga_id: MangaId(1),
            total: 40,
        },
        AppEvent::ChapterListError {
            manga_id: MangaId(1),
            error: "boom".into(),
        },
        AppEvent::JobStarted {
            job_id: uuid::Uuid::nil(),
            job_type: "scan".into(),
            description: "Scanning".into(),
        },
        AppEvent::JobProgress {
            job_id: uuid::Uuid::nil(),
            job_type: "scan".into(),
            current: 1,
            total: 2,
            message: "working".into(),
        },
        AppEvent::JobCompleted {
            job_id: uuid::Uuid::nil(),
            job_type: "scan".into(),
            description: "Scanning".into(),
        },
        AppEvent::JobFailed {
            job_id: uuid::Uuid::nil(),
            job_type: "scan".into(),
            message: "boom".into(),
            retryable: true,
        },
        AppEvent::JobCancelled {
            job_id: uuid::Uuid::nil(),
            job_type: "scan".into(),
        },
        AppEvent::SourceInstalled {
            source_id: 1,
            source_name: "Src".into(),
            from_repo: "repo".into(),
        },
        AppEvent::RepoRefreshed {
            repo_id: 1,
            repo_name: "repo".into(),
        },
        AppEvent::UpdateAvailable {
            source_id: 1,
            source_name: "Src".into(),
            installed_version: "1.0.0".into(),
            available_version: "1.1.0".into(),
            repo_id: 1,
        },
        AppEvent::SourceUpdating {
            source_id: 1,
            source_name: "Src".into(),
        },
        AppEvent::LibraryInvalidated,
        AppEvent::CircuitOpen {
            host: "example.com".into(),
            failure_count: 5,
        },
        AppEvent::Refresh(RefreshProgressEvent::Started {
            total: 2,
            manga_ids: vec![MangaId(1), MangaId(2)],
        }),
        AppEvent::Refresh(RefreshProgressEvent::MangaRefreshed {
            manga_id: MangaId(1),
            manga_name: "Manga".into(),
            completed: 1,
            total: 2,
            success: true,
            new_chapters: 3,
        }),
        AppEvent::Refresh(RefreshProgressEvent::Completed {
            total: 2,
            failed: 0,
        }),
    ]
}

fn download_event_samples() -> Vec<DownloadProgressEvent> {
    vec![
        DownloadProgressEvent::ChapterStarted {
            chapter_id: 1,
            chapter_name: "Ch. 1".into(),
            manga_id: 1,
            manga_title: "Manga".into(),
            total_pages: 10,
            job_id: None,
        },
        DownloadProgressEvent::PageCompleted {
            chapter_id: 1,
            chapter_name: "Ch. 1".into(),
            page_index: 0,
        },
        DownloadProgressEvent::ChapterCompleted {
            chapter_id: 1,
            chapter_name: "Ch. 1".into(),
            manga_id: 1,
            manga_title: "Manga".into(),
            successful_pages: 10,
        },
        DownloadProgressEvent::ChapterFailed {
            chapter_id: 1,
            chapter_name: "Ch. 1".into(),
            error: "boom".into(),
        },
        DownloadProgressEvent::ChapterCancelled {
            chapter_id: 1,
            chapter_name: "Ch. 1".into(),
        },
    ]
}

fn expected_app_event_tag(event: &AppEvent) -> &'static str {
    match event {
        AppEvent::NewChapters { .. } => "new_chapters",
        AppEvent::ImportStarted { .. } => "import_started",
        AppEvent::ImportProgress { .. } => "import_progress",
        AppEvent::ImportCompleted { .. } => "import_completed",
        AppEvent::PathMigrationStarted { .. } => "path_migration_started",
        AppEvent::PathMigrationProgress { .. } => "path_migration_progress",
        AppEvent::PathMigrationCompleted { .. } => "path_migration_completed",
        AppEvent::PathMigrationFailed { .. } => "path_migration_failed",
        AppEvent::UpgradesFound { .. } => "upgrades_found",
        AppEvent::ChapterListPartial { .. } => "chapter_list_partial",
        AppEvent::ChapterListComplete { .. } => "chapter_list_complete",
        AppEvent::ChapterListError { .. } => "chapter_list_error",
        AppEvent::JobStarted { .. } => "job_started",
        AppEvent::JobProgress { .. } => "job_progress",
        AppEvent::JobCompleted { .. } => "job_completed",
        AppEvent::JobFailed { .. } => "job_failed",
        AppEvent::JobCancelled { .. } => "job_cancelled",
        AppEvent::SourceInstalled { .. } => "source_installed",
        AppEvent::RepoRefreshed { .. } => "repo_refreshed",
        AppEvent::UpdateAvailable { .. } => "update_available",
        AppEvent::SourceUpdating { .. } => "source_updating",
        AppEvent::LibraryInvalidated => "library_invalidated",
        AppEvent::CircuitOpen { .. } => "circuit_open",
        AppEvent::Refresh(refresh) => match refresh {
            RefreshProgressEvent::Started { .. } => "started",
            RefreshProgressEvent::MangaRefreshed { .. } => "manga_refreshed",
            RefreshProgressEvent::Completed { .. } => "completed",
        },
    }
}

fn expected_download_event_tag(event: &DownloadProgressEvent) -> &'static str {
    match event {
        DownloadProgressEvent::ChapterStarted { .. } => "chapter_started",
        DownloadProgressEvent::PageCompleted { .. } => "page_completed",
        DownloadProgressEvent::ChapterCompleted { .. } => "chapter_completed",
        DownloadProgressEvent::ChapterFailed { .. } => "chapter_failed",
        DownloadProgressEvent::ChapterCancelled { .. } => "chapter_cancelled",
    }
}

fn serialised_tag(value: &serde_json::Value) -> String {
    value
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or_else(|| panic!("event carries no string `type` tag: {value}"))
        .to_string()
}

fn is_snake_case(tag: &str) -> bool {
    !tag.is_empty()
        && !tag.starts_with('_')
        && !tag.ends_with('_')
        && !tag.contains("__")
        && tag
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn emitted_event_tags() -> BTreeSet<String> {
    let mut tags: BTreeSet<String> = app_event_samples()
        .iter()
        .map(|e| serialised_tag(&serde_json::to_value(e).unwrap()))
        .collect();
    tags.extend(
        download_event_samples()
            .iter()
            .map(|e| serialised_tag(&serde_json::to_value(e).unwrap())),
    );
    tags.extend(HAND_ROLLED_FRAMES.iter().map(|s| (*s).to_string()));
    tags
}

fn frontend_js_files() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../static/js");
    let mut files = Vec::new();
    collect_js(&root, &mut files);
    assert!(
        files.len() > 20,
        "expected to find the frontend sources under {}, found {} files",
        root.display(),
        files.len()
    );
    files
}

fn collect_js(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .flatten();
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "dist") {
                continue;
            }
            collect_js(&path, out);
        } else if path.extension().is_some_and(|e| e == "js") {
            out.push(path);
        }
    }
}

fn quoted_after(haystack: &str, marker: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = haystack[cursor..].find(marker) {
        let at = cursor + rel;
        let rest = &haystack[at + marker.len()..];
        let rest_trimmed = rest.trim_start();
        let skipped = rest.len() - rest_trimmed.len();
        if let Some(inner) = rest_trimmed.strip_prefix('\'')
            && let Some(end) = inner.find('\'')
        {
            found.push((at, inner[..end].to_string()));
        }
        cursor = at + marker.len() + skipped;
    }
    found
}

fn frontend_subscribed_tags() -> BTreeSet<String> {
    let mut tags = BTreeSet::new();
    for path in frontend_js_files() {
        let src = std::fs::read_to_string(&path).unwrap();
        tags.extend(subscribed_tags_in(&src));
    }
    tags
}

fn subscribed_tags_in(src: &str) -> BTreeSet<String> {
    let mut tags = BTreeSet::new();
    for (at, literal) in quoted_after(src, "type ===") {
        let preceding = &src[..at];
        if preceding.ends_with("typeof ") {
            continue;
        }
        if preceding
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$')
        {
            continue;
        }
        tags.insert(literal);
    }
    for (_, literal) in quoted_after(src, "useSSE(") {
        tags.insert(literal);
    }
    tags
}

#[test]
fn every_app_event_serialises_to_a_snake_case_type_tag() {
    for event in app_event_samples() {
        let value = serde_json::to_value(&event).unwrap();
        let tag = serialised_tag(&value);
        assert_eq!(
            tag,
            expected_app_event_tag(&event),
            "wire tag drifted for {event:?}"
        );
        assert!(is_snake_case(&tag), "tag `{tag}` is not snake_case");
    }
    for event in download_event_samples() {
        let value = serde_json::to_value(&event).unwrap();
        let tag = serialised_tag(&value);
        assert_eq!(
            tag,
            expected_download_event_tag(&event),
            "wire tag drifted for {event:?}"
        );
        assert!(is_snake_case(&tag), "tag `{tag}` is not snake_case");
    }
    for frame in HAND_ROLLED_FRAMES {
        assert!(is_snake_case(frame), "tag `{frame}` is not snake_case");
    }
}

#[test]
fn every_event_variant_has_a_sample() {
    let app_tags: BTreeSet<&str> = app_event_samples()
        .iter()
        .map(expected_app_event_tag)
        .collect();
    assert_eq!(
        app_tags.len(),
        app_event_samples().len(),
        "two samples share a wire tag, so a variant is missing from app_event_samples()"
    );

    let download_tags: BTreeSet<&str> = download_event_samples()
        .iter()
        .map(expected_download_event_tag)
        .collect();
    assert_eq!(
        download_tags.len(),
        download_event_samples().len(),
        "two samples share a wire tag, so a variant is missing from download_event_samples()"
    );
}

#[test]
fn the_declared_frontend_prefix_handlers_exist() {
    let sources: String = frontend_js_files()
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect();
    for prefix in FRONTEND_PREFIX_HANDLERS {
        assert!(
            sources.contains(&format!("startsWith('{prefix}')")),
            "no frontend handler matches events by the declared prefix `{prefix}` — \
             remove it from FRONTEND_PREFIX_HANDLERS or restore the handler"
        );
    }
}

#[test]
fn the_subscription_scanner_reads_handlers_and_not_lookalikes() {
    let src = r#"
      useSSE('job_progress', (ev) => {});
      if (data.type === 'new_chapters') return;
      if (ev?.job_type === 'integrity_scrub') return;
      if (typeof type === 'string') return;
      if (state.type === 'running') return;
    "#;
    let found = subscribed_tags_in(src);
    assert!(
        found.contains("job_progress"),
        "missed a useSSE subscription"
    );
    assert!(
        found.contains("new_chapters"),
        "missed a `.type ===` handler"
    );
    assert!(
        found.contains("running"),
        "the scanner must surface every `.type ===` literal so a non-SSE one has to be declared"
    );
    assert!(
        !found.contains("integrity_scrub"),
        "`job_type ===` is a job type, not an event tag"
    );
    assert!(
        !found.contains("string"),
        "`typeof x === 'string'` is not an event tag"
    );
}

#[test]
fn the_frontend_scan_reaches_the_real_sources() {
    let found = frontend_subscribed_tags();
    for tag in ["new_chapters", "state_snapshot", "job_completed"] {
        assert!(
            found.contains(tag),
            "the scan of static/js found no subscription to `{tag}`, so the contract check is vacuous"
        );
    }
}

fn unhandled_events(emitted: &BTreeSet<String>, subscribed: &BTreeSet<String>) -> Vec<String> {
    emitted
        .iter()
        .filter(|tag| !subscribed.contains(*tag))
        .filter(|tag| {
            !FRONTEND_PREFIX_HANDLERS
                .iter()
                .any(|prefix| tag.starts_with(prefix))
        })
        .cloned()
        .collect()
}

#[test]
fn the_contract_check_rejects_a_renamed_event() {
    let emitted: BTreeSet<String> = ["new_chapters", "job_started"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let subscribed: BTreeSet<String> = ["new_chapters"].iter().map(|s| (*s).to_string()).collect();
    assert!(
        unhandled_events(&emitted, &subscribed).is_empty(),
        "a prefix-handled event must not be reported as unhandled"
    );

    let renamed: BTreeSet<String> = ["new_chapter", "job_started"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(
        unhandled_events(&renamed, &subscribed),
        vec!["new_chapter".to_string()],
        "renaming an emitted event must be reported"
    );
}

#[test]
fn the_emitted_event_names_match_the_frontend_subscriptions() {
    let emitted = emitted_event_tags();
    let subscribed = frontend_subscribed_tags();

    let unhandled = unhandled_events(&emitted, &subscribed);
    assert!(
        unhandled.is_empty(),
        "these events reach the browser and nothing in static/js reacts to them: {unhandled:?}"
    );

    let stale: Vec<&String> = subscribed
        .iter()
        .filter(|tag| !emitted.contains(*tag))
        .filter(|tag| !NON_SSE_TYPE_DISCRIMINANTS.contains(&tag.as_str()))
        .collect();
    assert!(
        stale.is_empty(),
        "static/js subscribes to event names the server never emits: {stale:?}"
    );
}

#[tokio::test]
async fn a_subscriber_receives_a_well_formed_event_payload() {
    let state = common::test_state().await;
    let (username, password) = common::create_admin(&state).await;
    let app = common::build_test_app(state.clone()).await;
    let cookie = common::login(&app, username, password).await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/rest/events")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream")),
        "the SSE route must announce itself as an event stream"
    );

    let mut stream = response.into_body().into_data_stream();

    let snapshot = next_frame(&mut stream).await;
    assert_eq!(serialised_tag(&snapshot), "state_snapshot");
    for field in ["chapters", "is_refreshing", "active_jobs"] {
        assert!(
            snapshot.get(field).is_some(),
            "state_snapshot is missing `{field}`: {snapshot}"
        );
    }

    state.service.invalidate_library();

    let event = next_frame(&mut stream).await;
    assert_eq!(serialised_tag(&event), "library_invalidated");

    let _ = state.service.refresh_tx.send(AppEvent::UpgradesFound {
        manga_id: 7,
        count: 3,
    });

    let upgrades = next_frame(&mut stream).await;
    assert_eq!(serialised_tag(&upgrades), "upgrades_found");
    assert_eq!(upgrades.get("manga_id").and_then(|v| v.as_i64()), Some(7));
    assert_eq!(upgrades.get("count").and_then(|v| v.as_u64()), Some(3));
}

async fn next_frame<S>(stream: &mut S) -> serde_json::Value
where
    S: futures::Stream<Item = Result<axum::body::Bytes, axum::Error>> + Unpin,
{
    let deadline = std::time::Duration::from_secs(5);
    loop {
        let chunk = tokio::time::timeout(deadline, stream.next())
            .await
            .expect("timed out waiting for an SSE frame")
            .expect("SSE stream ended early")
            .unwrap();
        let text = String::from_utf8(chunk.to_vec()).unwrap();
        for line in text.lines() {
            if let Some(payload) = line.strip_prefix("data:") {
                let payload = payload.trim();
                if payload.is_empty() {
                    continue;
                }
                return serde_json::from_str(payload)
                    .unwrap_or_else(|e| panic!("frame is not JSON ({e}): {payload}"));
            }
        }
    }
}
