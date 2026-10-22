//! Background job management routes.

use super::*;
use kani_app::service::traits::{JobDomain, JobListFilter};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/jobs", get(list_jobs))
        .route("/jobs/{id}", get(get_job).delete(cancel_job))
        .route("/jobs/{id}/pause", post(pause_job))
        .route("/jobs/{id}/resume", post(resume_job))
}

#[derive(serde::Deserialize, Default)]
pub(super) struct JobsQuery {
    pub job_type: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// `status` accepts a comma-separated list so the UI can ask for a tab's whole
/// status group in one request (e.g. `status=pending,running`).
fn parse_statuses(raw: Option<&str>) -> Vec<String> {
    raw.map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    })
    .unwrap_or_default()
}

#[utoipa::path(
    get, path = "/rest/jobs",
    params(("job_type" = Option<String>, Query, description = "Restrict to one job type"), ("status" = Option<String>, Query, description = "Comma-separated status group, e.g. pending,running"), ("limit" = Option<i64>, Query, description = "Page size, clamped to 1..=200 (default 50)"), ("offset" = Option<i64>, Query, description = "Rows to skip")),
    responses(
        (status = 200, description = "A page of background jobs"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "admin"
)]
pub(super) async fn list_jobs(
    _: AuthGuard<crate::permissions::guards::AdminJobs>,
    State(svc): State<Arc<dyn JobDomain>>,
    Query(q): Query<JobsQuery>,
) -> Result<impl IntoResponse, AppError> {
    let filter = JobListFilter {
        job_type: q.job_type.filter(|s| !s.is_empty()),
        statuses: parse_statuses(q.status.as_deref()),
        limit: q.limit.unwrap_or(50).clamp(1, 200),
        offset: q.offset.unwrap_or(0).max(0),
        user_id: None,
    };
    let page = svc.list_jobs(filter).await?;
    Ok(Json(page))
}

#[utoipa::path(
    get, path = "/rest/jobs/{id}",
    params(("id" = String, Path, description = "Job id (UUID)")),
    responses(
        (status = 200, description = "One job's status and progress"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "No such job"),
    ),
    security(("session" = [])),
    tag = "admin"
)]
pub(super) async fn get_job(
    _: AuthGuard<crate::permissions::guards::AdminJobs>,
    State(svc): State<Arc<dyn JobDomain>>,
    Path(id): Path<uuid::Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let status = svc.get_job_status(id).await?;
    Ok(Json(status))
}

#[utoipa::path(
    delete, path = "/rest/jobs/{id}",
    params(("id" = String, Path, description = "Job id (UUID)")),
    responses(
        (status = 200, description = "Job cancelled"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "No such job"),
    ),
    security(("session" = [])),
    tag = "admin"
)]
pub(super) async fn cancel_job(
    _: AuthGuard<crate::permissions::guards::AdminJobs>,
    State(svc): State<Arc<dyn JobDomain>>,
    Path(id): Path<uuid::Uuid>,
) -> Result<impl IntoResponse, AppError> {
    svc.cancel_job(id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(
    post, path = "/rest/jobs/{id}/pause",
    params(("id" = String, Path, description = "Job id (UUID)")),
    responses(
        (status = 200, description = "Job paused"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "No such job"),
    ),
    security(("session" = [])),
    tag = "admin"
)]
pub(super) async fn pause_job(
    _: AuthGuard<crate::permissions::guards::AdminJobs>,
    State(svc): State<Arc<dyn JobDomain>>,
    Path(id): Path<uuid::Uuid>,
) -> Result<impl IntoResponse, AppError> {
    svc.pause_job(id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(
    post, path = "/rest/jobs/{id}/resume",
    params(("id" = String, Path, description = "Job id (UUID)")),
    responses(
        (status = 200, description = "Job resumed"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "No such job"),
    ),
    security(("session" = [])),
    tag = "admin"
)]
pub(super) async fn resume_job(
    _: AuthGuard<crate::permissions::guards::AdminJobs>,
    State(svc): State<Arc<dyn JobDomain>>,
    Path(id): Path<uuid::Uuid>,
) -> Result<impl IntoResponse, AppError> {
    svc.resume_job(id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::parse_statuses;

    #[test]
    fn parse_statuses_splits_comma_list() {
        assert_eq!(
            parse_statuses(Some("pending,running")),
            vec!["pending".to_string(), "running".to_string()]
        );
    }

    #[test]
    fn parse_statuses_trims_and_drops_empties() {
        assert_eq!(
            parse_statuses(Some("failed, ,cancelled,")),
            vec!["failed".to_string(), "cancelled".to_string()]
        );
    }

    #[test]
    fn parse_statuses_none_is_empty() {
        assert!(parse_statuses(None).is_empty());
        assert!(parse_statuses(Some("")).is_empty());
    }
}
