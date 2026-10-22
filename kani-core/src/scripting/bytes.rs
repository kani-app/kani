//! Byte-sequence primitives for extension scripts.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rhai::{Array, Engine as RhaiEngine, EvalAltResult};

/// Opaque so a payload larger than `KANI_RHAI_MAX_ARRAY` can cross the boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);

fn err(message: impl Into<String>) -> Box<EvalAltResult> {
    Box::<EvalAltResult>::from(message.into())
}

fn to_byte_vec(values: &Array, label: &str) -> Result<Vec<u8>, Box<EvalAltResult>> {
    values
        .iter()
        .map(|v| {
            let n = v
                .as_int()
                .map_err(|_| err(format!("{label} must contain only integers")))?;
            u8::try_from(n).map_err(|_| err(format!("{label} values must be 0..=255, got {n}")))
        })
        .collect()
}

fn bytes_from_utf8(text: &str) -> Bytes {
    Bytes(text.as_bytes().to_vec())
}

fn bytes_to_utf8(data: Bytes) -> Result<String, Box<EvalAltResult>> {
    String::from_utf8(data.0).map_err(|_| err("bytes are not valid UTF-8"))
}

fn bytes_from_base64url(text: &str) -> Result<Bytes, Box<EvalAltResult>> {
    URL_SAFE_NO_PAD
        .decode(text)
        .map(Bytes)
        .map_err(|e| err(format!("invalid base64url: {e}")))
}

fn bytes_to_base64url(data: Bytes) -> String {
    URL_SAFE_NO_PAD.encode(data.0)
}

fn bytes_len(data: Bytes) -> i64 {
    data.0.len() as i64
}

/// One round of keyed substitution with output feedback:
/// `out[i] = table[data[i] ^ key[i % key.len] ^ prev]`, where `prev` is the
/// previous output byte and starts at `seed`. `inverse` runs the same round
/// backwards, which requires `table` to be a permutation of 0..=255.
fn bytes_substitute(
    data: Bytes,
    table: Array,
    key: Array,
    seed: i64,
    inverse: bool,
) -> Result<Bytes, Box<EvalAltResult>> {
    let table = to_byte_vec(&table, "table")?;
    let key = to_byte_vec(&key, "key")?;

    if table.len() != 256 {
        return Err(err(format!(
            "table must have exactly 256 entries, got {}",
            table.len()
        )));
    }
    if key.is_empty() {
        return Err(err("key must not be empty"));
    }
    let seed = u8::try_from(seed).map_err(|_| err(format!("seed must be 0..=255, got {seed}")))?;

    let mut prev = seed;
    let mut out = Vec::with_capacity(data.0.len());

    if inverse {
        let mut lookup = [None; 256];
        for (index, &mapped) in table.iter().enumerate() {
            if lookup[mapped as usize].is_some() {
                return Err(err("table must be a permutation of 0..=255 to be inverted"));
            }
            lookup[mapped as usize] = Some(index as u8);
        }
        for &value in &data.0 {
            let Some(index) = lookup[value as usize] else {
                return Err(err("table must be a permutation of 0..=255 to be inverted"));
            };
            out.push(index ^ key[out.len() % key.len()] ^ prev);
            prev = value;
        }
    } else {
        for &value in &data.0 {
            let substituted = table[(value ^ key[out.len() % key.len()] ^ prev) as usize];
            out.push(substituted);
            prev = substituted;
        }
    }

    Ok(Bytes(out))
}

/// Same as the array form, with `table` and `key` supplied as [`Bytes`] — the
/// shape a script gets back from `bytes_from_base64url`, so harvested material
/// needs no JSON parser to reach this call.
fn bytes_substitute_raw(
    data: Bytes,
    table: Bytes,
    key: Bytes,
    seed: i64,
    inverse: bool,
) -> Result<Bytes, Box<EvalAltResult>> {
    let to_array = |b: &Bytes| -> Array { b.0.iter().map(|v| (*v as i64).into()).collect() };
    bytes_substitute(data, to_array(&table), to_array(&key), seed, inverse)
}

pub(crate) fn register_byte_bindings(engine: &mut RhaiEngine) {
    engine
        .register_type_with_name::<Bytes>("Bytes")
        .register_fn("bytes_from_utf8", bytes_from_utf8)
        .register_fn("bytes_to_utf8", bytes_to_utf8)
        .register_fn("bytes_from_base64url", bytes_from_base64url)
        .register_fn("bytes_to_base64url", bytes_to_base64url)
        .register_fn("bytes_len", bytes_len)
        .register_fn("bytes_substitute", bytes_substitute)
        .register_fn("bytes_substitute", bytes_substitute_raw);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn permutation_table() -> Array {
        (0..256)
            .map(|i: i64| ((i & 0xf0) | (15 - (i & 0x0f))).into())
            .collect()
    }

    fn key_array() -> Array {
        vec![7i64.into(), 200i64.into(), 33i64.into()]
    }

    fn identity_table() -> Array {
        (0..256).map(|i: i64| i.into()).collect()
    }

    #[test]
    fn the_bytes_form_matches_the_array_form() {
        let data = Bytes(b"payload".to_vec());
        let table_bytes = Bytes(
            (0..256)
                .map(|i: u32| ((i & 0xf0) | (15 - (i & 0x0f))) as u8)
                .collect(),
        );
        let key_bytes = Bytes(vec![7, 200, 33]);

        let via_array =
            bytes_substitute(data.clone(), permutation_table(), key_array(), 189, false).unwrap();
        let via_bytes = bytes_substitute_raw(data, table_bytes, key_bytes, 189, false).unwrap();

        assert_eq!(via_array, via_bytes);
    }

    #[test]
    fn substitute_then_invert_returns_the_input() {
        let data = Bytes(b"the quick brown fox jumps over the lazy dog".to_vec());
        let key: Array = vec![7i64.into(), 200i64.into(), 33i64.into()];

        let encoded =
            bytes_substitute(data.clone(), permutation_table(), key.clone(), 189, false).unwrap();
        assert_ne!(encoded, data, "substitution left the input unchanged");

        let decoded = bytes_substitute(encoded, permutation_table(), key, 189, true).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn an_identity_table_with_a_zero_key_chains_the_previous_output() {
        let key: Array = vec![0i64.into()];
        let out = bytes_substitute(Bytes(vec![1, 2, 4]), identity_table(), key, 0, false).unwrap();
        assert_eq!(out, Bytes(vec![1, 3, 7]));
    }

    #[test]
    fn the_seed_feeds_the_first_byte() {
        let key: Array = vec![0i64.into()];
        let out = bytes_substitute(Bytes(vec![0]), identity_table(), key, 189, false).unwrap();
        assert_eq!(out, Bytes(vec![189]));
    }

    #[test]
    fn a_non_permutation_table_is_refused_for_inversion() {
        let flat: Array = (0..256).map(|_| 0i64.into()).collect();
        let key: Array = vec![1i64.into()];
        let error = bytes_substitute(Bytes(vec![0, 1]), flat, key, 0, true).unwrap_err();
        assert!(
            error.to_string().contains("permutation"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn a_short_table_is_refused() {
        let key: Array = vec![1i64.into()];
        let error = bytes_substitute(Bytes(vec![0]), vec![1i64.into()], key, 0, false).unwrap_err();
        assert!(
            error.to_string().contains("256"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn an_empty_key_is_refused() {
        let error =
            bytes_substitute(Bytes(vec![0]), identity_table(), Array::new(), 0, false).unwrap_err();
        assert!(
            error.to_string().contains("key"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn an_out_of_range_table_value_is_refused() {
        let mut table = identity_table();
        table[0] = 300i64.into();
        let key: Array = vec![1i64.into()];
        let error = bytes_substitute(Bytes(vec![0]), table, key, 0, false).unwrap_err();
        assert!(
            error.to_string().contains("0..=255"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn base64url_round_trips_through_bytes() {
        let encoded = bytes_to_base64url(Bytes(vec![251, 239, 190, 0, 1]));
        assert_eq!(
            bytes_from_base64url(&encoded).unwrap().0,
            vec![251, 239, 190, 0, 1]
        );
    }

    #[test]
    fn invalid_base64url_is_refused() {
        assert!(bytes_from_base64url("not base64!!").is_err());
    }

    #[test]
    fn utf8_round_trips_and_invalid_bytes_are_refused() {
        assert_eq!(bytes_to_utf8(bytes_from_utf8("héllo")).unwrap(), "héllo");
        assert!(bytes_to_utf8(Bytes(vec![0xff, 0xfe])).is_err());
    }

    #[test]
    fn a_script_can_round_trip_a_payload_through_the_engine() {
        let mut engine = crate::scripting::engine::make_pure_sandbox();
        register_byte_bindings(&mut engine);

        let result: String = engine
            .eval(
                r#"
                let table = [];
                for i in 0..256 { table.push((i & 0xf0) | (15 - (i & 0x0f))); }
                let key = [7, 200, 33];
                let sealed = bytes_substitute(bytes_from_utf8("payload"), table, key, 189, false);
                bytes_to_utf8(bytes_substitute(sealed, table, key, 189, true))
                "#,
            )
            .unwrap();

        assert_eq!(result, "payload");
    }
}
