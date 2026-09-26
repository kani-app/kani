//! Network-related security utilities.

use crate::error::Result;
use hickory_resolver::name_server::GenericConnector;
use hickory_resolver::proto::runtime::TokioRuntimeProvider;
use hickory_resolver::{TokioResolver, config::*};
use rquest::dns::{Addrs, Name, Resolve, Resolving};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

/// Every private, reserved, or otherwise untrusted IP range.
pub fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_documentation()
                || v4.is_multicast()
                || is_cgnat(v4)
                || is_reserved(v4)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || is_ipv4_mapped_private(v6)
        }
    }
}

fn is_cgnat(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (octets[1] & 0xC0) == 64
}

fn is_reserved(ip: Ipv4Addr) -> bool {
    ip.octets()[0] >= 240
}

/// IPv4-mapped IPv6 (::ffff:0:0/96) can encode private IPv4 addresses.
fn is_ipv4_mapped_private(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_forbidden_ip(IpAddr::V4(v4));
    }
    false
}

/// Whether a URL's host is an IP literal in a forbidden range.
///
/// The [`ValidatingResolver`] guards hostnames (and DNS rebinding) at resolve
/// time, but it is never consulted for a URL that already names a literal IP —
/// the connector dials those directly. So `http://169.254.169.254/` or
/// `http://127.0.0.1:9000/` would bypass it entirely. This closes that gap:
/// callers that accept a user-supplied URL (webhooks, trackers) reject a
/// forbidden IP literal before dialling. Returns `false` for hostnames (left to
/// the resolver) and for URLs that do not parse or carry no host.
pub fn is_forbidden_url_host(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    match parsed.host() {
        Some(url::Host::Ipv4(v4)) => is_forbidden_ip(IpAddr::V4(v4)),
        Some(url::Host::Ipv6(v6)) => is_forbidden_ip(IpAddr::V6(v6)),
        _ => false,
    }
}

/// Whether a URL's host is a loopback IP literal (`127.0.0.0/8` or `::1`).
pub fn is_loopback_url_host(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    match parsed.host() {
        Some(url::Host::Ipv4(v4)) => v4.is_loopback(),
        Some(url::Host::Ipv6(v6)) => v6.is_loopback(),
        _ => false,
    }
}

/// Build an HTTP client that refuses to reach private/reserved hosts, for
/// server-initiated egress to user-supplied URLs (webhooks). Redirects are
/// disabled so a `3xx` cannot bounce the request to an internal host that the
/// literal-host check never saw; the [`ValidatingResolver`] still guards every
/// hostname it does resolve.
pub fn build_validating_client() -> Result<rquest::Client> {
    let resolver = ValidatingResolver::new()?;
    let client = rquest::Client::builder()
        .redirect(rquest::redirect::Policy::none())
        .dns_resolver(Arc::new(resolver))
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    Ok(client)
}

/// Addresses no local-network grant can open: loopback (Kani itself), link-local (including
/// cloud metadata), unspecified and multicast.
pub fn is_never_grantable_ip(ip: IpAddr) -> bool {
    let ip = match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    };
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() || v4.is_multicast()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalGrant {
    host: String,
    port: Option<u16>,
}

/// Private hosts an operator has allowed one source to reach, as `host` or `host:port`.
///
/// A grant exempts only that host from the private-address refusal; loopback, link-local,
/// metadata and similar ranges stay refused ([`is_never_grantable_ip`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalGrants {
    grants: Vec<LocalGrant>,
}

fn grant_host(host: url::Host<&str>) -> String {
    match host {
        url::Host::Domain(d) => d.trim_end_matches('.').to_ascii_lowercase(),
        url::Host::Ipv4(v4) => v4.to_string(),
        url::Host::Ipv6(v6) => format!("[{v6}]"),
    }
}

impl LocalGrants {
    /// Parses operator entries, refusing any that could never be honoured.
    pub fn parse(entries: &[String]) -> std::result::Result<Self, String> {
        let mut grants = Vec::new();
        for raw in entries {
            let entry = raw.trim();
            let parsed = url::Url::parse(&format!("http://{entry}/"))
                .ok()
                .filter(|u| u.path() == "/" && u.username().is_empty() && u.query().is_none())
                .ok_or_else(|| format!("'{entry}' is not a host or host:port"))?;
            let host = parsed
                .host()
                .ok_or_else(|| format!("'{entry}' has no host"))?;
            let never = match host {
                url::Host::Domain(d) => {
                    let d = d.trim_end_matches('.').to_ascii_lowercase();
                    d == "localhost" || d.ends_with(".localhost")
                }
                url::Host::Ipv4(v4) => is_never_grantable_ip(IpAddr::V4(v4)),
                url::Host::Ipv6(v6) => is_never_grantable_ip(IpAddr::V6(v6)),
            };
            if never {
                return Err(format!(
                    "'{entry}' is loopback, link-local or otherwise cannot be granted"
                ));
            }
            grants.push(LocalGrant {
                host: grant_host(host),
                port: parsed.port(),
            });
        }
        Ok(Self { grants })
    }

    pub fn is_empty(&self) -> bool {
        self.grants.is_empty()
    }

    /// Whether a host name is granted on any port; used where no port is known (DNS).
    pub fn permits_host(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        self.grants.iter().any(|g| g.host == host)
    }

    /// Whether a URL's host and port are granted. A grant without a port covers any port.
    pub fn permits_url(&self, url: &str) -> bool {
        let Ok(parsed) = url::Url::parse(url) else {
            return false;
        };
        let Some(host) = parsed.host() else {
            return false;
        };
        let host = grant_host(host);
        let port = parsed.port_or_known_default();
        self.grants
            .iter()
            .any(|g| g.host == host && (g.port.is_none() || g.port == port))
    }
}

/// A DNS resolver that validates every address before returning it.
///
/// A name the source has been granted ([`LocalGrants`]) is resolved through the system
/// resolver instead, so LAN names and `/etc/hosts` entries work, and may resolve to a private
/// address; never-grantable ranges are still refused.
#[derive(Clone)]
pub struct ValidatingResolver {
    inner: Arc<TokioResolver>,
    grants: Arc<LocalGrants>,
}

impl ValidatingResolver {
    pub fn new() -> Result<Self> {
        let resolver = TokioResolver::builder_with_config(
            ResolverConfig::cloudflare(),
            GenericConnector::new(TokioRuntimeProvider::new()),
        )
        .with_options(ResolverOpts::default())
        .build();

        Ok(Self {
            inner: Arc::new(resolver),
            grants: Arc::default(),
        })
    }

    pub fn with_grants(mut self, grants: Arc<LocalGrants>) -> Self {
        self.grants = grants;
        self
    }
}

async fn resolve_granted(name: String) -> std::result::Result<Vec<SocketAddr>, String> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), 0))
        .await
        .map_err(|e| format!("DNS resolution of granted host {name} failed: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("granted host {name} resolved to no addresses"));
    }
    if let Some(bad) = addrs.iter().find(|a| is_never_grantable_ip(a.ip())) {
        return Err(format!(
            "granted host {name} resolved to {}, which cannot be granted",
            bad.ip()
        ));
    }
    Ok(addrs)
}

impl Resolve for ValidatingResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let resolver = self.inner.clone();
        if self.grants.permits_host(name.as_str()) {
            let name = name.as_str().to_string();
            return Box::pin(async move {
                let addrs = resolve_granted(name).await?;
                let addrs: Addrs = Box::new(addrs.into_iter());
                Ok(addrs)
            });
        }

        Box::pin(async move {
            let lookup = resolver
                .lookup_ip(name.as_str())
                .await
                .map_err(|e| format!("DNS resolution failed: {}", e))?;

            let addrs: Vec<SocketAddr> = lookup.iter().map(|ip| SocketAddr::new(ip, 0)).collect();

            if addrs.is_empty() {
                return Err("DNS returned no addresses".into());
            }

            for addr in &addrs {
                if is_forbidden_ip(addr.ip()) {
                    return Err(
                        format!("Resolved address {} is in a forbidden range", addr.ip()).into(),
                    );
                }
            }

            let addrs: Addrs = Box::new(addrs.into_iter());
            Ok(addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn ip4(s: &str) -> IpAddr {
        s.parse().unwrap()
    }
    fn ip6(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn grants(entries: &[&str]) -> LocalGrants {
        LocalGrants::parse(&entries.iter().map(|e| e.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn local_grants_accept_lan_hosts_and_refuse_what_cannot_be_granted() {
        for ok in [
            "komga.lan",
            "Komga.LAN:25600",
            "192.168.1.20:25600",
            "10.0.0.5",
            "[fd00::1]:8080",
        ] {
            assert!(
                LocalGrants::parse(&[ok.to_string()]).is_ok(),
                "{ok} should be grantable"
            );
        }
        for bad in [
            "localhost",
            "api.localhost",
            "127.0.0.1",
            "169.254.169.254",
            "[::1]",
            "[fe80::1]",
            "0.0.0.0",
            "http://komga.lan/",
            "komga.lan/path",
            "user@komga.lan",
        ] {
            assert!(
                LocalGrants::parse(&[bad.to_string()]).is_err(),
                "{bad} must be refused"
            );
        }
    }

    #[test]
    fn a_grant_with_a_port_covers_only_that_port() {
        let g = grants(&["komga.lan:25600", "10.0.0.5"]);
        assert!(g.permits_url("http://komga.lan:25600/api"));
        assert!(g.permits_url("http://KOMGA.lan:25600/api"));
        assert!(!g.permits_url("http://komga.lan:8080/api"));
        assert!(
            !g.permits_url("http://komga.lan/api"),
            "port 80 was not granted"
        );
        assert!(
            g.permits_url("http://10.0.0.5:9000/x"),
            "no port means any port"
        );
        assert!(!g.permits_url("http://10.0.0.6/x"));
        assert!(g.permits_host("komga.lan") && !g.permits_host("other.lan"));
    }

    #[tokio::test]
    async fn a_granted_name_resolving_to_loopback_is_still_refused() {
        let err = resolve_granted("localhost".to_string()).await.unwrap_err();
        assert!(err.contains("cannot be granted"), "got: {err}");
    }

    #[test]
    fn loopback_ipv4_is_forbidden() {
        assert!(is_forbidden_ip(ip4("127.0.0.1")));
    }

    #[test]
    fn private_class_a_is_forbidden() {
        assert!(is_forbidden_ip(ip4("10.0.0.1")));
    }

    #[test]
    fn private_class_b_is_forbidden() {
        assert!(is_forbidden_ip(ip4("172.16.0.1")));
    }

    #[test]
    fn private_class_c_is_forbidden() {
        assert!(is_forbidden_ip(ip4("192.168.1.1")));
    }

    #[test]
    fn link_local_is_forbidden() {
        assert!(is_forbidden_ip(ip4("169.254.1.1")));
    }

    #[test]
    fn cgnat_is_forbidden() {
        assert!(is_forbidden_ip(ip4("100.64.0.1")));
    }

    #[test]
    fn reserved_range_is_forbidden() {
        assert!(is_forbidden_ip(ip4("240.0.0.1")));
    }

    #[test]
    fn broadcast_is_forbidden() {
        assert!(is_forbidden_ip(ip4("255.255.255.255")));
    }

    #[test]
    fn ipv6_loopback_is_forbidden() {
        assert!(is_forbidden_ip(ip6("::1")));
    }

    #[test]
    fn ipv4_mapped_private_is_forbidden() {
        assert!(is_forbidden_ip(ip6("::ffff:192.168.1.1")));
    }

    #[test]
    fn google_dns_is_allowed() {
        assert!(!is_forbidden_ip(ip4("8.8.8.8")));
    }

    #[test]
    fn cloudflare_ipv6_is_allowed() {
        assert!(!is_forbidden_ip(ip6("2606:4700::1")));
    }

    #[test]
    fn url_host_literal_loopback_is_forbidden() {
        assert!(is_forbidden_url_host("http://127.0.0.1:9000/hook"));
    }

    #[test]
    fn url_host_literal_cloud_metadata_is_forbidden() {
        assert!(is_forbidden_url_host(
            "http://169.254.169.254/latest/meta-data/"
        ));
    }

    #[test]
    fn url_host_literal_private_ranges_are_forbidden() {
        assert!(is_forbidden_url_host("https://10.0.0.5/x"));
        assert!(is_forbidden_url_host("https://192.168.1.1/x"));
        assert!(is_forbidden_url_host("http://[::1]:8080/x"));
        assert!(is_forbidden_url_host("http://[::ffff:192.168.1.1]/x"));
    }

    #[test]
    fn url_host_public_literal_is_allowed() {
        assert!(!is_forbidden_url_host("https://8.8.8.8/x"));
    }

    #[test]
    fn url_hostname_is_left_to_the_resolver() {
        assert!(!is_forbidden_url_host("https://example.com/hook"));
        assert!(!is_forbidden_url_host("https://localhost/hook"));
    }

    #[test]
    fn unparseable_url_is_not_flagged() {
        assert!(!is_forbidden_url_host("not a url"));
    }
}
