use crate::error::{Result, ServiceError};
use crate::ids::UserId;
use crate::permissions::Permission;
use crate::service::AppService;

const API_TOKEN_PREFIX: &str = "kani_";
const API_TOKEN_BYTES: usize = 32;

pub struct ApiToken {
    pub id: String,
    pub user_id: UserId,
    pub name: String,
    pub kind: String,
    pub scopes: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    pub expires_at: Option<i64>,
    pub revoked_at: Option<i64>,
}

pub struct CreatedApiToken {
    pub token: ApiToken,
    pub raw_token: String,
}

pub struct ApiTokenAuth {
    pub user_id: UserId,
    pub kind: TokenKind,
    pub scopes: Vec<Permission>,
}

pub(crate) fn generate_raw_token() -> String {
    let bytes: [u8; API_TOKEN_BYTES] = rand::random();
    format!("{API_TOKEN_PREFIX}{}", hex::encode(bytes))
}

pub(crate) fn hash_token(raw: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(raw.as_bytes()))
}

pub(crate) fn parse_scopes(scopes: &str) -> Vec<Permission> {
    scopes
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// Programmatic tokens are scopable; OPDS reader tokens are not. Route
/// acceptance keys on this, never on scope contents, so an OPDS token cannot
/// reach /rest/* even if a broader scope string ends up in its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Opds,
    Api,
}

impl TokenKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Opds => "opds",
            Self::Api => "api",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "opds" => Some(Self::Opds),
            "api" => Some(Self::Api),
            _ => None,
        }
    }
}

/// Fixed scope set for an OPDS reader token. Not user-selectable.
pub(crate) const OPDS_TOKEN_SCOPES: &str = "opds:read opds:progress";

/// Upper bounds so a never-expiring credential is a deliberate choice rather
/// than the default, and one account cannot mint an unbounded number.
pub(crate) const MAX_TOKENS_PER_USER: i64 = 25;
pub(crate) const MAX_TOKEN_LIFETIME_DAYS: u32 = 365;

impl AppService {
    /// The permissions a user currently holds, resolved through role inheritance.
    /// Canonical for both token scope validation and use-time intersection.
    pub async fn user_permissions(&self, user_id: UserId) -> Result<Vec<Permission>> {
        let uid = user_id.0;
        let rows = sqlx::query_scalar!(
            "WITH RECURSIVE role_tree(slug) AS (
                SELECT role_slug FROM user_roles WHERE user_id = ?
                UNION
                SELECT r.parent FROM roles r JOIN role_tree rt ON r.slug = rt.slug
                WHERE r.parent IS NOT NULL
            )
            SELECT DISTINCT rp.permission
            FROM role_permissions rp
            JOIN role_tree rt ON rp.role_slug = rt.slug",
            uid
        )
        .fetch_all(&self.db_read)
        .await?;

        Ok(rows.into_iter().filter_map(|s| s.parse().ok()).collect())
    }

    /// Mints a token of the given kind.
    ///
    /// For `TokenKind::Api`, `scopes` must be a subset of what the creator holds:
    /// a user must never be able to mint a credential more capable than
    /// themselves. This is only half the guarantee — see
    /// `authenticate_token`, which re-intersects at use time.
    pub async fn create_token(
        &self,
        user_id: UserId,
        name: &str,
        expires_in_days: Option<u32>,
        kind: TokenKind,
        scopes: Option<&[Permission]>,
    ) -> Result<CreatedApiToken> {
        if name.trim().is_empty() {
            return Err(ServiceError::Validation("token name is required".into()));
        }
        if let Some(days) = expires_in_days
            && days > MAX_TOKEN_LIFETIME_DAYS
        {
            return Err(ServiceError::Validation(format!(
                "token lifetime cannot exceed {MAX_TOKEN_LIFETIME_DAYS} days"
            )));
        }

        let uid = user_id.0;
        let live: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM api_tokens WHERE user_id = ? AND revoked_at IS NULL",
            uid
        )
        .fetch_one(&self.db_read)
        .await?;
        if live >= MAX_TOKENS_PER_USER {
            return Err(ServiceError::Validation(format!(
                "at most {MAX_TOKENS_PER_USER} live tokens per user; revoke one first"
            )));
        }

        let scope_str = match kind {
            TokenKind::Opds => OPDS_TOKEN_SCOPES.to_string(),
            TokenKind::Api => {
                let requested = scopes.unwrap_or(&[]);
                if requested.is_empty() {
                    return Err(ServiceError::Validation(
                        "an API token needs at least one scope".into(),
                    ));
                }
                let held = self.user_permissions(user_id).await?;
                let over: Vec<String> = requested
                    .iter()
                    .filter(|p| !held.contains(p))
                    .map(|p| p.to_string())
                    .collect();
                if !over.is_empty() {
                    return Err(ServiceError::Validation(format!(
                        "cannot grant permissions you do not hold: {}",
                        over.join(", ")
                    )));
                }
                requested
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        };

        let raw_token = generate_raw_token();
        let hash = hash_token(&raw_token);
        let days = expires_in_days.map(i64::from);
        let kind_str = kind.as_str();

        sqlx::query!(
            r#"
            INSERT INTO api_tokens (user_id, name, token_hash, kind, scopes, expires_at)
            VALUES (?, ?, ?, ?, ?, CASE WHEN ? IS NULL THEN NULL ELSE unixepoch() + ? * 86400 END)
            "#,
            uid,
            name,
            hash,
            kind_str,
            scope_str,
            days,
            days,
        )
        .execute(&self.db)
        .await?;

        let row = sqlx::query!(
            r#"
            SELECT id AS "id!", user_id, name, kind, scopes, created_at,
                   last_used_at, expires_at, revoked_at
            FROM api_tokens WHERE token_hash = ?
            "#,
            hash,
        )
        .fetch_one(&self.db)
        .await?;

        Ok(CreatedApiToken {
            token: ApiToken {
                id: row.id,
                user_id: UserId(row.user_id),
                name: row.name,
                kind: row.kind,
                scopes: row.scopes,
                created_at: row.created_at,
                last_used_at: row.last_used_at,
                expires_at: row.expires_at,
                revoked_at: row.revoked_at,
            },
            raw_token,
        })
    }

    pub async fn list_api_tokens(&self, user_id: UserId) -> Result<Vec<ApiToken>> {
        let uid = user_id.0;
        let rows = sqlx::query!(
            r#"
            SELECT id AS "id!", user_id, name, kind, scopes, created_at,
                   last_used_at, expires_at, revoked_at
            FROM api_tokens
            WHERE user_id = ? AND revoked_at IS NULL
            ORDER BY created_at DESC
            "#,
            uid,
        )
        .fetch_all(&self.db_read)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| ApiToken {
                id: row.id,
                user_id: UserId(row.user_id),
                name: row.name,
                kind: row.kind,
                scopes: row.scopes,
                created_at: row.created_at,
                last_used_at: row.last_used_at,
                expires_at: row.expires_at,
                revoked_at: row.revoked_at,
            })
            .collect())
    }

    pub async fn revoke_api_token(&self, user_id: UserId, token_id: &str) -> Result<()> {
        let uid = user_id.0;
        let affected = sqlx::query!(
            r#"
            UPDATE api_tokens SET revoked_at = unixepoch()
            WHERE id = ? AND user_id = ? AND revoked_at IS NULL
            "#,
            token_id,
            uid,
        )
        .execute(&self.db)
        .await?
        .rows_affected();

        if affected == 0 {
            return Err(ServiceError::NotFound(format!("api token {token_id}")));
        }
        Ok(())
    }

    pub async fn authenticate_api_token(&self, raw_token: &str) -> Result<Option<ApiTokenAuth>> {
        if !raw_token.starts_with(API_TOKEN_PREFIX) {
            return Ok(None);
        }
        let hash = hash_token(raw_token);

        let row = sqlx::query!(
            r#"
            SELECT id AS "id!", user_id, scopes, kind
            FROM api_tokens
            WHERE token_hash = ? AND revoked_at IS NULL
              AND (expires_at IS NULL OR expires_at > unixepoch())
            "#,
            hash,
        )
        .fetch_optional(&self.db_read)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let id = row.id;
        sqlx::query!(
            r#"
            UPDATE api_tokens SET last_used_at = unixepoch()
            WHERE id = ? AND (last_used_at IS NULL OR last_used_at < unixepoch() - 60)
            "#,
            id,
        )
        .execute(&self.db)
        .await?;

        let owner = UserId(row.user_id);
        let kind = TokenKind::parse(&row.kind).unwrap_or(TokenKind::Opds);
        let declared = parse_scopes(&row.scopes);

        // Effective scopes cannot exceed the owner's current permissions after a role change.
        let held = self.user_permissions(owner).await?;
        let scopes: Vec<Permission> = declared.into_iter().filter(|p| held.contains(p)).collect();

        Ok(Some(ApiTokenAuth {
            user_id: owner,
            kind,
            scopes,
        }))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::permissions::Opds;

    #[test]
    fn generate_raw_token_has_prefix_and_length() {
        let token = generate_raw_token();
        assert!(token.starts_with("kani_"));
        assert_eq!(token.len(), 5 + API_TOKEN_BYTES * 2);
    }

    #[test]
    fn generate_raw_token_is_distinct() {
        assert_ne!(generate_raw_token(), generate_raw_token());
    }

    #[test]
    fn hash_token_is_deterministic() {
        assert_eq!(hash_token("kani_abc"), hash_token("kani_abc"));
    }

    #[test]
    fn hash_token_is_distinct_per_input() {
        assert_ne!(hash_token("kani_abc"), hash_token("kani_abd"));
    }

    #[test]
    fn parse_scopes_round_trip() {
        let scopes = parse_scopes("opds:read opds:progress");
        assert_eq!(
            scopes,
            vec![
                Permission::Opds(Opds::Read),
                Permission::Opds(Opds::Progress),
            ]
        );
    }

    #[test]
    fn parse_scopes_ignores_garbage() {
        let scopes = parse_scopes("opds:read not-a-perm opds:progress");
        assert_eq!(
            scopes,
            vec![
                Permission::Opds(Opds::Read),
                Permission::Opds(Opds::Progress),
            ]
        );
    }
}
