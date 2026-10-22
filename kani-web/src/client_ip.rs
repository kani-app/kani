//! Client address resolution behind an optional reverse proxy.
//!
//! `X-Forwarded-For` is attacker-controlled on any request that did not come through a proxy we
//! run, so it is consulted only when the immediate peer is a configured trusted proxy. Everything
//! that buckets or locks out by address must resolve it through [`client_ip`], or two call sites
//! will disagree about who the caller is.

use std::net::{IpAddr, SocketAddr};

use axum::http::HeaderMap;

/// Networks permitted to speak for a client via `X-Forwarded-For`.
///
/// Empty by default, which means no header is ever believed and the socket peer is always the
/// client. Populate it only with proxies you operate or pay for; trusting a network you do not
/// control hands it the ability to forge any client address.
#[derive(Clone, Debug, Default)]
pub struct TrustedProxies {
    nets: Vec<(IpAddr, u8)>,
}

impl TrustedProxies {
    /// Reads `KANI_TRUSTED_PROXIES`: a comma-separated list of addresses or CIDR blocks.
    /// Unparseable entries are returned rather than ignored so the caller can refuse to start.
    pub(crate) fn from_env() -> (Self, Vec<String>) {
        match std::env::var("KANI_TRUSTED_PROXIES") {
            Ok(spec) => Self::parse(&spec),
            Err(_) => (Self::default(), Vec::new()),
        }
    }

    pub fn parse(spec: &str) -> (Self, Vec<String>) {
        let mut nets = Vec::new();
        let mut rejected = Vec::new();
        for raw in spec.split(',') {
            let entry = raw.trim();
            if entry.is_empty() {
                continue;
            }
            match parse_net(entry) {
                Some(net) => nets.push(net),
                None => rejected.push(entry.to_string()),
            }
        }
        (Self { nets }, rejected)
    }

    pub fn is_empty(&self) -> bool {
        self.nets.is_empty()
    }

    pub fn len(&self) -> usize {
        self.nets.len()
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = canonical(ip);
        self.nets
            .iter()
            .any(|(net, prefix)| in_net(ip, *net, *prefix))
    }
}

/// Resolves the address to attribute a request to.
///
/// Returns `None` only when the peer is unknown, which callers must treat as "cannot rate-limit"
/// rather than as a shared bucket. When the peer is trusted, the rightmost `X-Forwarded-For`
/// entry that is not itself a trusted proxy is the client; anything further left was written by
/// a hop we do not vouch for.
pub(crate) fn client_ip(
    headers: &HeaderMap,
    peer: Option<IpAddr>,
    trusted: &TrustedProxies,
) -> Option<IpAddr> {
    let peer = canonical(peer?);
    if !trusted.contains(peer) {
        return Some(peer);
    }
    let forwarded = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    forwarded
        .split(',')
        .filter_map(|hop| parse_hop(hop.trim()))
        .rev()
        .find(|hop| !trusted.contains(*hop))
        .or(Some(peer))
}

/// [`client_ip`] rendered for storage, falling back to a sentinel when the peer is unknown.
pub fn client_ip_string(
    headers: &HeaderMap,
    peer: Option<IpAddr>,
    trusted: &TrustedProxies,
) -> String {
    client_ip(headers, peer, trusted)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn parse_hop(hop: &str) -> Option<IpAddr> {
    let hop = hop.trim();
    if let Ok(ip) = hop.parse::<IpAddr>() {
        return Some(canonical(ip));
    }
    if let Ok(addr) = hop.parse::<SocketAddr>() {
        return Some(canonical(addr.ip()));
    }
    let unbracketed = hop.strip_prefix('[')?.split(']').next()?;
    unbracketed.parse::<IpAddr>().ok().map(canonical)
}

fn parse_net(entry: &str) -> Option<(IpAddr, u8)> {
    match entry.split_once('/') {
        None => {
            let ip = entry.parse::<IpAddr>().ok()?;
            let full = if ip.is_ipv4() { 32 } else { 128 };
            Some((canonical(ip), full))
        }
        Some((addr, len)) => {
            let ip = canonical(addr.trim().parse::<IpAddr>().ok()?);
            let prefix: u8 = len.trim().parse().ok()?;
            let max = if ip.is_ipv4() { 32 } else { 128 };
            (prefix <= max).then_some((ip, prefix))
        }
    }
}

/// Collapses an IPv4-mapped IPv6 address so a dual-stack listener matches IPv4 rules.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        v4 => v4,
    }
}

fn in_net(ip: IpAddr, net: IpAddr, prefix: u8) -> bool {
    if prefix == 0 {
        return matches!((ip, net), (IpAddr::V4(_), IpAddr::V4(_)))
            || matches!((ip, net), (IpAddr::V6(_), IpAddr::V6(_)));
    }
    match (ip, net) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            let mask = u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0);
            (u32::from(a) & mask) == (u32::from(b) & mask)
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            let mask = u128::MAX.checked_shl(128 - u32::from(prefix)).unwrap_or(0);
            (u128::from(a) & mask) == (u128::from(b) & mask)
        }
        _ => false,
    }
}

/// The resolved client address, or `None` when the peer is unknown.
///
/// Extracting this is the only supported way for a handler to learn who is calling: it applies
/// the trusted-proxy rules from [`AppState`](crate::state::AppState) so no handler can decide to
/// believe a header on its own. Extraction never fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClientIp(pub Option<IpAddr>);

impl ClientIp {
    /// Rendered for storage, with the same sentinel [`client_ip_string`] uses.
    pub(crate) fn to_key(self) -> String {
        self.0
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }
}

impl axum::extract::FromRequestParts<crate::state::AppState> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &crate::state::AppState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<axum::extract::ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        Ok(ClientIp(client_ip(
            &parts.headers,
            peer,
            &state.trusted_proxies,
        )))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn xff(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", value.parse().unwrap());
        h
    }

    fn trusted(spec: &str) -> TrustedProxies {
        let (t, rejected) = TrustedProxies::parse(spec);
        assert!(rejected.is_empty(), "spec should parse: {rejected:?}");
        t
    }

    #[test]
    fn an_untrusted_peer_cannot_forge_its_address() {
        let t = trusted("10.0.0.1");
        let got = client_ip(&xff("1.2.3.4"), Some(ip("203.0.113.9")), &t);
        assert_eq!(
            got,
            Some(ip("203.0.113.9")),
            "a direct caller's X-Forwarded-For must be ignored entirely"
        );
    }

    #[test]
    fn with_no_trusted_proxies_the_header_is_never_believed() {
        let t = TrustedProxies::default();
        assert!(t.is_empty());
        let got = client_ip(&xff("1.2.3.4"), Some(ip("203.0.113.9")), &t);
        assert_eq!(got, Some(ip("203.0.113.9")));
    }

    #[test]
    fn a_trusted_peer_yields_the_forwarded_client() {
        let t = trusted("10.0.0.0/8");
        let got = client_ip(&xff("203.0.113.9"), Some(ip("10.1.2.3")), &t);
        assert_eq!(got, Some(ip("203.0.113.9")));
    }

    #[test]
    fn the_rightmost_untrusted_hop_wins_over_a_spoofed_prefix() {
        let t = trusted("10.0.0.0/8");
        // The client prepended a lie; the real address is the one our proxy appended.
        let got = client_ip(&xff("9.9.9.9, 203.0.113.9"), Some(ip("10.1.2.3")), &t);
        assert_eq!(
            got,
            Some(ip("203.0.113.9")),
            "taking the leftmost entry would return the forged 9.9.9.9"
        );
    }

    #[test]
    fn chained_trusted_proxies_are_skipped() {
        let t = trusted("10.0.0.0/8, 172.16.0.0/12");
        let got = client_ip(
            &xff("203.0.113.9, 172.16.5.5, 10.1.2.3"),
            Some(ip("10.9.9.9")),
            &t,
        );
        assert_eq!(got, Some(ip("203.0.113.9")));
    }

    #[test]
    fn a_trusted_peer_with_no_header_falls_back_to_the_peer() {
        let t = trusted("10.0.0.0/8");
        let got = client_ip(&HeaderMap::new(), Some(ip("10.1.2.3")), &t);
        assert_eq!(got, Some(ip("10.1.2.3")));
    }

    #[test]
    fn an_all_trusted_chain_falls_back_to_the_peer() {
        let t = trusted("10.0.0.0/8");
        let got = client_ip(&xff("10.1.1.1, 10.2.2.2"), Some(ip("10.3.3.3")), &t);
        assert_eq!(got, Some(ip("10.3.3.3")));
    }

    #[test]
    fn an_unknown_peer_is_not_a_shared_bucket() {
        let t = trusted("10.0.0.0/8");
        assert_eq!(client_ip(&xff("1.2.3.4"), None, &t), None);
        assert_eq!(client_ip_string(&xff("1.2.3.4"), None, &t), "unknown");
    }

    #[test]
    fn garbage_hops_are_skipped_not_trusted() {
        let t = trusted("10.0.0.0/8");
        let got = client_ip(&xff("not-an-ip, 203.0.113.9"), Some(ip("10.1.2.3")), &t);
        assert_eq!(got, Some(ip("203.0.113.9")));
    }

    #[test]
    fn a_hop_with_a_port_is_understood() {
        let t = trusted("10.0.0.0/8");
        assert_eq!(
            client_ip(&xff("203.0.113.9:51234"), Some(ip("10.1.2.3")), &t),
            Some(ip("203.0.113.9"))
        );
        assert_eq!(
            client_ip(&xff("[2001:db8::1]:443"), Some(ip("10.1.2.3")), &t),
            Some(ip("2001:db8::1"))
        );
    }

    #[test]
    fn an_ipv4_mapped_peer_matches_an_ipv4_rule() {
        let t = trusted("10.0.0.0/8");
        let got = client_ip(&xff("203.0.113.9"), Some(ip("::ffff:10.1.2.3")), &t);
        assert_eq!(
            got,
            Some(ip("203.0.113.9")),
            "a dual-stack listener reports IPv4 peers as IPv4-mapped IPv6"
        );
    }

    #[test]
    fn ipv6_networks_match_on_their_prefix() {
        let t = trusted("2001:db8::/32");
        assert!(t.contains(ip("2001:db8:dead:beef::1")));
        assert!(!t.contains(ip("2001:db9::1")));
    }

    #[test]
    fn a_bare_address_matches_only_itself() {
        let t = trusted("10.0.0.1");
        assert!(t.contains(ip("10.0.0.1")));
        assert!(!t.contains(ip("10.0.0.2")));
    }

    #[test]
    fn families_never_cross_match() {
        let t = trusted("0.0.0.0/0");
        assert!(t.contains(ip("203.0.113.9")));
        assert!(!t.contains(ip("2001:db8::1")));
    }

    #[test]
    fn malformed_entries_are_reported_rather_than_silently_dropped() {
        let (t, rejected) = TrustedProxies::parse("10.0.0.0/8, nonsense, 1.2.3.4/99, ,2001:db8::1");
        assert_eq!(t.len(), 2, "only the two valid entries are kept");
        assert_eq!(
            rejected,
            vec!["nonsense".to_string(), "1.2.3.4/99".to_string()]
        );
    }
}
