//! Cryptographic helpers for the opaque image proxy.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce, aead::Aead};
use hmac::{Hmac, Mac};
use sha2::Sha256;

const BUST_PARAMS: &[&str] = &["cb", "ts", "t", "_", "v", "ver", "nc"];

/// Return a canonical cache key by stripping well-known cache-bust query params.
pub fn canonical_proxy_key(url: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(url) else {
        return url.to_string();
    };
    let kept: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| !BUST_PARAMS.contains(&k.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        parsed.set_query(None);
    } else {
        let mut setter = parsed.query_pairs_mut();
        setter.clear();
        for (k, v) in &kept {
            setter.append_pair(k, v);
        }
    }
    parsed.to_string()
}

type HmacSha256 = Hmac<Sha256>;

const NONCE_LEN: usize = 12;

/// Leads the sealed plaintext. The nonce is carried in the token and used as
/// supplied, so an unversioned token still decrypts; this marker is what refuses it.
/// v3 adds the owning source's id; v2 tokens are still read, as belonging to no source.
const TOKEN_VERSION: &str = "v3";
const LEGACY_TOKEN_VERSION: &str = "v2";

/// Load the proxy secret from `KANI_PROXY_SECRET` (base64-encoded 32 bytes), read/persist it
/// from `data_dir/proxy.key`, or generate and persist a new one on first boot.
///
/// Priority order:
/// 1. `KANI_PROXY_SECRET` — base64url-encoded 32-byte value.
/// 2. `data_dir/proxy.key` — persisted from a previous boot.
/// 3. Generate a fresh random secret and write it to `data_dir/proxy.key`.
pub(crate) fn load_or_persist_secret(data_dir: &std::path::Path) -> [u8; 32] {
    if let Ok(val) = std::env::var("KANI_PROXY_SECRET") {
        let decoded = URL_SAFE_NO_PAD.decode(val.trim()).unwrap_or_default();
        if decoded.len() == 32 {
            let mut secret = [0u8; 32];
            secret.copy_from_slice(&decoded);
            tracing::info!("Loaded proxy secret from KANI_PROXY_SECRET");
            return secret;
        }
        tracing::warn!(
            "KANI_PROXY_SECRET was set but not 32 bytes after decoding — \
             falling back to persisted secret"
        );
    }

    let key_path = data_dir.join("proxy.key");
    if let Ok(encoded) = std::fs::read_to_string(&key_path) {
        let decoded = URL_SAFE_NO_PAD.decode(encoded.trim()).unwrap_or_default();
        if decoded.len() == 32 {
            let mut secret = [0u8; 32];
            secret.copy_from_slice(&decoded);
            tracing::info!("Loaded proxy secret from {}", key_path.display());
            return secret;
        }
        tracing::warn!("proxy.key exists but is malformed — regenerating");
    }

    let secret: [u8; 32] = rand::random();
    let encoded = URL_SAFE_NO_PAD.encode(secret);
    match std::fs::write(&key_path, &encoded) {
        Ok(_) => tracing::info!(
            "Generated and persisted proxy secret to {}",
            key_path.display()
        ),
        Err(e) => tracing::error!(
            "Failed to persist proxy secret to {}: {e}. \
             Cover URLs will break on next restart.",
            key_path.display()
        ),
    }
    secret
}

/// Seal a (url, referer) pair into an opaque token that belongs to no source.
#[cfg(test)]
pub(crate) fn seal_proxy_token(url: &str, referer: &str, secret: &[u8; 32]) -> String {
    seal_source_proxy_token(None, url, referer, secret)
}

/// Seal a (source, url, referer) triple. The proxy fetches with the source's own client, so a
/// source's local-network grant covers its images and no other source's.
pub(crate) fn seal_source_proxy_token(
    source_id: Option<i64>,
    url: &str,
    referer: &str,
    secret: &[u8; 32],
) -> String {
    let source = source_id.map(|id| id.to_string()).unwrap_or_default();
    let plaintext = format!("{TOKEN_VERSION}|{source}|{url}|{referer}");

    let mut mac =
        <HmacSha256 as hmac::Mac>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(b"nonce|");
    mac.update(plaintext.as_bytes());
    let digest = mac.finalize().into_bytes();
    let nonce_bytes: [u8; NONCE_LEN] = digest[..NONCE_LEN]
        .try_into()
        .expect("HMAC-SHA256 output is >= 12 bytes");

    let nonce = Nonce::from_slice(&nonce_bytes);
    let cipher = ChaCha20Poly1305::new(secret.into());
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .expect("ChaCha20Poly1305 encryption is infallible for valid key/nonce");

    let mut token = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    token.extend_from_slice(&nonce_bytes);
    token.extend_from_slice(&ciphertext);
    URL_SAFE_NO_PAD.encode(token)
}

/// Unseal a token, returning `(url, referer)` if it is authentic.
#[cfg(test)]
pub(crate) fn unseal_proxy_token(token: &str, secret: &[u8; 32]) -> Option<(String, String)> {
    unseal_proxy_token_with_source(token, secret).map(|(_, url, referer)| (url, referer))
}

/// Unseal a token, returning `(source, url, referer)` if it is authentic.
pub(crate) fn unseal_proxy_token_with_source(
    token: &str,
    secret: &[u8; 32],
) -> Option<(Option<i64>, String, String)> {
    let raw = URL_SAFE_NO_PAD.decode(token).ok()?;
    if raw.len() <= NONCE_LEN {
        return None;
    }

    let (nonce_bytes, ciphertext) = raw.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = ChaCha20Poly1305::new(secret.into());

    let plaintext = cipher.decrypt(nonce, ciphertext).ok()?;
    let s = String::from_utf8(plaintext).ok()?;

    let (source, body) = if let Some(rest) = s.strip_prefix(TOKEN_VERSION) {
        let (source, body) = rest.strip_prefix('|')?.split_once('|')?;
        let source = match source {
            "" => None,
            id => Some(id.parse::<i64>().ok()?),
        };
        (source, body)
    } else {
        (
            None,
            s.strip_prefix(LEGACY_TOKEN_VERSION)?.strip_prefix('|')?,
        )
    };

    let mut parts = body.splitn(2, '|');
    let url = parts.next()?.to_string();
    let referer = parts.next()?.to_string();

    Some((source, url, referer))
}

/// Compute a stable, server-signed ETag for a (url, referer) pair.
pub(crate) fn compute_etag(url: &str, referer: &str, secret: &[u8; 32]) -> String {
    let mut mac =
        <HmacSha256 as hmac::Mac>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(b"etag|");
    mac.update(url.as_bytes());
    mac.update(b"|");
    mac.update(referer.as_bytes());
    format!("\"{}\"", hex::encode(mac.finalize().into_bytes()))
}

pub fn range_response_status(is_partial: bool) -> axum::http::StatusCode {
    if is_partial {
        axum::http::StatusCode::PARTIAL_CONTENT
    } else {
        axum::http::StatusCode::OK
    }
}

pub fn build_range_response_headers(
    upstream: &rquest::header::HeaderMap,
    etag: &str,
) -> axum::http::HeaderMap {
    let mut out = axum::http::HeaderMap::new();
    if let Some(ct) = upstream.get(rquest::header::CONTENT_TYPE)
        && let Ok(hv) = axum::http::header::HeaderValue::from_bytes(ct.as_bytes())
    {
        out.insert(axum::http::header::CONTENT_TYPE, hv);
    }
    if let Some(cr) = upstream.get(rquest::header::CONTENT_RANGE)
        && let Ok(hv) = axum::http::header::HeaderValue::from_bytes(cr.as_bytes())
    {
        out.insert(axum::http::header::CONTENT_RANGE, hv);
    }
    if let Ok(ev) = axum::http::header::HeaderValue::from_str(etag) {
        out.insert(axum::http::header::ETAG, ev);
    }
    out
}

/// Tunable knobs for the image proxy's fetch path. Production uses
/// [`ProxyConfig::default`]; tests override it on `AppState` to drive the retry,
/// timeout, coalescing and cap paths without multi-second backoff or 50 MB bodies.
#[derive(Clone, Copy)]
pub struct ProxyConfig {
    pub max_retries: u32,
    pub base_delay: std::time::Duration,
    pub retry_jitter: std::time::Duration,
    pub retry_after_cap: std::time::Duration,
    pub request_timeout: std::time::Duration,
    pub per_host_concurrency: usize,
    pub min_host_interval: std::time::Duration,
    pub max_image_bytes: usize,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: std::time::Duration::from_secs(2),
            retry_jitter: std::time::Duration::from_millis(1000),
            retry_after_cap: std::time::Duration::from_secs(60),
            request_timeout: std::time::Duration::from_secs(35),
            per_host_concurrency: 5,
            min_host_interval: std::time::Duration::from_millis(20),
            max_image_bytes: 50 * 1024 * 1024,
        }
    }
}

pub fn make_proxy_url(
    url: &str,
    referer: &str,
    source_id: Option<i64>,
    secret: &[u8; 32],
    transform: Option<&str>,
) -> String {
    let token = seal_source_proxy_token(source_id, url, referer, secret);
    match transform {
        Some(t) if !t.is_empty() => format!(
            "/rest/image_proxy?token={}&transform={}",
            token,
            urlencoding::encode(t),
        ),
        _ => format!("/rest/image_proxy?token={}", token),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

    #[test]
    fn persists_on_first_boot_and_same_on_second() {
        let dir = tempfile::tempdir().unwrap();
        let s1 = load_or_persist_secret(dir.path());
        let s2 = load_or_persist_secret(dir.path());
        assert_eq!(
            s1, s2,
            "second call must return the persisted secret, not a new one"
        );
        assert!(
            dir.path().join("proxy.key").exists(),
            "proxy.key must be created"
        );
    }

    #[test]
    fn env_var_takes_precedence_over_file() {
        let dir = tempfile::tempdir().unwrap();
        let known: [u8; 32] = [0x55u8; 32];
        let encoded = URL_SAFE_NO_PAD.encode(known);
        std::fs::write(
            dir.path().join("proxy.key"),
            URL_SAFE_NO_PAD.encode([0xAAu8; 32]),
        )
        .unwrap();
        // SAFETY: no other thread may access the environment during the write. The harness does
        // not serialise this against auth.rs's KANI_DATA_DIR test; the keys are disjoint.
        unsafe { std::env::set_var("KANI_PROXY_SECRET", &encoded) };
        let result = load_or_persist_secret(dir.path());
        // SAFETY: as above; clears the key so later tests read the file instead.
        unsafe { std::env::remove_var("KANI_PROXY_SECRET") };
        assert_eq!(result, known, "env var must override the file");
    }

    fn secret() -> [u8; 32] {
        [0xABu8; 32]
    }
    fn other_secret() -> [u8; 32] {
        [0xCDu8; 32]
    }

    #[test]
    fn roundtrip_basic() {
        let s = secret();
        let token = seal_proxy_token("https://img.example.com/a.jpg", "https://example.com", &s);
        assert_eq!(
            unseal_proxy_token(&token, &s),
            Some((
                "https://img.example.com/a.jpg".into(),
                "https://example.com".into()
            ))
        );
    }

    #[test]
    fn roundtrip_empty_referer() {
        let s = secret();
        let token = seal_proxy_token("https://img.example.com/a.jpg", "", &s);
        assert_eq!(
            unseal_proxy_token(&token, &s),
            Some(("https://img.example.com/a.jpg".into(), "".into()))
        );
    }

    #[test]
    fn roundtrip_special_chars_in_url() {
        let s = secret();
        let url = "https://cdn.example.com/path?size=800&quality=90";
        let referer = "https://example.com/manga/chapter/1";
        let token = seal_proxy_token(url, referer, &s);
        assert_eq!(
            unseal_proxy_token(&token, &s),
            Some((url.into(), referer.into()))
        );
    }

    #[test]
    fn roundtrip_unicode_in_url() {
        let s = secret();
        let url = "https://example.com/\u{753b}\u{50cf}/test.jpg";
        let referer = "https://example.com/\u{6f2b}\u{753b}/1";
        let token = seal_proxy_token(url, referer, &s);
        assert_eq!(
            unseal_proxy_token(&token, &s),
            Some((url.into(), referer.into()))
        );
    }

    #[test]
    fn roundtrip_pipe_in_referer() {
        let s = secret();
        let url = "https://cdn.example.com/img.jpg";
        let referer = "https://example.com/page|with|pipes";
        let token = seal_proxy_token(url, referer, &s);
        assert_eq!(
            unseal_proxy_token(&token, &s),
            Some((url.into(), referer.into()))
        );
    }

    #[test]
    fn a_v3_token_carries_its_source() {
        let s = secret();
        let token = seal_source_proxy_token(Some(42), "https://img.example.com/a.jpg", "ref", &s);
        assert_eq!(
            unseal_proxy_token_with_source(&token, &s),
            Some((
                Some(42),
                "https://img.example.com/a.jpg".into(),
                "ref".into()
            ))
        );
        let token = seal_source_proxy_token(None, "https://img.example.com/a.jpg", "ref", &s);
        assert_eq!(unseal_proxy_token_with_source(&token, &s).unwrap().0, None);
    }

    #[test]
    fn a_v2_token_is_still_read_as_belonging_to_no_source() {
        let s = secret();
        let plaintext = "v2|https://img.example.com/a.jpg|ref";
        let nonce_bytes = [7u8; NONCE_LEN];
        let cipher = ChaCha20Poly1305::new((&s).into());
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
            .unwrap();
        let mut raw = nonce_bytes.to_vec();
        raw.extend_from_slice(&ciphertext);
        let token = URL_SAFE_NO_PAD.encode(raw);
        assert_eq!(
            unseal_proxy_token_with_source(&token, &s),
            Some((None, "https://img.example.com/a.jpg".into(), "ref".into()))
        );
    }

    #[test]
    fn wrong_secret_returns_none() {
        let token = seal_proxy_token("https://img.example.com/a.jpg", "ref", &secret());
        assert_eq!(unseal_proxy_token(&token, &other_secret()), None);
    }

    #[test]
    fn different_secrets_produce_different_tokens() {
        let url = "https://img.example.com/a.jpg";
        let t1 = seal_proxy_token(url, "ref", &secret());
        let t2 = seal_proxy_token(url, "ref", &other_secret());
        assert_ne!(t1, t2);
    }

    #[test]
    fn empty_token_returns_none() {
        assert_eq!(unseal_proxy_token("", &secret()), None);
    }

    #[test]
    fn garbage_token_returns_none() {
        assert_eq!(unseal_proxy_token("not-a-real-token!!", &secret()), None);
    }

    #[test]
    fn truncated_token_returns_none() {
        let token = seal_proxy_token("https://img.example.com/a.jpg", "ref", &secret());
        let truncated = &token[..token.len() / 2];
        assert_eq!(unseal_proxy_token(truncated, &secret()), None);
    }

    #[test]
    fn modified_ciphertext_returns_none() {
        let s = secret();
        let token = seal_proxy_token("https://img.example.com/a.jpg", "ref", &s);
        let mut raw = URL_SAFE_NO_PAD.decode(&token).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0xFF;
        let bad_token = URL_SAFE_NO_PAD.encode(&raw);
        assert_eq!(unseal_proxy_token(&bad_token, &s), None);
    }

    #[test]
    fn modified_nonce_returns_none() {
        let s = secret();
        let token = seal_proxy_token("https://img.example.com/a.jpg", "ref", &s);
        let mut raw = URL_SAFE_NO_PAD.decode(&token).unwrap();
        raw[0] ^= 0xFF;
        let bad_token = URL_SAFE_NO_PAD.encode(&raw);
        assert_eq!(unseal_proxy_token(&bad_token, &s), None);
    }

    #[test]
    fn token_contains_only_base64url_chars() {
        let token = seal_proxy_token("https://img.example.com/a.jpg", "ref", &secret());
        assert!(
            token
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn fresh_token_is_valid() {
        let s = secret();
        let token = seal_proxy_token("https://img.example.com/a.jpg", "ref", &s);
        assert!(unseal_proxy_token(&token, &s).is_some());
    }

    fn seal_legacy_expiring_token(url: &str, referer: &str, secret: &[u8; 32]) -> String {
        let expiry = time::OffsetDateTime::now_utc().unix_timestamp() + 3600;
        let plaintext = format!("{}|{}|{}", url, referer, expiry);

        let mut mac =
            <HmacSha256 as hmac::Mac>::new_from_slice(secret).expect("HMAC accepts any key length");
        mac.update(b"nonce|");
        mac.update(plaintext.as_bytes());
        let digest = mac.finalize().into_bytes();
        let nonce_bytes: [u8; NONCE_LEN] = digest[..NONCE_LEN].try_into().expect("HMAC is >= 12");

        let nonce = Nonce::from_slice(&nonce_bytes);
        let cipher = ChaCha20Poly1305::new(secret.into());
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .expect("infallible");

        let mut token = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        token.extend_from_slice(&nonce_bytes);
        token.extend_from_slice(&ciphertext);
        URL_SAFE_NO_PAD.encode(token)
    }

    #[test]
    fn a_token_is_byte_identical_however_often_it_is_minted() {
        let s = secret();
        let first = seal_proxy_token("https://img.example.com/a.jpg", "ref", &s);
        let second = seal_proxy_token("https://img.example.com/a.jpg", "ref", &s);
        assert_eq!(
            first, second,
            "the same (url, referer) must always produce the same token"
        );
    }

    #[test]
    fn a_legacy_expiring_token_is_refused_rather_than_misread() {
        let s = secret();
        let legacy = seal_legacy_expiring_token("https://img.example.com/a.jpg", "ref", &s);
        assert_eq!(
            unseal_proxy_token(&legacy, &s),
            None,
            "a legacy token must be rejected outright"
        );
    }

    #[test]
    fn seal_empty_url() {
        let s = secret();
        let token = seal_proxy_token("", "ref", &s);
        let result = unseal_proxy_token(&token, &s);
        assert_eq!(result, Some(("".into(), "ref".into())));
    }

    #[test]
    fn seal_long_url() {
        let s = secret();
        let url = format!("https://example.com/{}", "a".repeat(1000));
        let token = seal_proxy_token(&url, "ref", &s);
        assert_eq!(unseal_proxy_token(&token, &s), Some((url, "ref".into())));
    }

    #[test]
    fn etag_is_deterministic() {
        let s = secret();
        let e1 = compute_etag("https://img.example.com/a.jpg", "ref", &s);
        let e2 = compute_etag("https://img.example.com/a.jpg", "ref", &s);
        assert_eq!(e1, e2);
    }

    #[test]
    fn etag_differs_by_url() {
        let s = secret();
        let e1 = compute_etag("https://img1.example.com/a.jpg", "ref", &s);
        let e2 = compute_etag("https://img2.example.com/b.jpg", "ref", &s);
        assert_ne!(e1, e2);
    }

    #[test]
    fn etag_differs_by_referer() {
        let s = secret();
        let e1 = compute_etag("https://img.example.com/a.jpg", "ref1", &s);
        let e2 = compute_etag("https://img.example.com/a.jpg", "ref2", &s);
        assert_ne!(e1, e2);
    }

    #[test]
    fn etag_is_quoted_hex() {
        let etag = compute_etag("https://img.example.com/a.jpg", "ref", &secret());
        assert!(etag.starts_with('"') && etag.ends_with('"'));
        let inner = &etag[1..etag.len() - 1];
        assert!(!inner.is_empty());
        assert!(inner.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn canonical_key_strips_single_bust_param() {
        let url = "https://cdn.example.com/img.jpg?cb=12345";
        assert_eq!(canonical_proxy_key(url), "https://cdn.example.com/img.jpg");
    }

    #[test]
    fn canonical_key_strips_all_bust_params() {
        let url = "https://cdn.example.com/img.jpg?cb=1&ts=2&t=3&_=4&v=5&ver=6&nc=7";
        assert_eq!(canonical_proxy_key(url), "https://cdn.example.com/img.jpg");
    }

    #[test]
    fn canonical_key_preserves_non_bust_params() {
        let url = "https://cdn.example.com/img.jpg?size=800&quality=90&cb=99";
        let result = canonical_proxy_key(url);
        assert!(result.contains("size=800"));
        assert!(result.contains("quality=90"));
        assert!(!result.contains("cb="));
    }

    #[test]
    fn canonical_key_no_query_unchanged() {
        let url = "https://cdn.example.com/img.jpg";
        assert_eq!(canonical_proxy_key(url), url);
    }

    #[test]
    fn canonical_key_only_non_bust_params_unchanged() {
        let url = "https://cdn.example.com/img.jpg?page=2";
        assert_eq!(canonical_proxy_key(url), url);
    }

    #[test]
    fn canonical_key_invalid_url_passthrough() {
        let url = "not a url at all !!";
        assert_eq!(canonical_proxy_key(url), url);
    }
}
