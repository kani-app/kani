use std::io::{Cursor, Write};

use zip::{ZipWriter, write::SimpleFileOptions};

use crate::service::AppService;

const REDACTED: &str = "***REDACTED***";
const SECRET_MARKERS: [&str; 5] = ["secret", "token", "password", "key", "dsn"];

pub(crate) fn is_secret_field(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SECRET_MARKERS.iter().any(|m| lower.contains(m))
}

pub(crate) fn redact(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| {
                    if is_secret_field(k) && v.is_string() {
                        (k.clone(), serde_json::Value::String(REDACTED.to_string()))
                    } else {
                        (k.clone(), redact(v))
                    }
                })
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(redact).collect())
        }
        other => other.clone(),
    }
}

pub(crate) fn bundle_filename(now: time::OffsetDateTime) -> String {
    format!(
        "kani-support-{:04}{:02}{:02}-{:02}{:02}{:02}.zip",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

impl AppService {
    pub async fn generate_support_bundle(
        &self,
        logs_jsonl: Vec<u8>,
    ) -> crate::error::Result<(Vec<u8>, String)> {
        let diagnostics = self.get_diagnostics().await?;

        let settings_json =
            serde_json::to_value(self.get_settings().await).unwrap_or(serde_json::Value::Null);
        let config = redact(&settings_json);

        let schema_rows = sqlx::query_scalar!(
            "SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name"
        )
        .fetch_all(&self.db_read)
        .await?;
        let db_schema = schema_rows
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(";\n\n");

        // `_sqlx_migrations` is sqlx's own bookkeeping table, so it is queried through the
        // runtime API rather than the checked macro, matching `migration_checksums`.
        let db_schema_version: Option<i64> = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(version) FROM _sqlx_migrations WHERE success = TRUE",
        )
        .fetch_one(&self.db_read)
        .await
        .ok()
        .flatten();

        let kani_info = serde_json::json!({
            "version": diagnostics.version,
            "git_sha": diagnostics.git_sha,
            "uptime_secs": diagnostics.uptime_secs,
            "db_schema_version": db_schema_version,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        });
        let extensions = serde_json::to_value(&diagnostics.extensions)
            .unwrap_or(serde_json::Value::Array(Vec::new()));
        let diagnostics_json =
            serde_json::to_value(&diagnostics).unwrap_or(serde_json::Value::Null);

        let bytes = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
            let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
            let opts =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

            let mut write = |name: &str, data: &[u8]| -> std::io::Result<()> {
                zip.start_file(name, opts)?;
                zip.write_all(data)
            };

            write(
                "kani_info.json",
                serde_json::to_string_pretty(&kani_info)
                    .unwrap_or_default()
                    .as_bytes(),
            )?;
            write(
                "config.json",
                serde_json::to_string_pretty(&config)
                    .unwrap_or_default()
                    .as_bytes(),
            )?;
            write("db_schema.sql", db_schema.as_bytes())?;
            write(
                "extensions.json",
                serde_json::to_string_pretty(&extensions)
                    .unwrap_or_default()
                    .as_bytes(),
            )?;
            write(
                "diagnostics.json",
                serde_json::to_string_pretty(&diagnostics_json)
                    .unwrap_or_default()
                    .as_bytes(),
            )?;
            write("logs.jsonl", &logs_jsonl)?;

            Ok(zip.finish()?.into_inner())
        })
        .await
        .map_err(|e| crate::error::ServiceError::Internal(e.to_string()))?
        .map_err(|e| crate::error::ServiceError::Internal(e.to_string()))?;

        Ok((bytes, bundle_filename(time::OffsetDateTime::now_utc())))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn secret_field_names_are_detected_case_insensitively() {
        for name in [
            "KANI_SECRET_KEY",
            "api_token",
            "Password",
            "glitchtip_dsn",
            "encryption_key",
        ] {
            assert!(is_secret_field(name), "{name} should be treated as secret");
        }
        assert!(!is_secret_field("library_path"));
        assert!(!is_secret_field("scan_interval_minutes"));
    }

    #[test]
    fn redact_replaces_secret_values_at_every_depth() {
        let input = serde_json::json!({
            "library_path": "/library",
            "email_password": "hunter2",
            "nested": { "client_secret": "abc", "keep": 1 },
            "list": [{ "auth_token": "xyz" }]
        });

        let out = redact(&input);

        assert_eq!(out["library_path"], "/library");
        assert_eq!(out["email_password"], REDACTED);
        assert_eq!(out["nested"]["client_secret"], REDACTED);
        assert_eq!(out["nested"]["keep"], 1);
        assert_eq!(out["list"][0]["auth_token"], REDACTED);

        let serialised = out.to_string();
        assert!(!serialised.contains("hunter2"));
        assert!(!serialised.contains("xyz"));
    }

    #[test]
    fn non_string_values_are_never_redacted() {
        let input = serde_json::json!({
            "password_reset_enabled": false,
            "max_login_attempts": 5,
            "email_password": "hunter2",
        });

        let out = redact(&input);

        assert_eq!(
            out["password_reset_enabled"], false,
            "a boolean feature flag whose name merely contains 'password' is not a credential"
        );
        assert_eq!(out["max_login_attempts"], 5);
        assert_eq!(out["email_password"], REDACTED);
    }

    #[test]
    fn bundle_filename_is_timestamped_and_zip_suffixed() {
        let ts = time::OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let name = bundle_filename(ts);
        assert!(name.starts_with("kani-support-"), "got {name}");
        assert!(name.ends_with(".zip"), "got {name}");
        assert_eq!(name.len(), "kani-support-YYYYMMDD-HHMMSS.zip".len());
    }
}
