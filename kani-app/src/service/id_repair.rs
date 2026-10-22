//! Rewrites composite source ids that were stored decoded.
//!
//! An extension's `id_encoding` block packs several sub-values into the single
//! id the host stores. The stored value is fed back to `decode_composite` when
//! a request is built, so it has to be the encoded form; a decoded one yields
//! no sub-fields and the request silently loses its `$manga.hid$` placeholders.

use sqlx::SqlitePool;

use kani_shared::ast::IdEncoding;
use kani_shared::encoding::{decode_composite, encode_composite};
use kani_yaml::yaml::schema::{IdEncodingEntry, YamlIdEncoding};

use crate::source::SourceBackend;
use crate::source::registry::SourceRegistry;

/// Row counts from a repair pass.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RepairReport {
    pub rewritten: u64,
    pub unfixable: u64,
}

impl RepairReport {
    fn add(&mut self, other: Self) {
        self.rewritten += other.rewritten;
        self.unfixable += other.unfixable;
    }

    fn touched(self) -> bool {
        self.rewritten > 0 || self.unfixable > 0
    }
}

/// What a stored id needs.
#[derive(Debug, PartialEq, Eq)]
enum Decision {
    /// Already in the declared encoding.
    Leave,
    /// The decoded join; re-encode it to this.
    Rewrite(String),
    /// Neither decodable nor splittable into the declared arity.
    Unfixable,
}

fn shared_encoding(encoding: YamlIdEncoding) -> IdEncoding {
    match encoding {
        YamlIdEncoding::Base64Url => IdEncoding::Base64Url,
        YamlIdEncoding::Base64 => IdEncoding::Base64,
        YamlIdEncoding::Passthrough => IdEncoding::Passthrough,
        YamlIdEncoding::Hex => IdEncoding::Hex,
    }
}

/// Classifies one stored id against the encoding its extension declares.
///
/// Decoding is the test for the encoded form, so a value that already decodes
/// is left alone and a second pass over rewritten rows is a no-op. A value that
/// is genuinely opaque but happens to decode is left alone too, which is the
/// safe direction: this never rewrites twice.
fn decide(stored: &str, entry: &IdEncodingEntry) -> Decision {
    let encoding = shared_encoding(entry.encoding);
    let names: Vec<&str> = entry.fields.iter().map(String::as_str).collect();
    if decode_composite(stored, &entry.delimiter, &encoding, &names).is_ok() {
        return Decision::Leave;
    }
    let parts: Vec<&str> = if names.len() <= 1 {
        vec![stored]
    } else {
        stored
            .splitn(names.len(), entry.delimiter.as_str())
            .collect()
    };
    if parts.len() != names.len() {
        return Decision::Unfixable;
    }
    encode_composite(&parts, &entry.delimiter, &encoding)
        .map_or(Decision::Unfixable, Decision::Rewrite)
}

async fn repair_rows(
    pool: &SqlitePool,
    entry: &IdEncodingEntry,
    select_sql: &str,
    update_sql: &str,
    source_id: i64,
) -> RepairReport {
    let mut report = RepairReport::default();
    let rows: Vec<(i64, String)> = match sqlx::query_as(select_sql)
        .bind(source_id)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(source_id, %e, "id repair could not read rows");
            return report;
        }
    };

    for (row_id, stored) in rows {
        match decide(&stored, entry) {
            Decision::Leave => {}
            Decision::Unfixable => report.unfixable += 1,
            Decision::Rewrite(encoded) => {
                match sqlx::query(update_sql)
                    .bind(&encoded)
                    .bind(row_id)
                    .execute(pool)
                    .await
                {
                    Ok(_) => report.rewritten += 1,
                    Err(e) => {
                        tracing::warn!(source_id, row_id, %e, "id repair could not write a row");
                        report.unfixable += 1;
                    }
                }
            }
        }
    }
    report
}

/// Repairs every registered source that declares an `id_encoding`.
///
/// Idempotent, so it is safe to run on each boot: a rewritten row decodes on
/// the next pass and is left alone. Sources without composite ids are skipped
/// without reading a single row.
pub async fn repair_composite_ids(pool: &SqlitePool, registry: &SourceRegistry) -> RepairReport {
    let mut total = RepairReport::default();

    for (source_id, backend) in registry.entries() {
        let SourceBackend::Yaml(yaml) = backend.as_ref() else {
            continue;
        };
        let Some(block) = yaml.config.id_encoding.as_ref() else {
            continue;
        };

        let mut source_report = RepairReport::default();
        if let Some(entry) = block.manga.as_ref() {
            source_report.add(
                repair_rows(
                    pool,
                    entry,
                    "SELECT id, source_manga_id FROM manga WHERE source_id = ?",
                    "UPDATE manga SET source_manga_id = ? WHERE id = ?",
                    source_id,
                )
                .await,
            );
        }
        if let Some(entry) = block.chapter.as_ref() {
            source_report.add(
                repair_rows(
                    pool,
                    entry,
                    "SELECT c.id, c.source_chapter_id FROM chapters c \
                     JOIN manga m ON c.manga_id = m.id WHERE m.source_id = ?",
                    "UPDATE chapters SET source_chapter_id = ? WHERE id = ?",
                    source_id,
                )
                .await,
            );
        }

        if source_report.touched() {
            tracing::info!(
                source_id,
                rewritten = source_report.rewritten,
                unfixable = source_report.unfixable,
                "repaired composite source ids"
            );
        }
        total.add(source_report);
    }

    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(fields: &[&str], delimiter: &str, encoding: YamlIdEncoding) -> IdEncodingEntry {
        IdEncodingEntry {
            fields: fields.iter().map(|f| (*f).to_string()).collect(),
            delimiter: delimiter.to_string(),
            encoding,
        }
    }

    fn composite_source() -> IdEncodingEntry {
        entry(&["hid", "slug"], "|", YamlIdEncoding::Base64Url)
    }

    #[test]
    fn decoded_join_is_re_encoded() {
        let decoded = "ab12|a-generic-title";
        let Decision::Rewrite(encoded) = decide(decoded, &composite_source()) else {
            panic!("a decoded join should be rewritten");
        };
        let fields = decode_composite(&encoded, "|", &IdEncoding::Base64Url, &["hid", "slug"])
            .expect("the rewritten id decodes");
        assert_eq!(fields[0], ("hid".to_string(), "ab12".to_string()));
        assert_eq!(
            fields[1],
            ("slug".to_string(), "a-generic-title".to_string())
        );
    }

    #[test]
    fn a_second_pass_leaves_a_repaired_id_alone() {
        let Decision::Rewrite(encoded) = decide("ab12|a-generic-title", &composite_source()) else {
            panic!("first pass should rewrite");
        };
        assert_eq!(decide(&encoded, &composite_source()), Decision::Leave);
    }

    #[test]
    fn a_value_of_the_wrong_arity_is_left_for_a_human() {
        assert_eq!(
            decide("no-delimiter-here", &composite_source()),
            Decision::Unfixable
        );
    }

    #[test]
    fn passthrough_ids_are_never_touched() {
        let e = entry(&["slug"], "|", YamlIdEncoding::Passthrough);
        assert_eq!(decide("plain-slug", &e), Decision::Leave);
    }
}
