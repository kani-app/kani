//! Typed identifier newtypes.
//!
//! These wrap the raw `i64`/`String` IDs used throughout the schema so the
//! compiler can distinguish a `MangaId` from a `ChapterId`. They are
//! `#[serde(transparent)]` and `#[sqlx(transparent)]`, so JSON bodies and SQL
//! binds/decodes are byte-identical to the underlying primitive — adoption is
//! incremental and call sites may convert via `From`/`Into` during migration.

/// Defines an `i64`-backed transparent ID newtype with the standard conversions.
macro_rules! int_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash,
            serde::Serialize, serde::Deserialize, sqlx::Type,
        )]
        #[serde(transparent)]
        #[sqlx(transparent)]
        pub struct $name(pub i64);

        impl From<i64> for $name {
            fn from(v: i64) -> Self {
                Self(v)
            }
        }
        impl From<$name> for i64 {
            fn from(v: $name) -> i64 {
                v.0
            }
        }
        impl std::str::FromStr for $name {
            type Err = std::num::ParseIntError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse::<i64>().map(Self)
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

int_id!(MangaId);
int_id!(ChapterId);
int_id!(UserId);

/// A source/extension identifier. `String`-backed to match the WIT interface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct SourceId(pub String);

impl From<String> for SourceId {
    fn from(s: String) -> Self {
        Self(s)
    }
}
impl From<&str> for SourceId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}
impl std::fmt::Display for SourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn manga_id_i64_roundtrip() {
        let id = MangaId::from(42);
        assert_eq!(i64::from(id), 42);
        assert_eq!(id, MangaId(42));
    }

    #[test]
    fn manga_id_parse_roundtrip() {
        let id: MangaId = "42".parse().unwrap();
        assert_eq!(id.0, 42);
        assert_eq!(id.to_string(), "42");
    }

    #[test]
    fn manga_id_parse_rejects_non_integer() {
        assert!("abc".parse::<MangaId>().is_err());
    }

    #[test]
    fn chapter_id_serde_is_transparent() {
        let id = ChapterId(5);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "5");
        let back: ChapterId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn source_id_conversions() {
        let from_string = SourceId::from("example-source".to_string());
        let from_str = SourceId::from("example-source");
        assert_eq!(from_string, from_str);
        assert_eq!(from_string.to_string(), "example-source");
    }

    #[test]
    fn source_id_serde_is_transparent() {
        let id = SourceId("example-source".into());
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"example-source\"");
        let back: SourceId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[tokio::test]
    async fn id_newtypes_sqlx_roundtrip() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY, src TEXT)")
            .execute(&pool)
            .await
            .unwrap();

        let id = MangaId(99);
        let src = SourceId("s1".into());
        sqlx::query("INSERT INTO t (id, src) VALUES (?, ?)")
            .bind(id)
            .bind(&src)
            .execute(&pool)
            .await
            .unwrap();

        let (got_id, got_src): (MangaId, SourceId) = sqlx::query_as("SELECT id, src FROM t")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(got_id, id);
        assert_eq!(got_src, src);
    }
}
