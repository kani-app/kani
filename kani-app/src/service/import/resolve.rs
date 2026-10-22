//! Links imported manga to ids their source actually accepts.
//!
//! A Mihon backup identifies a manga by that app's own URL path, which no Kani
//! extension can address: MangaDex wants a bare UUID, WeebCentral a series id,
//! a composite-id source an encoded field pair. Every id here is therefore
//! proved against the source before it is stored — a value merely shaped like
//! an id is what produces an empty chapter list later.

use crate::error::{Result, ServiceError};
use crate::ids::MangaId;
use crate::service::AppService;
use crate::service::dedup::normalise_title;
use kani_core::wasm::kani::extension::types::MangaList;

/// How many search results to consider before giving up on a title.
const CANDIDATE_LIMIT: i32 = 20;

/// How many search attempts one manga may cost. Each is a full source request,
/// which for a browser-backed source is seconds.
const MAX_SEARCH_QUERIES: usize = 3;

#[derive(Debug, PartialEq, Eq)]
pub enum Resolution {
    /// The stored id already addresses the manga.
    AlreadyValid,
    /// The stored id was replaced with one the source accepts.
    Relinked(String),
    /// The source could not answer; the manga stays queued for another attempt.
    Deferred(String),
    /// No candidate could be proved; the manga keeps its imported id.
    Unresolved(String),
}

enum Probe {
    Valid,
    Invalid,
    Transient(String),
}

/// Decides whether a source failure says "this id is wrong" or "ask again later".
///
/// Anything unrecognised counts as transient: a few wasted retries cost less
/// than marking a working manga unlinked because the solver was restarting.
fn is_transient(e: &kani_core::Error) -> bool {
    use kani_core::Error as E;
    use kani_shared::extension::ExtensionErrorKind as K;
    match e {
        E::NotFound(_) => false,
        E::Extension(inner) => matches!(
            inner.kind,
            K::Network | K::RateLimited | K::Timeout | K::Updating
        ),
        E::HttpStatus { status, .. } => !matches!(status, 400 | 401 | 403 | 404 | 410),
        _ => true,
    }
}

/// As [`is_transient`], for the service layer the search now goes through.
///
/// A disabled source is the case worth naming: it says nothing about the id, so
/// switching a source off for a while must not write off its whole import queue.
fn is_transient_service(e: &ServiceError) -> bool {
    match e {
        ServiceError::Core(inner) => is_transient(inner),
        ServiceError::Validation(_) | ServiceError::Conflict(_) | ServiceError::Forbidden(_) => {
            false
        }
        _ => true,
    }
}

/// The queries to try for `title`, widest first, at most [`MAX_SEARCH_QUERIES`].
///
/// A source that indexes a work under a shorter name than the backup carries
/// returns nothing for the full string, so each rung drops one thing a title can
/// carry that a search index usually will not: a trailing parenthetical, a
/// subtitle after a separator, then punctuation. The title gate still judges
/// every result, so a wider query cannot loosen what counts as a match.
fn search_queries(title: &str) -> Vec<String> {
    fn push(queries: &mut Vec<String>, candidate: &str) {
        let candidate = candidate.trim();
        if candidate.is_empty() || queries.len() >= MAX_SEARCH_QUERIES {
            return;
        }
        // Compared case-insensitively rather than normalised: the punctuation a
        // rung strips is the whole reason that rung is worth a request.
        let key = candidate.to_lowercase();
        if normalise_title(candidate).is_empty() || queries.iter().any(|q| q.to_lowercase() == key)
        {
            return;
        }
        queries.push(candidate.to_string());
    }

    let no_parenthetical = title
        .split_once('(')
        .map(|(head, _)| head)
        .unwrap_or(title)
        .trim();
    let head = [" - ", ": ", " ~ "]
        .iter()
        .filter_map(|sep| no_parenthetical.split_once(sep).map(|(head, _)| head))
        .min_by_key(|head| head.len())
        .unwrap_or(no_parenthetical);

    let mut queries: Vec<String> = Vec::new();
    push(&mut queries, title);
    push(&mut queries, no_parenthetical);
    push(&mut queries, head);
    push(&mut queries, &normalise_title(head));
    queries
}

impl AppService {
    /// Queues a resolve pass over the manga still waiting on a usable id.
    ///
    /// `source_id` narrows the pass to one source; `None` sweeps every source,
    /// which is how a restart picks the queue back up. An equivalent pass that
    /// is already queued or running is reused rather than duplicated.
    pub async fn queue_import_resolve(&self, source_id: Option<i64>) -> Result<uuid::Uuid> {
        if let Some(existing) = self.active_import_resolve_job(source_id).await {
            return Ok(existing);
        }
        let job = crate::jobs::import_resolve::ImportResolveJob::new(source_id);
        self.job_manager
            .submit(job)
            .await
            .map_err(|e| crate::error::ServiceError::Internal(e.to_string()))
    }

    /// An in-flight pass that already covers `source_id`, if there is one.
    async fn active_import_resolve_job(&self, source_id: Option<i64>) -> Option<uuid::Uuid> {
        sqlx::query_scalar!(
            "SELECT id FROM jobs WHERE job_type = 'import_resolve' \
             AND status IN ('pending', 'running') \
             AND (?1 IS NULL \
                  OR json_extract(params_json, '$.source_id') IS NULL \
                  OR json_extract(params_json, '$.source_id') = ?1) \
             LIMIT 1",
            source_id
        )
        .fetch_optional(&self.db_read)
        .await
        .ok()
        .flatten()
        .and_then(|id| id.parse().ok())
    }

    /// Counts the manga still queued for linking, for the pass scheduler.
    pub async fn pending_import_link_count(&self) -> i64 {
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM manga_import_links l JOIN manga m ON m.id = l.manga_id \
             WHERE l.status = 'pending' AND m.deleted_at IS NULL"
        )
        .fetch_one(&self.db_read)
        .await
        .unwrap_or(0)
    }

    /// Probes an id against the source without recording source health.
    ///
    /// A rejected id says nothing about whether the source is working, so these
    /// probes are kept out of the health signal that scanning and browsing feed.
    async fn probe_manga_id(&self, source_id: i64, manga_id: &str) -> Probe {
        let Some(backend) = self.sources.get_backend(source_id) else {
            return Probe::Transient("source is not loaded".to_string());
        };
        match backend.get_manga_details(manga_id).await {
            Ok(_) => Probe::Valid,
            Err(e) if is_transient(&e) => Probe::Transient(e.to_string()),
            Err(_) => Probe::Invalid,
        }
    }

    async fn set_link_status(&self, manga_row_id: MangaId, status: Option<&str>) -> Result<()> {
        match status {
            Some(status) => {
                sqlx::query(
                    "INSERT OR REPLACE INTO manga_import_links (manga_id, status) VALUES (?, ?)",
                )
                .bind(manga_row_id.0)
                .bind(status)
                .execute(&self.db)
                .await?;
            }
            None => {
                sqlx::query("DELETE FROM manga_import_links WHERE manga_id = ?")
                    .bind(manga_row_id.0)
                    .execute(&self.db)
                    .await?;
            }
        }
        Ok(())
    }

    /// Finds an id for `title` that the source accepts, or explains why it could not.
    ///
    /// Search supplies candidates and the title gate rejects the ones that are
    /// not the same work; the probe then proves the survivor addresses something
    /// real. Both have to pass, because search alone can rank a near-miss first
    /// and a probe alone cannot tell one manga from another.
    pub async fn resolve_imported_manga(
        &self,
        source_id: i64,
        manga_row_id: MangaId,
        stored_id: &str,
        title: &str,
    ) -> Result<Resolution> {
        match self.probe_manga_id(source_id, stored_id).await {
            Probe::Valid => {
                self.set_link_status(manga_row_id, None).await?;
                return Ok(Resolution::AlreadyValid);
            }
            Probe::Transient(reason) => return Ok(Resolution::Deferred(reason)),
            Probe::Invalid => {}
        }

        if self.sources.get_backend(source_id).is_none() {
            return Ok(Resolution::Deferred("source is not loaded".to_string()));
        }

        let wanted = normalise_title(title);
        let mut candidate = None;
        let mut seen_titles: Vec<String> = Vec::new();

        for query in search_queries(title) {
            // Through the service rather than the backend: it merges each filter
            // group's declared default into the request, which is what makes
            // this search return what the same search in the UI returns.
            let raw = match self
                .search_manga(source_id, &query, 1, CANDIDATE_LIMIT, None)
                .await
            {
                Ok(raw) => raw,
                Err(e) if is_transient_service(&e) => {
                    return Ok(Resolution::Deferred(format!("search failed: {e}")));
                }
                Err(e) => {
                    self.set_link_status(manga_row_id, Some("unlinked")).await?;
                    return Ok(Resolution::Unresolved(format!("search failed: {e}")));
                }
            };
            let results = match serde_json::from_str::<MangaList>(&raw) {
                Ok(list) => list.manga,
                Err(e) => {
                    return Ok(Resolution::Deferred(format!("search was unreadable: {e}")));
                }
            };

            seen_titles.extend(results.iter().map(|m| m.title.clone()));
            candidate = results
                .into_iter()
                .find(|m| normalise_title(&m.title) == wanted);
            if candidate.is_some() {
                break;
            }
        }

        let Some(candidate) = candidate else {
            self.set_link_status(manga_row_id, Some("unlinked")).await?;
            tracing::info!(
                manga_id = manga_row_id.0,
                title,
                rejected = ?seen_titles,
                "no search result matched the title"
            );
            return Ok(Resolution::Unresolved(
                "no search result matched the title".to_string(),
            ));
        };

        match self.probe_manga_id(source_id, &candidate.id).await {
            Probe::Valid => {}
            Probe::Transient(reason) => return Ok(Resolution::Deferred(reason)),
            Probe::Invalid => {
                self.set_link_status(manga_row_id, Some("unlinked")).await?;
                return Ok(Resolution::Unresolved(
                    "the matching search result could not be opened".to_string(),
                ));
            }
        }

        sqlx::query("UPDATE manga SET source_manga_id = ? WHERE id = ?")
            .bind(&candidate.id)
            .bind(manga_row_id.0)
            .execute(&self.db)
            .await?;
        self.set_link_status(manga_row_id, None).await?;

        Ok(Resolution::Relinked(candidate.id))
    }
}

#[cfg(test)]
mod tests {
    use super::is_transient;
    use kani_core::Error;
    use kani_shared::extension::{ExtensionError, ExtensionErrorKind};

    fn ext(kind: ExtensionErrorKind, message: &str) -> Error {
        Error::Extension(ExtensionError {
            kind,
            message: message.to_string(),
            source_url: None,
            retry_after_secs: None,
        })
    }

    #[test]
    fn solver_and_network_failures_are_transient() {
        assert!(is_transient(&Error::BrowserCaptureUnavailable {
            code: "solver_unreachable".to_string(),
            message: "connection refused".to_string(),
        }));
        assert!(is_transient(&Error::HttpStatus {
            status: 503,
            retry_after_secs: None,
            context: String::new(),
        }));
        assert!(is_transient(&Error::HttpStatus {
            status: 429,
            retry_after_secs: Some(30),
            context: String::new(),
        }));
        assert!(is_transient(&Error::Internal("something else".to_string())));
        assert!(is_transient(&ext(ExtensionErrorKind::Timeout, "timed out")));
        assert!(is_transient(&ext(ExtensionErrorKind::Network, "HTTP 502")));
    }

    #[test]
    fn a_query_ladder_widens_a_title_the_source_may_not_index() {
        assert_eq!(
            super::search_queries("Ano Ko wa Kishi (Pre-Serialization)"),
            vec!["Ano Ko wa Kishi (Pre-Serialization)", "Ano Ko wa Kishi"]
        );
        assert_eq!(
            super::search_queries(
                "Bocchi the Rock! Side Story - Kikuri Hiroi's Heavy-Drinking Diary"
            ),
            vec![
                "Bocchi the Rock! Side Story - Kikuri Hiroi's Heavy-Drinking Diary",
                "Bocchi the Rock! Side Story",
                "bocchi the rock side story",
            ]
        );
        assert_eq!(
            super::search_queries("Dirty Thirty ♡ Magical Girl"),
            vec!["Dirty Thirty ♡ Magical Girl", "dirty thirty magical girl"]
        );
    }

    #[test]
    fn a_title_with_nothing_to_drop_costs_one_search() {
        assert_eq!(
            super::search_queries("Record Journey"),
            vec!["Record Journey"]
        );
        assert!(super::search_queries("").is_empty());
    }

    #[test]
    fn a_rejected_id_is_not_transient() {
        assert!(!is_transient(&ext(
            ExtensionErrorKind::Unknown,
            "Required field 'title' produced null"
        )));
        assert!(!is_transient(&ext(
            ExtensionErrorKind::Parse,
            "unresolved route placeholder `$manga.slug$`"
        )));
        assert!(!is_transient(&Error::NotFound("no such manga".to_string())));
        assert!(!is_transient(&Error::HttpStatus {
            status: 404,
            retry_after_secs: None,
            context: String::new(),
        }));
    }
}
