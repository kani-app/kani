//! ComicInfo XML serialization and spread-metadata parsing for CBZ archives.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

fn is_false(b: &bool) -> bool {
    !b
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename = "Page")]
pub struct ComicPage {
    /// 0-based page index (ComicInfo standard: `Image` attribute).
    #[serde(rename = "@Image")]
    pub image: u32,
    /// Omit the attribute entirely when `false` — keeps files small.
    #[serde(rename = "@DoublePage", skip_serializing_if = "is_false")]
    pub double_page: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename = "Pages")]
pub struct ComicPages {
    #[serde(rename = "Page")]
    pub pages: Vec<ComicPage>,
}

impl ComicPages {
    /// Build a `Pages` block from the complete set of double-page indices.
    /// Only emits individual `<Page>` elements that are actually needed — one
    /// per image entry, with `DoublePage="true"` on the flagged ones.
    pub(crate) fn from_flags(total: usize, double_pages: &HashSet<usize>) -> Self {
        let pages = (0..total)
            .map(|i| ComicPage {
                image: i as u32,
                double_page: double_pages.contains(&i),
            })
            .collect();
        ComicPages { pages }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename = "ComicInfo")]
pub struct ComicInfo {
    #[serde(rename = "@xmlns:xsi")]
    pub xmlns_xsi: &'static str,
    #[serde(rename = "Series")]
    pub series: String,
    #[serde(rename = "Title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "Number")]
    pub number: f64,
    #[serde(rename = "Volume", skip_serializing_if = "Option::is_none")]
    pub volume: Option<i64>,
    #[serde(rename = "Summary", skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(rename = "LanguageISO", skip_serializing_if = "Option::is_none")]
    pub language_iso: Option<String>,
    #[serde(rename = "Writer", skip_serializing_if = "Option::is_none")]
    pub writer: Option<String>,
    #[serde(rename = "Penciller", skip_serializing_if = "Option::is_none")]
    pub penciller: Option<String>,
    #[serde(rename = "Genre", skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(rename = "Web", skip_serializing_if = "Option::is_none")]
    pub web: Option<String>,
    /// Per-page spread metadata.  `None` means no spread analysis was run.
    #[serde(rename = "Pages", skip_serializing_if = "Option::is_none")]
    pub pages: Option<ComicPages>,
}

/// Serialise a `ComicInfo` to a UTF-8 XML string with a proper declaration.
pub(crate) fn build_xml(info: &ComicInfo) -> crate::error::Result<String> {
    let mut buf = String::from(r#"<?xml version="1.0" encoding="utf-8"?>"#);
    buf.push('\n');
    let serialized = quick_xml::se::to_string(info).map_err(|e| {
        tracing::error!("Failed to serialize ComicInfo: {}", e);
        crate::error::Error::Internal(format!("Failed to serialize ComicInfo: {}", e))
    })?;
    buf.push_str(&serialized);
    Ok(buf)
}

#[derive(Deserialize, Debug)]
struct ParsedComicPage {
    #[serde(rename = "@Image")]
    image: u32,
    #[serde(rename = "@DoublePage", default)]
    double_page: bool,
}

#[derive(Deserialize, Debug)]
struct ParsedComicPages {
    #[serde(rename = "Page", default)]
    pages: Vec<ParsedComicPage>,
}

#[derive(Deserialize, Debug)]
struct ParsedComicInfo {
    #[serde(rename = "Pages")]
    pages: Option<ParsedComicPages>,
}

/// Returns `true` if the XML string contains a `<Pages>` element with at
/// least one `<Page>` child — i.e., spread metadata has already been written.
pub(crate) fn has_pages_metadata(xml: &str) -> bool {
    match quick_xml::de::from_str::<ParsedComicInfo>(xml) {
        Ok(info) => info.pages.map(|p| !p.pages.is_empty()).unwrap_or(false),
        Err(_) => false,
    }
}

/// Parse `<Pages>` out of a ComicInfo XML string and return the set of
/// 0-based image indices that are flagged as `DoublePage="true"`.
///
/// Returns an empty set on parse failure or when no `<Pages>` block exists.
pub(crate) fn parse_double_pages(xml: &str) -> HashSet<u32> {
    match quick_xml::de::from_str::<ParsedComicInfo>(xml) {
        Ok(info) => info
            .pages
            .map(|p| {
                p.pages
                    .into_iter()
                    .filter(|page| page.double_page)
                    .map(|page| page.image)
                    .collect()
            })
            .unwrap_or_default(),
        Err(_) => HashSet::new(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn minimal() -> ComicInfo {
        ComicInfo {
            xmlns_xsi: "http://www.w3.org/2001/XMLSchema-instance",
            series: "Test Series".to_string(),
            title: None,
            number: 1.0,
            volume: None,
            summary: None,
            language_iso: None,
            writer: None,
            penciller: None,
            genre: None,
            web: None,
            pages: None,
        }
    }

    #[test]
    fn escapes_special_characters() {
        let info = ComicInfo {
            series: "Berserk & Co".to_string(),
            title: Some("<Dark Fantasy>".to_string()),
            summary: Some("A \"great\" story".to_string()),
            language_iso: Some("en".to_string()),
            ..minimal()
        };
        let xml = build_xml(&info).unwrap();
        assert!(xml.contains("Berserk &amp; Co"));
        assert!(xml.contains("&lt;Dark Fantasy&gt;"));
        assert!(xml.contains("&quot;great&quot;") || xml.contains("\"great\""));
    }

    #[test]
    fn xml_declaration_is_present() {
        let xml = build_xml(&minimal()).unwrap();
        assert!(xml.starts_with(r#"<?xml version="1.0" encoding="utf-8"?>"#));
    }

    #[test]
    fn mandatory_fields_appear() {
        let info = ComicInfo {
            series: "My Manga".to_string(),
            number: 7.5,
            ..minimal()
        };
        let xml = build_xml(&info).unwrap();
        assert!(xml.contains("<Series>My Manga</Series>"));
        assert!(xml.contains("<Number>7.5</Number>"));
    }

    #[test]
    fn whole_number_serializes_without_fraction() {
        let info = ComicInfo {
            number: 3.0,
            ..minimal()
        };
        let xml = build_xml(&info).unwrap();
        assert!(xml.contains("<Number>3</Number>") || xml.contains("<Number>3.0</Number>"));
    }

    #[test]
    fn optional_none_fields_are_absent() {
        let xml = build_xml(&minimal()).unwrap();
        assert!(!xml.contains("<Title>"));
        assert!(!xml.contains("<Volume>"));
        assert!(!xml.contains("<Summary>"));
        assert!(!xml.contains("<LanguageISO>"));
        assert!(!xml.contains("<Writer>"));
        assert!(!xml.contains("<Penciller>"));
        assert!(!xml.contains("<Genre>"));
        assert!(!xml.contains("<Web>"));
        assert!(!xml.contains("<Pages>"));
    }

    #[test]
    fn all_optional_fields_appear_when_set() {
        let info = ComicInfo {
            series: "Fullmetal".to_string(),
            title: Some("Brotherhood".to_string()),
            number: 1.0,
            volume: Some(1),
            summary: Some("Alchemy story".to_string()),
            language_iso: Some("ja".to_string()),
            writer: Some("Arakawa Hiromu".to_string()),
            penciller: Some("Arakawa Hiromu".to_string()),
            genre: Some("Action".to_string()),
            web: Some("https://example.com".to_string()),
            ..minimal()
        };
        let xml = build_xml(&info).unwrap();
        assert!(xml.contains("<Title>Brotherhood</Title>"));
        assert!(xml.contains("<Volume>1</Volume>"));
        assert!(xml.contains("<Summary>Alchemy story</Summary>"));
        assert!(xml.contains("<LanguageISO>ja</LanguageISO>"));
        assert!(xml.contains("<Writer>Arakawa Hiromu</Writer>"));
        assert!(xml.contains("<Penciller>Arakawa Hiromu</Penciller>"));
        assert!(xml.contains("<Genre>Action</Genre>"));
        assert!(xml.contains("<Web>https://example.com</Web>"));
    }

    #[test]
    fn pages_block_absent_when_none() {
        let xml = build_xml(&minimal()).unwrap();
        assert!(!xml.contains("<Pages>"));
        assert!(!xml.contains("DoublePage"));
    }

    #[test]
    fn double_page_attribute_omitted_when_false() {
        let flags: HashSet<usize> = HashSet::new();
        let info = ComicInfo {
            pages: Some(ComicPages::from_flags(3, &flags)),
            ..minimal()
        };
        let xml = build_xml(&info).unwrap();
        assert!(xml.contains("<Pages>"));
        assert!(xml.contains(r#"Image="0""#));
        assert!(!xml.contains("DoublePage"));
    }

    #[test]
    fn double_page_attribute_present_for_flagged_pages() {
        let flags: HashSet<usize> = [1usize].into_iter().collect();
        let info = ComicInfo {
            pages: Some(ComicPages::from_flags(4, &flags)),
            ..minimal()
        };
        let xml = build_xml(&info).unwrap();
        assert!(xml.contains(r#"Image="1" DoublePage="true""#));
        assert!(!xml.contains(r#"Image="0" DoublePage"#));
        assert!(!xml.contains(r#"Image="2" DoublePage"#));
        assert!(!xml.contains(r#"Image="3" DoublePage"#));
    }

    #[test]
    fn parse_double_pages_returns_flagged_indices() {
        let flags: HashSet<usize> = [1usize, 3].into_iter().collect();
        let info = ComicInfo {
            pages: Some(ComicPages::from_flags(5, &flags)),
            ..minimal()
        };
        let xml = build_xml(&info).unwrap();
        let body = xml
            .strip_prefix(r#"<?xml version="1.0" encoding="utf-8"?>"#)
            .unwrap_or(&xml)
            .trim();
        let parsed = parse_double_pages(body);
        assert_eq!(parsed, [1u32, 3].into_iter().collect::<HashSet<_>>());
    }

    #[test]
    fn parse_double_pages_empty_on_no_pages_block() {
        let xml = build_xml(&minimal()).unwrap();
        let body = xml
            .strip_prefix(r#"<?xml version="1.0" encoding="utf-8"?>"#)
            .unwrap_or(&xml)
            .trim();
        assert!(parse_double_pages(body).is_empty());
    }

    #[test]
    fn has_pages_metadata_true_when_pages_present() {
        let flags: HashSet<usize> = [0usize].into_iter().collect();
        let info = ComicInfo {
            pages: Some(ComicPages::from_flags(2, &flags)),
            ..minimal()
        };
        let xml = build_xml(&info).unwrap();
        let body = xml
            .strip_prefix(r#"<?xml version="1.0" encoding="utf-8"?>"#)
            .unwrap_or(&xml)
            .trim();
        assert!(has_pages_metadata(body));
    }

    #[test]
    fn has_pages_metadata_false_when_absent() {
        let xml = build_xml(&minimal()).unwrap();
        let body = xml
            .strip_prefix(r#"<?xml version="1.0" encoding="utf-8"?>"#)
            .unwrap_or(&xml)
            .trim();
        assert!(!has_pages_metadata(body));
    }

    #[test]
    fn parse_double_pages_from_external_xml() {
        let xml = r#"<ComicInfo>
  <Series>Test</Series>
  <Pages>
    <Page Image="0" />
    <Page Image="1" DoublePage="true" />
    <Page Image="2" DoublePage="true" />
    <Page Image="3" />
  </Pages>
</ComicInfo>"#;
        let parsed = parse_double_pages(xml);
        assert_eq!(parsed, [1u32, 2].into_iter().collect::<HashSet<_>>());
    }
}
