//! Server-sent events and boot-id routes.

use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/events", get(combined_sse))
        .route("/boot_id", get(get_boot_id))
}

pub(super) async fn combined_sse(
    AuthGuard(..): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let snapshot = state.downloader.snapshot().await;
    let is_refreshing = state.is_refreshing().await;
    let active_jobs = state.job_manager.active_job_summaries();

    let snapshot_event = Ok::<Event, Infallible>(
        Event::default().data(
            serde_json::json!({
                "type": "state_snapshot",
                "chapters": snapshot,
                "is_refreshing": is_refreshing,
                "active_jobs": active_jobs,
            })
            .to_string(),
        ),
    );

    let download_rx = state.downloader.subscribe();
    let refresh_rx = state.subscribe_refresh();

    let download_stream = BroadcastStream::new(download_rx).filter_map(|result| match result {
        Ok(event) => {
            let json = serde_json::to_string(&event).ok()?;
            Some(Ok::<Event, Infallible>(Event::default().data(json)))
        }
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
            tracing::warn!("Download SSE lagged by {} events", n);
            Some(Ok(Event::default().event("close").data("")))
        }
    });

    let refresh_stream = BroadcastStream::new(refresh_rx).filter_map(|result| match result {
        Ok(event) => {
            let json = serde_json::to_string(&event).ok()?;
            Some(Ok::<Event, Infallible>(Event::default().data(json)))
        }
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
            tracing::warn!("Refresh SSE lagged by {} events", n);
            None
        }
    });

    let guard = SseClientGuard::connected();
    let live_stream = download_stream.merge(refresh_stream);
    let stream = tokio_stream::once(snapshot_event)
        .chain(live_stream)
        .map(move |event| {
            let _connected = &guard;
            event
        });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

struct SseClientGuard;

impl SseClientGuard {
    fn connected() -> Self {
        metrics::gauge!("kani_sse_clients").increment(1.0);
        Self
    }
}

impl Drop for SseClientGuard {
    fn drop(&mut self) {
        metrics::gauge!("kani_sse_clients").decrement(1.0);
    }
}

#[utoipa::path(
    get, path = "/rest/boot_id",
    responses(
        (status = 200, description = "This server process's boot id, which clients compare to detect a restart"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "system"
)]
pub(super) async fn get_boot_id(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    Json(json!({ "boot_id": state.boot_id }))
}
