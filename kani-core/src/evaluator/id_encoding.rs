pub use kani_shared::encoding::{decode_composite, encode_composite};

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use kani_shared::ast::IdEncoding;

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
}
