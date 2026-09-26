//! Manga identifier encoding shared by declarative codegen and runtime decoding.

use crate::ast::IdEncoding;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

pub fn encode_manga_id(raw: &str) -> String {
    URL_SAFE_NO_PAD.encode(raw.as_bytes())
}

pub fn decode_manga_id(encoded: &str) -> String {
    URL_SAFE_NO_PAD
        .decode(encoded)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_else(|| encoded.to_string())
}

/// Joins `parts` with `delimiter` and encodes the result.
///
/// Decoding splits at most `parts.len() - 1` times, so only the last part may contain
/// the delimiter; any other part containing it is rejected rather than misparsed later.
pub fn encode_composite(
    parts: &[&str],
    delimiter: &str,
    encoding: &IdEncoding,
) -> Result<String, String> {
    if let Some((index, part)) = parts
        .iter()
        .enumerate()
        .take(parts.len().saturating_sub(1))
        .find(|(_, part)| part.contains(delimiter))
    {
        return Err(format!(
            "composite id part {index} ({part:?}) contains the delimiter {delimiter:?}; \
             only the last part may"
        ));
    }
    let joined = parts.join(delimiter);
    Ok(match encoding {
        IdEncoding::Passthrough => joined,
        IdEncoding::Base64Url => {
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(joined.as_bytes())
        }
        IdEncoding::Base64 => base64::engine::general_purpose::STANDARD.encode(joined.as_bytes()),
        IdEncoding::Hex => hex::encode(joined.as_bytes()),
    })
}

pub fn decode_composite(
    id: &str,
    delimiter: &str,
    encoding: &IdEncoding,
    field_names: &[&str],
) -> Result<Vec<(String, String)>, String> {
    let joined = match encoding {
        IdEncoding::Passthrough => id.to_owned(),
        IdEncoding::Base64Url => {
            let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(id)
                .map_err(|e| format!("base64url decode failed: {e}"))?;
            String::from_utf8(bytes).map_err(|e| format!("base64url utf8 error: {e}"))?
        }
        IdEncoding::Base64 => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(id)
                .map_err(|e| format!("base64 decode failed: {e}"))?;
            String::from_utf8(bytes).map_err(|e| format!("base64 utf8 error: {e}"))?
        }
        IdEncoding::Hex => {
            let bytes = hex::decode(id).map_err(|e| format!("hex decode failed: {e}"))?;
            String::from_utf8(bytes).map_err(|e| format!("hex utf8 error: {e}"))?
        }
    };

    if field_names.is_empty() {
        return Ok(vec![]);
    }

    let max_splits = field_names.len();
    let parts: Vec<&str> = if max_splits == 1 {
        vec![joined.as_str()]
    } else {
        joined.splitn(max_splits, delimiter).collect()
    };

    if parts.len() != field_names.len() {
        return Err(format!(
            "expected {} fields separated by {:?}, got {}",
            field_names.len(),
            delimiter,
            parts.len()
        ));
    }

    Ok(field_names
        .iter()
        .zip(parts.iter())
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn round_trip(parts: &[&str], delimiter: &str, encoding: IdEncoding) {
        let field_names: Vec<&str> = (0..parts.len()).map(|i| ["a", "b", "c"][i]).collect();
        let encoded = encode_composite(parts, delimiter, &encoding).unwrap();
        let decoded = decode_composite(&encoded, delimiter, &encoding, &field_names).unwrap();
        let values: Vec<&str> = decoded.iter().map(|(_, v)| v.as_str()).collect();
        assert_eq!(values, parts);
    }

    #[test]
    fn passthrough_round_trip() {
        round_trip(&["manga_123", "ch_456"], "|", IdEncoding::Passthrough);
    }

    #[test]
    fn base64url_round_trip() {
        round_trip(&["manga/123", "ch&456"], "|", IdEncoding::Base64Url);
    }

    #[test]
    fn base64_round_trip() {
        round_trip(&["hello", "world"], ":", IdEncoding::Base64);
    }

    #[test]
    fn hex_round_trip() {
        round_trip(&["foo", "bar"], "-", IdEncoding::Hex);
    }

    #[test]
    fn single_field_no_delimiter() {
        let enc = encode_composite(&["solo"], "|", &IdEncoding::Base64Url).unwrap();
        let dec = decode_composite(&enc, "|", &IdEncoding::Base64Url, &["id"]).unwrap();
        assert_eq!(dec[0].1, "solo");
    }

    #[test]
    fn field_with_underscores() {
        round_trip(&["created_at_desc", "en"], "|", IdEncoding::Base64Url);
    }

    #[test]
    fn mismatched_field_count_errors() {
        let enc = encode_composite(&["a", "b"], "|", &IdEncoding::Passthrough).unwrap();
        let result = decode_composite(&enc, "|", &IdEncoding::Passthrough, &["x", "y", "z"]);
        assert!(result.is_err());
    }

    #[test]
    fn delimiter_in_last_part_round_trips() {
        round_trip(&["h1", "title|with|bars"], "|", IdEncoding::Base64Url);
    }

    #[test]
    fn delimiter_in_non_final_part_is_rejected() {
        let err = encode_composite(&["a|b", "slug"], "|", &IdEncoding::Base64Url).unwrap_err();
        assert!(
            err.contains("part 0") && err.contains("\"a|b\""),
            "error must name the offending part: {err}"
        );
    }
}

#[cfg(test)]
mod manga_id_tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

    fn encode(s: &str) -> String {
        URL_SAFE_NO_PAD.encode(s.as_bytes())
    }

    #[test]
    fn roundtrip_encode_decode() {
        let original = "https://example.com/manga/12345";
        assert_eq!(decode_manga_id(&encode(original)), original);
    }

    #[test]
    fn plain_id_falls_back_to_original() {
        assert_eq!(decode_manga_id("my-manga-slug"), "my-manga-slug");
    }

    #[test]
    fn empty_string_returns_empty() {
        assert_eq!(decode_manga_id(""), "");
    }

    #[test]
    fn invalid_base64_returns_original() {
        let input = "not-base64!!!";
        assert_eq!(decode_manga_id(input), input);
    }

    #[test]
    fn valid_base64_non_utf8_falls_back() {
        let bad_utf8 = URL_SAFE_NO_PAD.encode([0xFF, 0xFE]);
        assert_eq!(decode_manga_id(&bad_utf8), bad_utf8);
    }

    #[test]
    fn unicode_manga_id_roundtrips() {
        let original = "manga/進撃の巨人/ch1";
        assert_eq!(decode_manga_id(&encode(original)), original);
    }
}
