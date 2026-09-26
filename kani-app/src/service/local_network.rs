//! Per-source grants that let an operator-authorised source reach named private hosts.

use kani_core::http::SmartClient;
use kani_core::network::LocalGrants;
use sqlx::SqlitePool;

use crate::error::{Result, ServiceError};
use crate::ids::UserId;
use crate::service::AppService;

async fn stored_entries(db: &SqlitePool, source_id: i64) -> Result<Vec<String>> {
    let raw: Option<Option<String>> =
        sqlx::query_scalar("SELECT local_hosts FROM sources WHERE id = ?")
            .bind(source_id)
            .fetch_optional(db)
            .await?;
    Ok(raw
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default())
}

/// The client a source's backend should use: `base` itself, or a copy that may also reach the
/// source's granted private hosts. A stored grant that no longer parses grants nothing.
pub(crate) async fn client_for(db: &SqlitePool, source_id: i64, base: &SmartClient) -> SmartClient {
    let entries = stored_entries(db, source_id).await.unwrap_or_default();
    let grants = match LocalGrants::parse(&entries) {
        Ok(grants) if !grants.is_empty() => grants,
        Ok(_) => return base.clone(),
        Err(e) => {
            tracing::warn!(source_id, "ignoring invalid local-network grant: {e}");
            return base.clone();
        }
    };
    match base.with_local_grants(grants) {
        Ok(client) => client,
        Err(e) => {
            tracing::warn!(source_id, "could not build a granted client: {e}");
            base.clone()
        }
    }
}

impl AppService {
    pub async fn get_source_local_hosts(&self, source_id: i64) -> Result<Vec<String>> {
        self.get_source(source_id).await?;
        stored_entries(&self.db_read, source_id).await
    }

    /// Replaces the private hosts a source may reach, then reloads the source so its requests
    /// use the new grant. An entry that can never be granted is refused before anything changes.
    pub async fn set_source_local_hosts(
        &self,
        source_id: i64,
        hosts: Vec<String>,
        user_id: UserId,
    ) -> Result<Vec<String>> {
        let source = self.get_source(source_id).await?;
        let hosts: Vec<String> = hosts
            .into_iter()
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
            .collect();
        LocalGrants::parse(&hosts).map_err(ServiceError::Validation)?;
        let json = if hosts.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&hosts).map_err(|e| ServiceError::Internal(e.to_string()))?)
        };
        sqlx::query("UPDATE sources SET local_hosts = ? WHERE id = ?")
            .bind(json)
            .bind(source_id)
            .execute(&self.db)
            .await?;
        self.source_clients.remove(&source_id);
        self.audit(
            Some(user_id),
            "source.local_hosts.update",
            Some(&source.name),
            Some(serde_json::json!({ "hosts": hosts })),
        )
        .await;
        if self.sources.contains_key(source_id)
            && let Err(e) = self.reload_source(source_id).await
        {
            tracing::warn!(source_id, "grant saved but the source did not reload: {e}");
        }
        Ok(hosts)
    }

    /// The source's extraction client, honouring its grants. Cached until the grant changes.
    pub async fn smart_client_for_source(&self, source_id: i64) -> SmartClient {
        self.source_client_pair(source_id).await.0
    }

    /// The source's image client (no automatic redirects), honouring its grants.
    pub async fn proxy_client_for_source(&self, source_id: i64) -> SmartClient {
        self.source_client_pair(source_id).await.1
    }

    async fn source_client_pair(&self, source_id: i64) -> (SmartClient, SmartClient) {
        if let Some(pair) = self.source_clients.get(&source_id) {
            return pair.clone();
        }
        let pair = (
            client_for(&self.db_read, source_id, &self.smart_client).await,
            client_for(&self.db_read, source_id, &self.proxy_client).await,
        );
        self.source_clients.insert(source_id, pair.clone());
        pair
    }
}
