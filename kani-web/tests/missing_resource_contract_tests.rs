#![allow(clippy::unwrap_used)]

//! Naming a resource that does not exist answers 404.
//!
//! Fifteen tests asserted this one endpoint at a time, and two of them were the
//! same test in two files. The contract is worth holding because both ways of
//! breaking it are silent: a handler that unwraps a missing row answers 500 and
//! looks like an outage, and one that treats "no rows" as success answers 200
//! and looks like the resource was deleted.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{admin_app, csrf_cookie, csrf_token};
use tower::ServiceExt;

/// `(method, path, body)` naming an id that cannot exist. `1` is used where the
/// outer id must be plausible for the inner one to be reached, and a body is
/// given where an empty one would fail validation before the lookup.
const MISSING: &[(&str, &str, &str)] = &[
    ("GET", "/rest/manga/999999", "{}"),
    ("DELETE", "/rest/manga/999999", "{}"),
    ("POST", "/rest/manga/999999/untrash", "{}"),
    ("POST", "/rest/manga/999999/refresh", "{}"),
    (
        "POST",
        "/rest/manga/999999/enrich-metadata",
        r#"{"provider":"stub"}"#,
    ),
    ("GET", "/rest/sources/999999", "{}"),
    ("GET", "/rest/sources/999999/capabilities", "{}"),
    ("GET", "/rest/sources/repos/99999", "{}"),
    ("GET", "/rest/collections/999999/manga", "{}"),
    ("GET", "/rest/chapters/999999/export/epub", "{}"),
    ("DELETE", "/rest/saved-searches/999999", "{}"),
    ("DELETE", "/rest/trash/999999", "{}"),
    ("DELETE", "/rest/manga/1/volumes/999999", "{}"),
    (
        "PUT",
        "/rest/manga/1/chapters/999999/volume",
        r#"{"volume_id":null}"#,
    ),
];

fn request(method: &str, path: &str, body: &str, cookie: &str) -> Request<Body> {
    let builder = Request::builder().method(method).uri(path);
    if matches!(method, "POST" | "PUT" | "PATCH" | "DELETE") {
        builder
            .header("Content-Type", "application/json")
            .header("Cookie", csrf_cookie(cookie))
            .header("X-CSRF-Token", csrf_token(cookie))
            .body(Body::from(body.to_owned()))
            .unwrap()
    } else {
        builder
            .header("Cookie", cookie)
            .body(Body::empty())
            .unwrap()
    }
}

#[tokio::test]
async fn naming_a_resource_that_does_not_exist_answers_404() {
    let (app, cookie) = admin_app().await;

    let mut wrong = Vec::new();
    for (method, path, body) in MISSING {
        let res = app
            .clone()
            .oneshot(request(method, path, body, &cookie))
            .await
            .unwrap();
        if res.status() != StatusCode::NOT_FOUND {
            wrong.push(format!("  {method} {path} -> {}", res.status()));
        }
    }

    wrong.sort();
    assert!(
        wrong.is_empty(),
        "{} request(s) for a missing resource did not answer 404. A 5xx means the handler \
         unwrapped a missing row; a 2xx means it treated 'no rows' as success:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
