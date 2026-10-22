//! Pluggable metadata enrichment providers and their application registry.

use crate::error::{Result, ServiceError};
use crate::ids::{MangaId, UserId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Partial metadata returned by a provider; absent scalar fields leave local values unchanged.
pub struct FullMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub authors: Vec<String>,
    pub artists: Vec<String>,
    pub tags: Vec<String>,
    pub external_ids: HashMap<String, String>,
}

#[async_trait::async_trait]
/// External metadata lookup implemented by each registered provider.
/// `Ok(None)` means the provider found no match rather than failing.
pub trait MetadataProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    async fn fetch(&self, title: &str) -> Result<Option<FullMetadata>>;
}

/// Runtime registry keyed by stable provider identifier.
pub struct MetadataProviderRegistry {
    pub providers: HashMap<String, Box<dyn MetadataProvider>>,
}

impl MetadataProviderRegistry {
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut providers: HashMap<String, Box<dyn MetadataProvider>> = HashMap::new();
        // The stub fabricates a description and enrichment writes it to the manga
        // row, so a release build must not offer it.
        #[cfg(debug_assertions)]
        providers.insert(StubProvider.id().to_string(), Box::new(StubProvider));
        Self { providers }
    }

    pub fn list(&self) -> Vec<ProviderInfo> {
        self.providers
            .values()
            .map(|p| ProviderInfo {
                id: p.id().to_string(),
                name: p.name().to_string(),
            })
            .collect()
    }

    pub(crate) async fn fetch_from(
        &self,
        provider_id: &str,
        title: &str,
    ) -> Result<Option<FullMetadata>> {
        let provider = self
            .providers
            .get(provider_id)
            .ok_or_else(|| ServiceError::NotFound(format!("Provider '{provider_id}' not found")))?;
        provider.fetch(title).await
    }
}

impl Default for MetadataProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Provider identity exposed to clients selecting an enrichment source.
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
}

#[cfg(debug_assertions)]
struct StubProvider;

#[cfg(debug_assertions)]
#[async_trait::async_trait]
impl MetadataProvider for StubProvider {
    fn id(&self) -> &'static str {
        "stub"
    }

    fn name(&self) -> &'static str {
        "Stub (Test)"
    }

    async fn fetch(&self, title: &str) -> Result<Option<FullMetadata>> {
        Ok(Some(FullMetadata {
            title: Some(title.to_string()),
            description: Some(format!("Stub description for '{title}'")),
            cover_url: None,
            authors: vec![],
            artists: vec![],
            tags: vec![],
            external_ids: HashMap::new(),
        }))
    }
}

/// Fields changed by an enrich operation, returned to the caller for display.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnrichResult {
    pub fields_updated: Vec<String>,
}

/// AppService extension methods for metadata enrichment.
impl super::AppService {
    pub async fn enrich_manga_metadata(
        &self,
        manga_id: MangaId,
        provider_id: &str,
        user_id: UserId,
    ) -> Result<EnrichResult> {
        let manga = self.get_manga_by_id(manga_id).await?;
        let registry = self.metadata_provider_registry.read().await;
        let fetched = registry
            .fetch_from(provider_id, &manga.name)
            .await?
            .ok_or_else(|| ServiceError::NotFound("Provider returned no result".into()))?;
        drop(registry);

        let mut fields_updated = Vec::new();

        let new_desc = if manga.local_description.is_none() {
            fetched.description.as_deref()
        } else {
            None
        };

        if new_desc.is_some() {
            fields_updated.push("description".to_string());
            sqlx::query("UPDATE manga SET description = COALESCE(?, description) WHERE id = ?")
                .bind(new_desc)
                .bind(manga_id.0)
                .execute(&self.db)
                .await
                .map_err(ServiceError::Db)?;
        }

        for (provider, external_id) in &fetched.external_ids {
            let rows_affected = sqlx::query(
                "INSERT INTO manga_external_ids (manga_id, provider, external_id)
                 VALUES (?, ?, ?)
                 ON CONFLICT (manga_id, provider) DO NOTHING",
            )
            .bind(manga_id.0)
            .bind(provider)
            .bind(external_id)
            .execute(&self.db)
            .await
            .map_err(ServiceError::Db)?
            .rows_affected();

            if rows_affected > 0 {
                fields_updated.push(format!("external_id:{provider}"));
            }
        }

        if !fields_updated.is_empty() {
            self.audit(
                Some(user_id),
                "manga.enrich_metadata",
                Some(&manga.name),
                Some(serde_json::json!({
                    "provider": provider_id,
                    "fields": fields_updated,
                })),
            )
            .await;
        }

        Ok(EnrichResult { fields_updated })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stub is compiled out of release builds, so the meaningful half of this
    /// only runs under `cargo test --release`, which nothing does today. It is here
    /// so that a future release-profile run reports the regression rather than
    /// shipping fabricated metadata quietly.
    #[test]
    fn the_stub_provider_is_absent_from_a_release_build() {
        let registry = MetadataProviderRegistry::new();
        if cfg!(debug_assertions) {
            assert_eq!(
                registry.list().len(),
                1,
                "the stub stays available for development"
            );
        } else {
            assert!(
                registry.list().is_empty(),
                "the stub fabricates descriptions that enrichment writes to the manga row"
            );
        }
    }
}
