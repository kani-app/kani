//! Ownership-aware guest wrappers around host-imported WIT capabilities.
//!
//! Handle wrappers release host resources on drop. Lower-level generated bindings remain exposed
//! for operations that do not require additional ownership or error semantics.

use crate::{
    ExtensionError,
    bindings::kani::extension::{cache as cache_wit, html, http, json, utility},
};

pub use http::Method as HttpMethod;
pub type DocumentHandle = html::DocHandle;
pub type ListHandle = html::ListHandle;

/// High-level API for making HTTP requests from extensions.
pub struct HttpRequest {
    method: HttpMethod,
    url: Option<String>,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    queries: Vec<(String, String)>,
    endpoint_id: Option<String>,
}

impl HttpRequest {
    pub fn new() -> Self {
        Self {
            method: HttpMethod::Get,
            url: None,
            headers: Vec::new(),
            body: None,
            queries: Vec::new(),
            endpoint_id: None,
        }
    }

    fn new_method<S: Into<String>>(url: S, method: HttpMethod) -> Self {
        Self {
            url: Some(url.into()),
            method,
            headers: Vec::new(),
            body: None,
            queries: Vec::new(),
            endpoint_id: None,
        }
    }

    pub fn get<S: Into<String>>(url: S) -> Self {
        Self::new_method(url, HttpMethod::Get)
    }

    pub fn post<S: Into<String>>(url: S) -> Self {
        Self::new_method(url, HttpMethod::Post)
    }

    pub fn put<S: Into<String>>(url: S) -> Self {
        Self::new_method(url, HttpMethod::Put)
    }

    pub fn delete<S: Into<String>>(url: S) -> Self {
        Self::new_method(url, HttpMethod::Delete)
    }

    pub fn method(mut self, method: HttpMethod) -> Self {
        self.method = method;
        self
    }

    pub fn url<S: Into<String>>(mut self, url: S) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Tag this request with a named endpoint ID (used by hook dispatch).
    pub fn endpoint_id<S: Into<String>>(mut self, id: S) -> Self {
        self.endpoint_id = Some(id.into());
        self
    }

    pub fn header<K: Into<String>, V: std::fmt::Display>(mut self, key: K, value: V) -> Self {
        self.headers.push((key.into(), value.to_string()));
        self
    }

    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    #[cfg(feature = "host")]
    pub fn json<T: serde::Serialize>(mut self, payload: &T) -> Result<Self, ExtensionError> {
        let body = serde_json::to_vec(payload).map_err(|e| ExtensionError::parse(e.to_string()))?;

        self.body = Some(body);
        Ok(self.header("Content-Type", "application/json"))
    }

    pub fn form<K: std::fmt::Display, V: std::fmt::Display>(mut self, data: &[(K, V)]) -> Self {
        let string_data: Vec<(String, String)> = data
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let form_string = crate::utility::encode_form(&string_data);

        self.body = Some(form_string.into_bytes());
        self.header("Content-Type", "application/x-www-form-urlencoded")
    }

    /// Appends a URL-encoded query parameter to the request.
    pub fn query<K: Into<String>, V: std::fmt::Display>(mut self, key: K, value: V) -> Self {
        self.queries.push((key.into(), value.to_string()));
        self
    }

    #[cfg(test)]
    pub(crate) fn into_queries(self) -> Vec<(String, String)> {
        self.queries
    }

    /// Consume this request and return its components for use with raw extraction.
    #[allow(clippy::type_complexity)]
    pub(crate) fn into_extract_parts(
        self,
    ) -> Result<(String, String, Vec<(String, String)>, Vec<(String, String)>), ExtensionError>
    {
        let url = self.build_final_url()?;
        let method = match self.method {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
        }
        .to_string();
        Ok((url, method, self.headers, self.queries))
    }

    fn build_final_url(&self) -> Result<String, ExtensionError> {
        let url = match &self.url {
            Some(u) => u,
            None => return Err(ExtensionError::unknown("URL is not set".to_string())),
        };

        if self.queries.is_empty() {
            return Ok(url.clone());
        }

        crate::utility::build_url(url, &self.queries).map_err(crate::ExtensionError::unknown)
    }

    /// Send the request and get the response body as a string.
    pub fn send(self) -> Result<http::Response, ExtensionError> {
        let url = self.build_final_url()?;

        let req = http::Request {
            method: self.method,
            url,
            headers: self.headers,
            body: self.body,
        };

        match http::send(&req) {
            Ok(res) => Ok(res),
            Err(e) => Err(ExtensionError::network(e)),
        }
    }

    pub fn send_html(self) -> Result<HtmlDocument, ExtensionError> {
        let resp = self.send()?;
        let html_string = String::from_utf8(resp.body)
            .map_err(|_| ExtensionError::parse("Invalid UTF-8 in HTML".into()))?;
        HtmlDocument::new(&html_string)
    }

    pub fn send_json_handle(self) -> Result<JsonHandle, ExtensionError> {
        let res = self.send()?;

        if res.body.is_empty() {
            return Err(ExtensionError::network("Empty response".to_string()));
        }

        JsonHandle::parse(&res.body)
    }
}

impl Default for HttpRequest {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII wrapper for a host-owned HTML node.
///
/// Query helpers treat host errors and missing values as empty results. Dropping
/// the wrapper releases its host handle.
pub struct HtmlDocument {
    handle: DocumentHandle,
}

impl HtmlDocument {
    pub fn new(html: &str) -> Result<Self, ExtensionError> {
        let handle = html::parse(html).map_err(ExtensionError::unknown)?;
        Ok(Self { handle })
    }

    pub fn handle(&self) -> DocumentHandle {
        self.handle
    }

    pub fn select(&self, selector: &str) -> Vec<HtmlDocument> {
        let list_handle = match html::select(self.handle, selector) {
            Ok(h) => h,
            Err(_) => return Vec::new(),
        };

        let len = html::list_len(list_handle).unwrap_or(0);
        let mut docs = Vec::with_capacity(len as usize);

        for i in 0..len {
            if let Ok(doc_h) = html::list_get(list_handle, i) {
                docs.push(HtmlDocument { handle: doc_h });
            }
        }

        html::drop_list(list_handle);
        docs
    }

    pub fn first(&self, selector: &str) -> Option<HtmlDocument> {
        html::first(self.handle, selector)
            .ok()
            .flatten()
            .map(|h| HtmlDocument { handle: h })
    }

    pub fn children(&self) -> Vec<HtmlDocument> {
        let list_handle = match html::children(self.handle) {
            Ok(h) => h,
            Err(_) => return Vec::new(),
        };

        let len = html::list_len(list_handle).unwrap_or(0);
        let mut docs = Vec::with_capacity(len as usize);

        for i in 0..len {
            if let Ok(doc_h) = html::list_get(list_handle, i) {
                docs.push(HtmlDocument { handle: doc_h });
            }
        }

        html::drop_list(list_handle);
        docs
    }

    pub fn attr(&self, selector: &str, attribute: &str) -> String {
        html::attr(self.handle, selector, attribute)
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    pub fn get_attr(&self, attribute: &str) -> String {
        html::attr(self.handle, "", attribute)
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    pub fn text(&self, selector: &str) -> String {
        html::text(self.handle, selector)
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    pub fn get_text(&self) -> String {
        html::text(self.handle, "")
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    pub fn inner_html(&self) -> String {
        html::inner_html(self.handle)
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    pub fn outer_html(&self) -> String {
        html::outer_html(self.handle)
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    pub fn has_class(&self, class_name: &str) -> bool {
        let classes = self.get_attr("class");
        classes.split_whitespace().any(|c| c == class_name)
    }

    pub fn tag_name(&self) -> String {
        let html = self.outer_html();
        if html.starts_with('<')
            && let Some(stripped) = html.strip_prefix('<')
        {
            return stripped
                .split(|c: char| c.is_whitespace() || c == '>')
                .next()
                .unwrap_or("")
                .to_string();
        }
        String::new()
    }
}

impl std::fmt::Debug for HtmlDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let content = self.outer_html();
        let preview = if content.len() > 100 {
            format!("{}...", &content[..100])
        } else {
            content
        };
        write!(
            f,
            "HtmlDocument(handle: {}, html: {:?})",
            self.handle, preview
        )
    }
}

impl Drop for HtmlDocument {
    fn drop(&mut self) {
        html::drop_doc(self.handle);
    }
}

/// Parse a date string with the given chrono format into epoch seconds.
/// Returns `None` if parsing fails.
pub fn parse_date(date: &str, fmt: &str) -> Option<i64> {
    let result = utility::date_parse(date, fmt);
    match result {
        Ok(t) if t >= 0 => Some(t),
        _ => None,
    }
}

pub fn parse_date_rfc3339(date: &str) -> Option<i64> {
    let result = utility::date_parse_rfc3339(date);
    match result {
        Ok(t) if t >= 0 => Some(t),
        _ => None,
    }
}

/// Joins a base URL and a relative path, handling slashes.
pub fn resolve<B: Into<String>, P: Into<String>>(base: B, path: P) -> Result<String, String> {
    crate::utility::resolve_url(&base.into(), &path.into())
}

/// URL-encode a raw string
pub fn encode<S: Into<String>>(input: S) -> String {
    crate::utility::url_encode(&input.into())
}

/// URL-decode a percent-encoded string.
pub fn decode<S: Into<String>>(input: S) -> Result<String, String> {
    crate::utility::url_decode(&input.into())
}

/// Extract the value of a specific query parameter from a full URL.
pub fn get_query_param<U: Into<String>, K: Into<String>>(url: U, key: K) -> Option<String> {
    crate::utility::get_query_param(&url.into(), &key.into())
}

/// Returns the extension version string, appending `+debug` when compiled with
/// debug assertions (i.e. `wasm-debug` profile). Usage: `ext_version!("1.2.3")`
#[macro_export]
macro_rules! ext_version {
    ($v:literal) => {{
        #[cfg(debug_assertions)]
        {
            concat!($v, "+debug").to_string()
        }
        #[cfg(not(debug_assertions))]
        {
            $v.to_string()
        }
    }};
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        let msg = format!($($arg)*);
        $crate::utility::log(1, line!(), &msg);
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        let msg = format!($($arg)*);
        $crate::utility::log(3, line!(), &msg);
    };
}

#[macro_export]
macro_rules! dbg {
    () => {
        $crate::utility::log(0, line!(), "");
    };
    ($val:expr $(,)?) => {
        match $val {
            tmp => {
                let msg = format!("{} = {:#?}", stringify!($val), &tmp);
                $crate::utility::log(0, line!(), &msg);
                tmp
            }
        }
    };
}

/// RAII wrapper around a host-owned JSON value.
///
/// Optional accessors treat host errors and missing values alike. Dropping the
/// wrapper releases its host handle.
pub struct JsonHandle {
    handle: json::JsonHandle,
}

impl JsonHandle {
    pub fn from_raw(handle: json::JsonHandle) -> Self {
        Self { handle }
    }

    /// Return the underlying host-side handle (an opaque i32).
    /// The handle remains valid as long as this `JsonHandle` is live.
    pub fn raw_handle(&self) -> json::JsonHandle {
        self.handle
    }

    pub fn parse(data: &[u8]) -> Result<Self, ExtensionError> {
        let handle = json::parse(data).map_err(ExtensionError::parse)?;
        Ok(Self { handle })
    }

    pub fn get_str(&self, ptr: &str) -> Option<String> {
        json::get_str(self.handle, ptr).ok().flatten()
    }

    /// Get a required string, returning ParseError if absent.
    pub fn require_str(&self, ptr: &str) -> Result<String, ExtensionError> {
        self.get_str(ptr)
            .ok_or_else(|| ExtensionError::parse(format!("Missing required field: {}", ptr)))
    }

    pub fn get_i64(&self, ptr: &str) -> Option<i64> {
        json::get_i64(self.handle, ptr).ok().flatten()
    }

    pub fn get_f64(&self, ptr: &str) -> Option<f64> {
        json::get_f64(self.handle, ptr).ok().flatten()
    }

    pub fn get_bool(&self, ptr: &str) -> Option<bool> {
        json::get_bool(self.handle, ptr).ok().flatten()
    }

    pub fn array_len(&self, ptr: &str) -> Option<i32> {
        json::array_len(self.handle, ptr).ok().flatten()
    }

    pub fn array_get(&self, ptr: &str, index: i32) -> Result<JsonHandle, ExtensionError> {
        let child = json::array_get(self.handle, ptr, index).map_err(ExtensionError::parse)?;
        Ok(JsonHandle { handle: child })
    }

    pub fn array_iter(&self, ptr: &str) -> impl Iterator<Item = JsonHandle> + '_ {
        let len = self.array_len(ptr).unwrap_or(0);
        let ptr = ptr.to_string();
        (0..len).filter_map(move |i| self.array_get(&ptr, i).ok())
    }

    pub fn object_keys(&self, ptr: &str) -> Vec<String> {
        json::object_keys(self.handle, ptr).ok().unwrap_or_default()
    }

    /// Returns a child handle for the keyed object value under `ptr`.
    /// Returns `None` if the key doesn't exist or `ptr` is not an object.
    pub fn object_get(&self, ptr: &str, key: &str) -> Option<JsonHandle> {
        json::object_get(self.handle, ptr, key)
            .ok()
            .flatten()
            .map(|h| JsonHandle { handle: h })
    }

    pub fn object_iter<'a>(
        &'a self,
        ptr: &'a str,
    ) -> impl Iterator<Item = (String, JsonHandle)> + 'a {
        self.object_keys(ptr)
            .into_iter()
            .filter_map(move |k| self.object_get(ptr, &k).map(|v| (k, v)))
    }

    pub fn rows_len(&self) -> i32 {
        self.array_len("/rows").unwrap_or(0)
    }

    pub fn rows_get(&self, index: i32) -> Result<JsonHandle, crate::ExtensionError> {
        self.array_get("/rows", index)
    }

    pub fn rows_iter(&self) -> impl Iterator<Item = JsonHandle> + '_ {
        let len = self.rows_len();
        (0..len).filter_map(|i| self.rows_get(i).ok())
    }

    pub fn get_scalar_str(&self, name: &str) -> Option<String> {
        self.get_str(&format!("/scalars/{}", name))
    }

    /// Get a scalar boolean from a blueprint extraction result. Returns `false` if absent.
    pub fn get_scalar_bool(&self, name: &str) -> bool {
        self.get_bool(&format!("/scalars/{}", name))
            .unwrap_or(false)
    }

    pub fn get_scalar_i64(&self, name: &str) -> Option<i64> {
        self.get_i64(&format!("/scalars/{}", name))
    }

    /// Collect all string elements of a JSON array at `ptr` into a `Vec<String>`.
    /// Non-string elements and missing values are silently skipped.
    pub fn get_array_of_strings(&self, ptr: &str) -> Vec<String> {
        self.array_iter(ptr).filter_map(|h| h.get_str("")).collect()
    }
}

impl std::fmt::Debug for JsonHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let json_str = json::to_string(self.handle).unwrap_or_else(|_| "null".to_string());
        write!(
            f,
            "JsonHandle(handle: {}, value: {})",
            self.handle, json_str
        )
    }
}

impl Drop for JsonHandle {
    fn drop(&mut self) {
        json::drop_json(self.handle);
    }
}

#[cfg(feature = "builder")]
impl crate::ast::BlueprintBuilder {
    /// Attach an HTTP request that the host will fetch before running extraction.
    /// Converts the `HttpRequest` builder into a `RequestDef` (body is not forwarded —
    /// blueprint requests are used for fetching pages, not posting data).
    pub fn request(self, req: HttpRequest) -> Self {
        self.with_request(http_request_to_def(req))
    }
}

#[cfg(feature = "builder")]
impl crate::ast::Blueprint {
    /// Return a clone of this blueprint with a different HTTP request attached.
    /// Useful for loop patterns where the URL/params change each iteration but
    /// the field definitions stay the same.
    pub fn with_request(&self, req: HttpRequest) -> crate::ast::Blueprint {
        self.with_request_def(http_request_to_def(req))
    }
}

#[cfg(feature = "builder")]
fn http_request_to_def(req: HttpRequest) -> crate::ast::RequestDef {
    crate::ast::RequestDef {
        url: req.url.unwrap_or_default(),
        method: match req.method {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
        }
        .to_string(),
        headers: req.headers,
        queries: req.queries,
        endpoint_id: req.endpoint_id,
    }
}

#[cfg(feature = "builder")]
pub mod extract {
    use crate::ExtensionError;
    use crate::ast::Blueprint;
    use crate::bindings::kani::extension::extraction;
    use crate::host_abi::JsonHandle;

    /// Turn a host-side extraction error into a typed [`ExtensionError`]. When the
    /// host marks it with the HTTP-status sentinel (a 429/5xx/auth the body was
    /// never useful for), classify it so the compiled-WASM backend reports the
    /// same kind the interpreted-YAML backend does; otherwise it is a genuine
    /// extraction failure and stays `Unknown`.
    fn classify_host_error(msg: String) -> ExtensionError {
        crate::extension::classify_status_error(&msg)
            .unwrap_or_else(|| ExtensionError::unknown(msg))
    }

    /// Run a blueprint extraction over an HTML document handle.
    /// Returns a `JsonHandle` wrapping `[{field: value, ...}, ...]`.
    pub fn html(doc: Option<i32>, blueprint: &Blueprint) -> Result<JsonHandle, ExtensionError> {
        let bytes = blueprint.to_bytes();
        let handle = extraction::extract_html(doc, &bytes).map_err(classify_host_error)?;
        Ok(JsonHandle::from_raw(handle))
    }

    /// Run a blueprint extraction over a JSON document handle.
    /// Returns a `JsonHandle` wrapping `[{field: value, ...}, ...]`.
    pub fn json(handle: Option<i32>, blueprint: &Blueprint) -> Result<JsonHandle, ExtensionError> {
        let bytes = blueprint.to_bytes();
        let result_handle =
            extraction::extract_json(handle, &bytes).map_err(classify_host_error)?;
        Ok(JsonHandle::from_raw(result_handle))
    }

    /// Run a paginated HTML extraction.  The blueprint must include a `PaginationConfig`;
    /// the host handles multi-chunk fetching, stitching, and `has_next_page` detection.
    pub fn paginated_html(
        page: i32,
        page_size: i32,
        blueprint: &Blueprint,
    ) -> Result<JsonHandle, ExtensionError> {
        let bytes = blueprint.to_bytes();
        let handle = extraction::paginated_extract_html(page, page_size, &bytes)
            .map_err(classify_host_error)?;
        Ok(JsonHandle::from_raw(handle))
    }

    /// Run a paginated JSON extraction.  The blueprint must include a `PaginationConfig`;
    /// the host handles multi-chunk fetching, stitching, and `has_next_page` detection.
    /// Supports `OffsetType::ItemOffset`, `PageNumber`, and `CursorToken`.
    pub fn paginated_json(
        page: i32,
        page_size: i32,
        blueprint: &Blueprint,
    ) -> Result<JsonHandle, ExtensionError> {
        let bytes = blueprint.to_bytes();
        let handle = extraction::paginated_extract_json(page, page_size, &bytes)
            .map_err(classify_host_error)?;
        Ok(JsonHandle::from_raw(handle))
    }
}

/// Like the builder-only `extract` module but takes pre-computed postcard bytes directly,
/// bypassing `BlueprintBuilder` and the `builder` feature entirely. The blueprint bytes
/// must **not** include a `RequestDef`; callers are responsible for fetching
/// the document and passing the resulting handle.
pub mod extract_raw {
    use super::HttpRequest;
    use crate::ExtensionError;
    use crate::bindings::kani::extension::extraction;
    use crate::host_abi::JsonHandle;

    /// Run a blueprint extraction over a pre-fetched HTML document handle.
    pub fn html(doc: Option<i32>, bytes: &[u8]) -> Result<JsonHandle, ExtensionError> {
        let handle = extraction::extract_html(doc, bytes).map_err(ExtensionError::unknown)?;
        Ok(JsonHandle::from_raw(handle))
    }

    /// Run a blueprint extraction over a pre-fetched JSON handle.
    pub fn json(handle: Option<i32>, bytes: &[u8]) -> Result<JsonHandle, ExtensionError> {
        let result = extraction::extract_json(handle, bytes).map_err(ExtensionError::unknown)?;
        Ok(JsonHandle::from_raw(result))
    }

    /// Run a paginated HTML extraction.  The blueprint bytes must include a
    /// `PaginationConfig` but no `RequestDef`; the request is provided separately
    /// so the host can attach it before each paginated fetch.
    pub fn paginated_html(
        page: i32,
        page_size: i32,
        req: HttpRequest,
        bytes: &[u8],
    ) -> Result<JsonHandle, ExtensionError> {
        let (url, method, headers, queries) = req.into_extract_parts()?;
        let handle = extraction::paginated_extract_html_raw(
            page, page_size, &url, &method, &headers, &queries, bytes,
        )
        .map_err(ExtensionError::unknown)?;
        Ok(JsonHandle::from_raw(handle))
    }

    /// Run a paginated JSON extraction.  The blueprint bytes must include a
    /// `PaginationConfig` but no `RequestDef`; the request is provided separately
    /// so the host can attach it before each paginated fetch.
    pub fn paginated_json(
        page: i32,
        page_size: i32,
        req: HttpRequest,
        bytes: &[u8],
    ) -> Result<JsonHandle, ExtensionError> {
        let (url, method, headers, queries) = req.into_extract_parts()?;
        let handle = extraction::paginated_extract_json_raw(
            page, page_size, &url, &method, &headers, &queries, bytes,
        )
        .map_err(ExtensionError::unknown)?;
        Ok(JsonHandle::from_raw(handle))
    }
}

/// Typed helpers for reading extension preference values stored by the host.
///
/// Values are stored as strings by the host. Each helper converts to the
/// appropriate Rust type based on the preference kind declared in
/// `get_preferences()`.
pub mod prefs {
    fn raw(key: &str) -> Option<String> {
        crate::bindings::kani::extension::prefs::get_value(key)
    }

    /// Returns the stored string value, or an empty string if unset.
    pub fn get_str(key: &str) -> String {
        raw(key).unwrap_or_default()
    }

    /// Returns the stored string value, or `default` if unset or empty.
    pub fn get_str_or(key: &str, default: &str) -> String {
        raw(key)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| default.to_string())
    }

    /// Returns `true` if the stored value is exactly `"true"`.
    pub fn get_bool(key: &str) -> bool {
        raw(key).map(|v| v == "true").unwrap_or(false)
    }

    /// Parses the stored value as `i64`, returning `None` if unset or not parseable.
    pub fn get_i64(key: &str) -> Option<i64> {
        raw(key)?.parse().ok()
    }

    /// Parses the stored value as `f64`, returning `None` if unset or not parseable.
    pub fn get_f64(key: &str) -> Option<f64> {
        raw(key)?.parse().ok()
    }

    /// Parses a multi-value-list preference stored as a JSON array into a `Vec<String>`.
    /// Returns an empty vec if the value is unset or cannot be parsed.
    pub fn get_list(key: &str) -> Vec<String> {
        parse_json_str_array(&raw(key).unwrap_or_default())
    }

    fn parse_json_str_array(s: &str) -> Vec<String> {
        let s = s.trim();
        if !s.starts_with('[') || !s.ends_with(']') {
            return vec![];
        }
        let inner = &s[1..s.len() - 1];
        if inner.trim().is_empty() {
            return vec![];
        }
        let mut result = Vec::new();
        let bytes = inner.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | b',') {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            if bytes[i] == b'"' {
                i += 1;
                let mut value = String::new();
                while i < bytes.len() {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 1;
                        match bytes[i] {
                            b'n' => value.push('\n'),
                            b't' => value.push('\t'),
                            b'r' => value.push('\r'),
                            b'"' => value.push('"'),
                            b'\\' => value.push('\\'),
                            c => {
                                value.push('\\');
                                value.push(c as char);
                            }
                        }
                    } else if bytes[i] == b'"' {
                        break;
                    } else {
                        value.push(bytes[i] as char);
                    }
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
                result.push(value);
            } else {
                i += 1;
            }
        }
        result
    }
}

/// Wrappers for the host-side JS execution context (backed by the Node.js V8 subprocess).
///
/// This is an alias for `v8_context` kept for backwards compatibility. New extensions
/// should use `v8_context` directly; existing extensions using `js_context` continue
/// to work unchanged and will use the V8 runtime.
pub mod js_context {
    use crate::ExtensionError;

    pub fn exists(name: &str) -> bool {
        super::v8_context::exists(name)
    }

    pub fn create(name: &str, init_script: &str) -> Result<(), ExtensionError> {
        super::v8_context::create(name, init_script)
    }

    pub fn eval(name: &str, script: &str) -> Result<String, ExtensionError> {
        super::v8_context::eval(name, script)
    }

    pub fn drop_ctx(name: &str) {
        super::v8_context::drop_ctx(name);
    }
}

/// Wrappers for the host-side Node.js V8 execution context.
///
/// The host maintains named V8 contexts backed by a persistent Node.js subprocess.
/// Extensions initialize a context once (loading their JS bundle + minimal stubs),
/// then call `eval` on subsequent requests. Provides native Web APIs: `crypto.subtle`,
/// `TextEncoder`, `URL`, `URLSearchParams`, etc.
pub mod v8_context {
    use crate::ExtensionError;
    use crate::bindings::kani::extension::scripting;

    /// Returns true if a named V8 context currently exists on the host.
    pub fn exists(name: &str) -> bool {
        scripting::v8_context_exists(name)
    }

    /// Creates a named V8 context by running `init_script` in a fresh Node.js
    /// vm.createContext sandbox. Idempotent: if the context already exists this is a no-op.
    pub fn create(name: &str, init_script: &str) -> Result<(), ExtensionError> {
        scripting::v8_context_create(name, init_script).map_err(ExtensionError::unknown)
    }

    /// Evaluates `script` in the named V8 context. The script must produce a string value.
    pub fn eval(name: &str, script: &str) -> Result<String, ExtensionError> {
        scripting::v8_context_eval(name, script).map_err(ExtensionError::unknown)
    }

    /// Drops the named context and frees its memory on the host.
    pub fn drop_ctx(name: &str) {
        scripting::v8_context_drop(name);
    }

    pub fn capture_page_payload(
        page_url: &str,
        init_script: &str,
        timeout_ms: u32,
    ) -> Result<String, ExtensionError> {
        scripting::capture_page_payload(page_url, init_script, timeout_ms)
            .map_err(ExtensionError::unknown)
    }

    pub fn capture_page_payload_configured(
        page_url: &str,
        init_script: &str,
        timeout_ms: u32,
        auto_scroll: bool,
    ) -> Result<String, ExtensionError> {
        let script = crate::types::mark_auto_scroll(init_script, auto_scroll);
        capture_page_payload(page_url, &script, timeout_ms)
    }
}

pub mod cache {
    use super::cache_wit;

    pub fn get(key: &str) -> Option<Vec<u8>> {
        cache_wit::get(key)
    }

    pub fn put(key: &str, value: Vec<u8>, ttl_secs: u32) {
        cache_wit::put(key, &value, ttl_secs);
    }

    pub fn delete(key: &str) {
        cache_wit::delete(key);
    }

    pub fn clear() {
        cache_wit::clear();
    }
}
