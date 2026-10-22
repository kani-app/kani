//! Tests for `HostState` handle management, `AllowedHost` policy, and the
//! `html`, `json`, and `utility` Host trait implementations.
//!
//! None of these tests need a live WASM guest or network access — they drive
//! `HostState` directly via `HostState::default()`.
#![allow(clippy::unwrap_used)]
#![allow(clippy::field_reassign_with_default)]

use super::kani::extension::{html, json, utility};
use super::{AllowedHost, HostState, SendHtml, StoredNode};

const SIMPLE_HTML: &str = r#"<html><body><div class="card" data-id="42"><p>Hello</p><span>World</span></div></body></html>"#;
const SIMPLE_JSON: &[u8] = br#"{"name":"Alice","age":30,"scores":[10,20,30],"active":true}"#;

#[tokio::test]
async fn handle_capacity_ok_when_empty() {
    let state = HostState::default();
    assert!(state.check_handle_capacity().is_ok());
}

#[tokio::test]
async fn handle_capacity_err_at_max() {
    let mut state = HostState::default();
    for i in 0..super::MAX_HANDLES as i32 {
        state.json_docs.insert(i, serde_json::Value::Null);
    }
    assert!(state.check_handle_capacity().is_err());
}

#[tokio::test]
async fn handle_capacity_counts_all_types() {
    let mut state = HostState::default();
    state.json_docs.insert(1, serde_json::Value::Null);
    state.html_lists.insert(2, vec![]);
    assert!(state.check_handle_capacity().is_ok());
}

#[test]
fn restricted_allows_matching_host() {
    let mut state = HostState::default();
    state.allowed_host = AllowedHost::Restricted("example.com".to_string());
    assert!(
        state
            .check_allowed_url(&url("https://example.com/"))
            .is_ok()
    );
}

#[test]
fn restricted_rejects_different_host() {
    let mut state = HostState::default();
    state.allowed_host = AllowedHost::Restricted("example.com".to_string());
    let err = state
        .check_allowed_url(&url("https://other.com/"))
        .unwrap_err();
    assert!(err.contains("blocked"));
}

#[test]
fn unrestricted_allows_any_host() {
    let mut state = HostState::default();
    state.allowed_host = AllowedHost::Unrestricted;
    assert!(
        state
            .check_allowed_url(&url("https://anything.example.com/"))
            .is_ok()
    );
    assert!(state.check_allowed_url(&url("https://evil.com/")).is_ok());
}

fn url(s: &str) -> url::Url {
    s.parse().unwrap()
}

#[test]
fn unrestricted_still_refuses_non_http_schemes() {
    let mut state = HostState::default();
    state.allowed_host = AllowedHost::Unrestricted;
    for bad in ["ftp://example.com/x", "file://localhost/etc/passwd"] {
        let err = state.check_allowed_url(&url(bad)).unwrap_err();
        assert!(err.contains("only http and https"), "{bad}: {err}");
    }
}

#[test]
fn metadata_only_rejects_all() {
    let state = HostState::default();
    let err = state
        .check_allowed_url(&url("https://example.com/"))
        .unwrap_err();
    assert!(err.contains("not permitted"));
}

#[test]
fn get_json_returns_value_for_valid_handle() {
    let mut state = HostState::default();
    state.json_docs.insert(1, serde_json::json!({"key": "val"}));
    let v = state.get_json(1).unwrap();
    assert_eq!(v["key"], "val");
}

#[test]
fn get_json_errors_on_missing_handle() {
    let state = HostState::default();
    assert!(state.get_json(999).is_err());
}

#[test]
fn get_html_doc_returns_node_for_valid_handle() {
    let mut state = HostState::default();
    let send = SendHtml::parse_document("<div>hi</div>");
    let root_id = send.0.lock().unwrap().0.root_element().id();
    state.html_docs.insert(
        1,
        StoredNode {
            doc: send.0,
            node_id: root_id,
        },
    );
    assert!(state.get_html_doc(1).is_ok());
}

#[test]
fn get_html_doc_errors_on_missing_handle() {
    let state = HostState::default();
    assert!(state.get_html_doc(999).is_err());
}

#[test]
fn selector_cache_miss_then_hit() {
    let mut state = HostState::default();
    let _sel = state.get_or_parse_selector("div.card").unwrap();
    assert!(state.get_or_parse_selector("div.card").is_ok());
    assert_eq!(state.selector_cache.lock().expect("lock").len(), 1);
}

#[test]
fn invalid_selector_returns_error() {
    let mut state = HostState::default();
    assert!(state.get_or_parse_selector("::invalid:::").is_err());
}

#[test]
fn clear_all_removes_all_handles_and_resets_counter() {
    let mut state = HostState::default();
    state.json_docs.insert(5, serde_json::Value::Null);
    state.html_lists.insert(6, vec![]);
    state.next_doc_handle = 10;
    state.clear_all();
    assert!(state.json_docs.is_empty());
    assert!(state.html_docs.is_empty());
    assert!(state.html_lists.is_empty());
    assert_eq!(state.next_doc_handle, 1);
}

#[tokio::test]
async fn html_parse_returns_positive_handle() {
    let mut state = HostState::default();
    let handle = html::Host::parse(&mut state, SIMPLE_HTML.to_string()).unwrap();
    assert!(handle > 0);
    assert!(state.html_docs.contains_key(&handle));
}

#[tokio::test]
async fn html_select_returns_list_handle() {
    let mut state = HostState::default();
    let doc = html::Host::parse(&mut state, SIMPLE_HTML.to_string()).unwrap();
    let list = html::Host::select(&mut state, doc, "p".to_string()).unwrap();
    assert!(list > 0);
    let len = html::Host::list_len(&mut state, list).unwrap();
    assert_eq!(len, 1);
}

#[tokio::test]
async fn html_attr_reads_attribute() {
    let mut state = HostState::default();
    let doc = html::Host::parse(&mut state, SIMPLE_HTML.to_string()).unwrap();
    let val = html::Host::attr(
        &mut state,
        doc,
        "div.card".to_string(),
        "data-id".to_string(),
    )
    .unwrap();
    assert_eq!(val, Some("42".to_string()));
}

#[tokio::test]
async fn html_attr_returns_none_for_missing() {
    let mut state = HostState::default();
    let doc = html::Host::parse(&mut state, SIMPLE_HTML.to_string()).unwrap();
    let val =
        html::Host::attr(&mut state, doc, "p".to_string(), "nonexistent".to_string()).unwrap();
    assert_eq!(val, None);
}

#[tokio::test]
async fn html_text_reads_text_content() {
    let mut state = HostState::default();
    let doc = html::Host::parse(&mut state, SIMPLE_HTML.to_string()).unwrap();
    let text = html::Host::text(&mut state, doc, "p".to_string()).unwrap();
    assert_eq!(text, Some("Hello".to_string()));
}

#[tokio::test]
async fn html_inner_html_returns_content() {
    let mut state = HostState::default();
    let doc = html::Host::parse(&mut state, "<div><p>inner</p></div>".to_string()).unwrap();
    let inner = html::Host::inner_html(&mut state, doc).unwrap();
    assert!(inner.is_some());
}

#[tokio::test]
async fn html_first_returns_matching_handle() {
    let mut state = HostState::default();
    let doc = html::Host::parse(&mut state, SIMPLE_HTML.to_string()).unwrap();
    let first = html::Host::first(&mut state, doc, "p".to_string()).unwrap();
    assert!(first.is_some());
}

#[tokio::test]
async fn html_first_returns_none_for_no_match() {
    let mut state = HostState::default();
    let doc = html::Host::parse(&mut state, SIMPLE_HTML.to_string()).unwrap();
    let first = html::Host::first(&mut state, doc, "table".to_string()).unwrap();
    assert!(first.is_none());
}

#[tokio::test]
async fn html_children_returns_list() {
    let mut state = HostState::default();
    let doc = html::Host::parse(&mut state, SIMPLE_HTML.to_string()).unwrap();
    let card = html::Host::first(&mut state, doc, "div.card".to_string())
        .unwrap()
        .unwrap();
    let children = html::Host::children(&mut state, card).unwrap();
    let len = html::Host::list_len(&mut state, children).unwrap();
    assert_eq!(len, 2);
}

#[tokio::test]
async fn html_list_get_returns_element_handle() {
    let mut state = HostState::default();
    let doc = html::Host::parse(&mut state, SIMPLE_HTML.to_string()).unwrap();
    let list = html::Host::select(&mut state, doc, "div.card".to_string()).unwrap();
    let elem = html::Host::list_get(&mut state, list, 0).unwrap();
    assert!(elem > 0);
}

#[tokio::test]
async fn html_list_get_out_of_bounds_errors() {
    let mut state = HostState::default();
    let doc = html::Host::parse(&mut state, SIMPLE_HTML.to_string()).unwrap();
    let list = html::Host::select(&mut state, doc, "p".to_string()).unwrap();
    assert!(html::Host::list_get(&mut state, list, 99).is_err());
}

#[tokio::test]
async fn html_drop_doc_removes_handle() {
    let mut state = HostState::default();
    let doc = html::Host::parse(&mut state, SIMPLE_HTML.to_string()).unwrap();
    assert!(state.html_docs.contains_key(&doc));
    html::Host::drop_doc(&mut state, doc);
    assert!(!state.html_docs.contains_key(&doc));
}

#[tokio::test]
async fn html_drop_list_removes_handle() {
    let mut state = HostState::default();
    let doc = html::Host::parse(&mut state, SIMPLE_HTML.to_string()).unwrap();
    let list = html::Host::select(&mut state, doc, "p".to_string()).unwrap();
    assert!(state.html_lists.contains_key(&list));
    html::Host::drop_list(&mut state, list);
    assert!(!state.html_lists.contains_key(&list));
}

#[tokio::test]
async fn json_parse_returns_positive_handle() {
    let mut state = HostState::default();
    let h = json::Host::parse(&mut state, SIMPLE_JSON.to_vec()).unwrap();
    assert!(h > 0);
    assert!(state.json_docs.contains_key(&h));
}

#[tokio::test]
async fn json_parse_rejects_invalid_json() {
    let mut state = HostState::default();
    assert!(json::Host::parse(&mut state, b"not json".to_vec()).is_err());
}

#[tokio::test]
async fn json_get_str_reads_value() {
    let mut state = HostState::default();
    let h = json::Host::parse(&mut state, SIMPLE_JSON.to_vec()).unwrap();
    let val = json::Host::get_str(&mut state, h, "/name".to_string()).unwrap();
    assert_eq!(val, Some("Alice".to_string()));
}

#[tokio::test]
async fn json_get_str_returns_none_for_missing_pointer() {
    let mut state = HostState::default();
    let h = json::Host::parse(&mut state, SIMPLE_JSON.to_vec()).unwrap();
    let val = json::Host::get_str(&mut state, h, "/nonexistent".to_string()).unwrap();
    assert_eq!(val, None);
}

#[tokio::test]
async fn json_get_i64_reads_integer() {
    let mut state = HostState::default();
    let h = json::Host::parse(&mut state, SIMPLE_JSON.to_vec()).unwrap();
    let val = json::Host::get_i64(&mut state, h, "/age".to_string()).unwrap();
    assert_eq!(val, Some(30));
}

#[tokio::test]
async fn json_get_bool_reads_boolean() {
    let mut state = HostState::default();
    let h = json::Host::parse(&mut state, SIMPLE_JSON.to_vec()).unwrap();
    let val = json::Host::get_bool(&mut state, h, "/active".to_string()).unwrap();
    assert_eq!(val, Some(true));
}

#[tokio::test]
async fn json_array_len_counts_elements() {
    let mut state = HostState::default();
    let h = json::Host::parse(&mut state, SIMPLE_JSON.to_vec()).unwrap();
    let len = json::Host::array_len(&mut state, h, "/scores".to_string()).unwrap();
    assert_eq!(len, Some(3));
}

#[tokio::test]
async fn json_array_get_returns_element_handle() {
    let mut state = HostState::default();
    let h = json::Host::parse(&mut state, SIMPLE_JSON.to_vec()).unwrap();
    let elem = json::Host::array_get(&mut state, h, "/scores".to_string(), 1).unwrap();
    let val = json::Host::get_i64(&mut state, elem, "".to_string()).unwrap();
    assert_eq!(val, Some(20));
}

#[tokio::test]
async fn json_array_get_out_of_bounds_errors() {
    let mut state = HostState::default();
    let h = json::Host::parse(&mut state, SIMPLE_JSON.to_vec()).unwrap();
    assert!(json::Host::array_get(&mut state, h, "/scores".to_string(), 99).is_err());
}

#[tokio::test]
async fn json_object_keys_returns_field_names() {
    let mut state = HostState::default();
    let h = json::Host::parse(&mut state, SIMPLE_JSON.to_vec()).unwrap();
    let mut keys = json::Host::object_keys(&mut state, h, "".to_string()).unwrap();
    keys.sort();
    assert_eq!(keys, vec!["active", "age", "name", "scores"]);
}

#[tokio::test]
async fn json_object_get_returns_child_handle() {
    let mut state = HostState::default();
    let h = json::Host::parse(&mut state, br#"{"nested":{"x":1}}"#.to_vec()).unwrap();
    let child =
        json::Host::object_get(&mut state, h, "".to_string(), "nested".to_string()).unwrap();
    assert!(child.is_some());
    let x = json::Host::get_i64(&mut state, child.unwrap(), "/x".to_string()).unwrap();
    assert_eq!(x, Some(1));
}

#[tokio::test]
async fn json_drop_removes_handle() {
    let mut state = HostState::default();
    let h = json::Host::parse(&mut state, SIMPLE_JSON.to_vec()).unwrap();
    assert!(state.json_docs.contains_key(&h));
    json::Host::drop_json(&mut state, h);
    assert!(!state.json_docs.contains_key(&h));
}

#[tokio::test]
async fn json_to_string_serializes_value() {
    let mut state = HostState::default();
    let h = json::Host::parse(&mut state, br#"{"a":1}"#.to_vec()).unwrap();
    let s = json::Host::to_string(&mut state, h).unwrap();
    let reparsed: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert_eq!(reparsed["a"], 1);
}

#[test]
fn utility_date_parse_rfc3339_epoch() {
    let mut state = HostState::default();
    let ts =
        utility::Host::date_parse_rfc3339(&mut state, "1970-01-01T00:00:00Z".to_string()).unwrap();
    assert_eq!(ts, 0);
}

#[test]
fn utility_date_parse_rfc3339_known_date() {
    let mut state = HostState::default();
    let ts =
        utility::Host::date_parse_rfc3339(&mut state, "2024-01-01T00:00:00Z".to_string()).unwrap();
    assert_eq!(ts, 1_704_067_200);
}

#[test]
fn utility_date_parse_rfc3339_rejects_invalid() {
    let mut state = HostState::default();
    assert!(utility::Host::date_parse_rfc3339(&mut state, "not-a-date".to_string()).is_err());
}

#[test]
fn utility_resolve_url_relative_path() {
    let mut state = HostState::default();
    let resolved = utility::Host::resolve_url(
        &mut state,
        "https://example.com/manga/".to_string(),
        "chapter/1".to_string(),
    )
    .unwrap();
    assert_eq!(resolved, "https://example.com/manga/chapter/1");
}

#[test]
fn utility_resolve_url_absolute_overrides_base() {
    let mut state = HostState::default();
    let resolved = utility::Host::resolve_url(
        &mut state,
        "https://example.com/some/path".to_string(),
        "https://other.com/new".to_string(),
    )
    .unwrap();
    assert_eq!(resolved, "https://other.com/new");
}

#[test]
fn utility_build_url_appends_query_params() {
    let mut state = HostState::default();
    let url = utility::Host::build_url(
        &mut state,
        "https://example.com/search".to_string(),
        vec![
            ("q".to_string(), "manga".to_string()),
            ("page".to_string(), "1".to_string()),
        ],
    )
    .unwrap();
    assert!(url.contains("q=manga"));
    assert!(url.contains("page=1"));
}

#[test]
fn utility_url_encode_round_trip() {
    let mut state = HostState::default();
    let encoded = utility::Host::url_encode(&mut state, "hello world & more".to_string());
    let decoded = utility::Host::url_decode(&mut state, encoded).unwrap();
    assert_eq!(decoded, "hello world & more");
}

#[test]
fn utility_url_decode_rejects_invalid_utf8() {
    let mut state = HostState::default();
    // %80 decodes to byte 0x80 which is a lone UTF-8 continuation byte — invalid
    assert!(utility::Host::url_decode(&mut state, "%80".to_string()).is_err());
}

#[test]
fn utility_get_query_param_finds_key() {
    let mut state = HostState::default();
    let val = utility::Host::get_query_param(
        &mut state,
        "https://example.com/?foo=bar&baz=qux".to_string(),
        "foo".to_string(),
    );
    assert_eq!(val, Some("bar".to_string()));
}

#[test]
fn utility_get_query_param_returns_none_for_missing() {
    let mut state = HostState::default();
    let val = utility::Host::get_query_param(
        &mut state,
        "https://example.com/?foo=bar".to_string(),
        "missing".to_string(),
    );
    assert_eq!(val, None);
}

#[test]
fn utility_encode_form_produces_form_data() {
    let mut state = HostState::default();
    let out = utility::Host::encode_form(
        &mut state,
        vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "hello world".to_string()),
        ],
    );
    assert!(out.contains("a=1"));
    assert!(out.contains("b=hello+world") || out.contains("b=hello%20world"));
}

#[test]
fn ext_error_from_wit_maps_source_updating() {
    use super::kani::extension::types::{ExtensionError as WitErr, ExtensionErrorKind as WitKind};

    let wit_err = WitErr {
        kind: WitKind::SourceUpdating,
        message: "Source is being updated".to_string(),
        source_url: None,
        retry_after_secs: Some(2),
    };
    let err = super::ext_error_from_wit(wit_err);
    assert_eq!(
        err.kind,
        kani_shared::extension::ExtensionErrorKind::Updating
    );
}

#[tokio::test]
async fn browser_capture_is_blocked_for_a_disallowed_host() {
    use super::kani::extension::scripting::Host as _;

    let mut state = HostState {
        allowed_host: AllowedHost::Restricted("example.com".to_string()),
        ..Default::default()
    };

    let error = state
        .capture_page_payload(
            "https://evil.com/browse".to_string(),
            "passPayload('x')".to_string(),
            1000,
        )
        .await
        .expect_err("a capture outside the source's allowed host must be refused");

    assert!(
        error.contains("evil.com") || error.contains("not permitted") || error.contains("blocked"),
        "the refusal names the host policy, got: {error}"
    );
}

#[tokio::test]
async fn browser_capture_rejects_an_unparseable_url() {
    use super::kani::extension::scripting::Host as _;

    let mut state = HostState {
        allowed_host: AllowedHost::Restricted("example.com".to_string()),
        ..Default::default()
    };

    let error = state
        .capture_page_payload(
            "not a url".to_string(),
            "passPayload('x')".to_string(),
            1000,
        )
        .await
        .expect_err("a malformed page URL must not reach the browser");

    assert!(error.contains("Invalid browser page URL"), "got: {error}");
}

#[test]
fn a_refresh_auth_from_a_guest_hook_is_a_clear_auth_error() {
    use super::kani::extension::types::{ExtensionError as WitErr, ExtensionErrorKind as WitKind};
    let err = super::guest_error(WitErr {
        kind: WitKind::Unknown,
        message: "__refresh_auth__:login".to_string(),
        source_url: None,
        retry_after_secs: None,
    });
    match err {
        crate::error::Error::Extension(e) => {
            assert_eq!(e.kind, kani_shared::extension::ExtensionErrorKind::Auth);
            assert!(
                e.message.contains("refresh_auth is only supported"),
                "{}",
                e.message
            );
            assert!(!e.message.contains("__refresh_auth__"), "{}", e.message);
        }
        other => panic!("{other:?}"),
    }
}
