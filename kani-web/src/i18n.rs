//! Server-side locale negotiation. Parses an `Accept-Language` header against
//! the set of locales the frontend actually ships (`static/locales/`) and picks
//! the best match, falling back to English.

const DEFAULT_LOCALE: &str = "en";

/// Locales with a catalog under `static/locales/`. Grows as translations land;
/// until a second one does, negotiation can only ever resolve to `en`.
const AVAILABLE_LOCALES: &[&str] = &["en"];

/// Resolve the best locale for a request from its `Accept-Language` header.
pub fn resolve_locale(accept_language: &str) -> &'static str {
    negotiate(accept_language, AVAILABLE_LOCALES, DEFAULT_LOCALE)
}

/// Negotiate against an explicit locale set — the testable core of
/// [`resolve_locale`]. Honours q-values (higher wins; `q=0` rejects a tag),
/// matches a requested tag against an available locale by exact equality or a
/// shared primary language subtag (`en-US` matches `en`), and treats `*` as the
/// default. Returns `default` when nothing acceptable is offered.
fn negotiate(
    accept_language: &str,
    available: &[&'static str],
    default: &'static str,
) -> &'static str {
    let mut ranked: Vec<(f32, usize, &str)> = Vec::new();
    for (index, part) in accept_language.split(',').enumerate() {
        let mut fields = part.split(';');
        let tag = match fields.next() {
            Some(t) => t.trim(),
            None => continue,
        };
        if tag.is_empty() {
            continue;
        }

        let mut q = 1.0_f32;
        for field in fields {
            let field = field.trim();
            if let Some(value) = field.strip_prefix("q=") {
                q = value.trim().parse::<f32>().unwrap_or(0.0);
            }
        }
        if q <= 0.0 {
            continue;
        }
        ranked.push((q, index, tag));
    }

    ranked.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });

    for (_, _, tag) in ranked {
        if tag == "*" {
            return default;
        }
        if let Some(hit) = available.iter().find(|loc| tags_match(tag, loc)) {
            return hit;
        }
    }

    default
}

/// A requested tag matches an available locale when they are equal or share a
/// primary language subtag, both compared case-insensitively.
fn tags_match(requested: &str, available: &str) -> bool {
    let req = requested.trim();
    if req.eq_ignore_ascii_case(available) {
        return true;
    }
    let primary = |t: &str| t.split('-').next().unwrap_or(t).to_ascii_lowercase();
    primary(req) == primary(available)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const LOCALES: &[&str] = &["en", "fr", "ja"];

    #[test]
    fn picks_the_highest_q_available_locale() {
        assert_eq!(
            negotiate("de, fr;q=0.9, en;q=0.8", LOCALES, "en"),
            "fr",
            "fr outranks en and de is unavailable"
        );
    }

    #[test]
    fn region_subtag_matches_the_primary_locale() {
        assert_eq!(negotiate("en-US,en;q=0.9", LOCALES, "en"), "en");
        assert_eq!(negotiate("fr-CA", LOCALES, "en"), "fr");
    }

    #[test]
    fn wildcard_yields_the_default() {
        assert_eq!(negotiate("*", LOCALES, "en"), "en");
        assert_eq!(negotiate("de, *;q=0.5", LOCALES, "en"), "en");
    }

    #[test]
    fn q_zero_rejects_a_tag() {
        assert_eq!(
            negotiate("fr;q=0, en", LOCALES, "en"),
            "en",
            "fr is explicitly not acceptable"
        );
    }

    #[test]
    fn no_available_match_falls_back_to_default() {
        assert_eq!(negotiate("de, es;q=0.5", LOCALES, "en"), "en");
    }

    #[test]
    fn empty_or_malformed_header_falls_back() {
        assert_eq!(negotiate("", LOCALES, "en"), "en");
        assert_eq!(negotiate("   ", LOCALES, "en"), "en");
        assert_eq!(negotiate(";;;", LOCALES, "en"), "en");
        assert_eq!(negotiate("en;q=notanumber", LOCALES, "en"), "en");
    }

    #[test]
    fn malformed_q_value_drops_the_tag_but_keeps_others() {
        assert_eq!(
            negotiate("fr;q=oops, ja;q=0.5", LOCALES, "en"),
            "ja",
            "fr's q fails to parse (treated as 0) so ja wins"
        );
    }

    #[test]
    fn resolve_locale_only_offers_shipped_locales() {
        assert_eq!(resolve_locale("fr, en;q=0.5"), "en");
        assert_eq!(resolve_locale("en-GB"), "en");
        assert_eq!(resolve_locale(""), "en");
    }
}
