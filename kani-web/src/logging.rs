//! Bounded in-memory log capture with query and SSE subscription support.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tracing::{Event, Subscriber};
use tracing_subscriber::{Layer, layer::Context};

#[derive(Clone, serde::Serialize)]
/// Transport representation of one captured tracing or injected log event.
pub struct LogEntry {
    /// UTC timestamp encoded as RFC 3339.
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
    /// Free-text source tag. "app" = tracing events; future sources ("http",
    /// "extension") can inject entries via `LogHandle::push()` directly.
    pub source: String,
}

/// Shared query, injection, clearing, and subscription handle for the log ring buffer.
pub struct LogHandle {
    buffer: Arc<Mutex<VecDeque<LogEntry>>>,
    capacity: usize,
    broadcast_tx: broadcast::Sender<LogEntry>,
}

impl LogHandle {
    /// Appends an entry to the ring buffer and broadcasts it to SSE subscribers.
    /// Call this directly to inject entries from non-tracing sources.
    pub fn push(&self, entry: LogEntry) {
        let _ = self.broadcast_tx.send(entry.clone());
        let mut buf = self.buffer.lock().unwrap_or_else(|e| e.into_inner());
        if buf.len() >= self.capacity {
            buf.pop_front();
        }
        buf.push_back(entry);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LogEntry> {
        self.broadcast_tx.subscribe()
    }

    /// Returns (entries, total_matching_count), newest-first within the page.
    #[allow(clippy::too_many_arguments)]
    pub fn query(
        &self,
        level_filter: &[String],
        source_filter: &[String],
        from: Option<&str>,
        to: Option<&str>,
        search: Option<&str>,
        page: usize,
        page_size: usize,
    ) -> (Vec<LogEntry>, usize) {
        let buf = self.buffer.lock().unwrap_or_else(|e| e.into_inner());

        let filtered: Vec<&LogEntry> = buf
            .iter()
            .filter(|e| {
                if !level_filter.is_empty()
                    && !level_filter
                        .iter()
                        .any(|l| l.eq_ignore_ascii_case(&e.level))
                {
                    return false;
                }
                if !source_filter.is_empty()
                    && !source_filter
                        .iter()
                        .any(|s| s.eq_ignore_ascii_case(&e.source))
                {
                    return false;
                }
                if let Some(s) = search
                    && !s.is_empty()
                {
                    let s_lower = s.to_lowercase();
                    if !e.message.to_lowercase().contains(&s_lower)
                        && !e.target.to_lowercase().contains(&s_lower)
                    {
                        return false;
                    }
                }
                if let Some(from_str) = from
                    && !from_str.is_empty()
                    && e.timestamp.as_str() < from_str
                {
                    return false;
                }
                if let Some(to_str) = to
                    && !to_str.is_empty()
                    && e.timestamp.as_str() > to_str
                {
                    return false;
                }
                true
            })
            .collect();

        let total = filtered.len();
        let start = page.saturating_sub(1) * page_size;
        let entries = filtered
            .into_iter()
            .rev()
            .skip(start)
            .take(page_size)
            .cloned()
            .collect();

        (entries, total)
    }

    /// Clears all entries from the ring buffer.
    pub fn clear(&self) {
        let mut buf = self.buffer.lock().unwrap_or_else(|e| e.into_inner());
        buf.clear();
    }

    /// Returns all matching entries, newest-first, with no pagination.
    pub(crate) fn query_all(
        &self,
        level_filter: &[String],
        source_filter: &[String],
        from: Option<&str>,
        to: Option<&str>,
        search: Option<&str>,
    ) -> Vec<LogEntry> {
        let (entries, _) = self.query(level_filter, source_filter, from, to, search, 1, usize::MAX);
        entries
    }
}

/// Tracing subscriber layer that copies events into a [`LogHandle`].
pub struct RingBufferLayer {
    handle: Arc<LogHandle>,
}

impl RingBufferLayer {
    pub fn new(capacity: usize) -> (Self, Arc<LogHandle>) {
        let (tx, _rx) = broadcast::channel(256);
        let handle = Arc::new(LogHandle {
            buffer: Arc::new(Mutex::new(VecDeque::new())),
            capacity,
            broadcast_tx: tx,
        });
        (
            Self {
                handle: handle.clone(),
            },
            handle,
        )
    }
}

struct MessageVisitor(String);

impl tracing::field::Visit for MessageVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl<S: Subscriber> Layer<S> for RingBufferLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() == tracing::Level::ERROR {
            metrics::counter!("kani_log_errors_total").increment(1);
        }

        let mut visitor = MessageVisitor(String::new());
        event.record(&mut visitor);

        let timestamp = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        self.handle.push(LogEntry {
            timestamp,
            level: event.metadata().level().to_string().to_uppercase(),
            target: event.metadata().target().to_string(),
            message: visitor.0,
            source: "app".to_string(),
        });
    }
}
