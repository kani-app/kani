use std::hash::{Hash, Hasher};

use axum::http::{Request, header::AUTHORIZATION};
use tower_governor::{GovernorError, key_extractor::KeyExtractor};

/// Rate-limit bucket identity.
///
/// Bearer-authenticated traffic is bucketed per token rather than per peer IP.
/// This prevents an integration from consuming its owner's browser budget.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RateKey {
    /// Hash of the presented bearer, never the token itself: this value is used
    /// as a map key and appears in error paths.
    Token(u64),
    Peer(std::net::IpAddr),
}

#[derive(Clone)]
/// Tower Governor key extractor preferring a hashed bearer token, else the resolved client
/// address. Behind a trusted proxy the socket peer is the proxy, so keying on it would put every
/// anonymous caller in one bucket; it resolves through [`crate::client_ip::client_ip`] instead.
pub struct TokenOrPeerIp {
    trusted: std::sync::Arc<crate::client_ip::TrustedProxies>,
}

impl TokenOrPeerIp {
    pub fn new(trusted: std::sync::Arc<crate::client_ip::TrustedProxies>) -> Self {
        Self { trusted }
    }
}

fn hash_bearer(raw: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    raw.hash(&mut hasher);
    hasher.finish()
}

impl KeyExtractor for TokenOrPeerIp {
    type Key = RateKey;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, GovernorError> {
        if let Some(raw) = req
            .headers()
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .filter(|v| !v.is_empty())
        {
            return Ok(RateKey::Token(hash_bearer(raw)));
        }

        let peer = req
            .extensions()
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|info| info.0.ip());
        crate::client_ip::client_ip(req.headers(), peer, &self.trusted)
            .map(RateKey::Peer)
            .ok_or(GovernorError::UnableToExtractKey)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn extractor() -> TokenOrPeerIp {
        TokenOrPeerIp::new(std::sync::Arc::new(Default::default()))
    }

    fn req_with_auth(value: Option<&str>) -> Request<()> {
        let mut b = Request::builder().uri("/rest/sources");
        if let Some(v) = value {
            b = b.header(AUTHORIZATION, v);
        }
        b.body(()).unwrap()
    }

    fn extractor_trusting(spec: &str) -> TokenOrPeerIp {
        let (trusted, rejected) = crate::client_ip::TrustedProxies::parse(spec);
        assert!(rejected.is_empty());
        TokenOrPeerIp::new(std::sync::Arc::new(trusted))
    }

    fn req_from(peer: &str, forwarded: Option<&str>) -> Request<()> {
        let mut b = Request::builder().uri("/rest/sources");
        if let Some(v) = forwarded {
            b = b.header("x-forwarded-for", v);
        }
        let mut req = b.body(()).unwrap();
        let addr: std::net::SocketAddr = peer.parse().unwrap();
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo(addr));
        req
    }

    #[test]
    fn behind_a_trusted_proxy_clients_do_not_share_one_bucket() {
        let e = extractor_trusting("198.51.100.0/24");
        let a = e
            .extract(&req_from("198.51.100.7:1", Some("203.0.113.1")))
            .unwrap();
        let b = e
            .extract(&req_from("198.51.100.7:2", Some("203.0.113.2")))
            .unwrap();
        assert_ne!(
            a, b,
            "keying on the proxy's socket address would collapse every client into one bucket"
        );
    }

    #[test]
    fn an_untrusted_peer_cannot_split_its_bucket_with_a_header() {
        let e = extractor();
        let a = e
            .extract(&req_from("203.0.113.9:1", Some("1.1.1.1")))
            .unwrap();
        let b = e
            .extract(&req_from("203.0.113.9:2", Some("2.2.2.2")))
            .unwrap();
        assert_eq!(
            a, b,
            "rotating X-Forwarded-For must not mint a fresh rate-limit budget"
        );
    }

    #[test]
    fn distinct_tokens_get_distinct_buckets() {
        let a = extractor().extract(&req_with_auth(Some("Bearer kani_aaa")));
        let b = extractor().extract(&req_with_auth(Some("Bearer kani_bbb")));
        assert!(a.is_ok() && b.is_ok());
        assert_ne!(a.unwrap(), b.unwrap());
    }

    #[test]
    fn the_same_token_reuses_one_bucket() {
        let a = extractor().extract(&req_with_auth(Some("Bearer kani_aaa")));
        let b = extractor().extract(&req_with_auth(Some("Bearer kani_aaa")));
        assert_eq!(a.unwrap(), b.unwrap());
    }

    #[test]
    fn the_raw_token_is_never_the_key() {
        let key = extractor()
            .extract(&req_with_auth(Some("Bearer kani_supersecret")))
            .unwrap();
        assert!(
            !format!("{key:?}").contains("supersecret"),
            "the bucket key must not carry the credential"
        );
    }

    #[test]
    fn a_request_without_a_bearer_falls_back_to_peer_ip() {
        assert!(extractor().extract(&req_with_auth(None)).is_err());
        assert!(
            extractor()
                .extract(&req_with_auth(Some("Basic abc")))
                .is_err(),
            "only Bearer opts into token bucketing"
        );
    }
}
