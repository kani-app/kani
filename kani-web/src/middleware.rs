//! Transport middleware shared by the main router.

/// Request-ID generation and propagation.
pub mod trace_id {
    use axum::http::{HeaderValue, Request};
    use tower_http::request_id::{MakeRequestId, RequestId};

    #[derive(Clone, Copy, Default)]
    /// Generates a fresh UUID v4 request identifier for each request.
    pub struct UuidRequestId;

    impl MakeRequestId for UuidRequestId {
        fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
            let id = uuid::Uuid::new_v4().to_string();
            HeaderValue::from_str(&id).ok().map(RequestId::new)
        }
    }

    #[cfg(test)]
    mod tests {
        #![allow(clippy::unwrap_used)]
        use super::*;

        fn make_id() -> String {
            let mut maker = UuidRequestId;
            let req = Request::builder().body(()).unwrap();
            let id = maker.make_request_id(&req).unwrap();
            id.header_value().to_str().unwrap().to_string()
        }

        #[test]
        fn make_request_id_yields_uuid_of_expected_shape() {
            let id = make_id();
            assert_eq!(id.len(), 36, "uuid v4 hyphenated is 36 chars, got {id}");
            assert_eq!(id.matches('-').count(), 4);
        }

        #[test]
        fn make_request_id_is_unique_per_call() {
            assert_ne!(make_id(), make_id());
        }
    }
}
