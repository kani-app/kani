//! Reading statistics routes.

use super::*;

pub fn router() -> Router<AppState> {
    Router::new().route("/stats", get(reading_stats))
}

#[utoipa::path(
    get, path = "/rest/stats",
    params(
        ("period" = Option<i32>, Query, description = "Rolling window in days (default 90, max 365)"),
    ),
    responses(
        (status = 200, description = "Reading statistics: pages read, chapters read, daily activity"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn reading_stats(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
    ValidatedQuery(q): ValidatedQuery<crate::models::StatsQuery>,
) -> Result<impl IntoResponse, AppError> {
    let period = q.period.unwrap_or(90);
    let stats = state.get_reading_stats(user.id, period).await?;
    Ok(Json((*stats).clone()))
}
