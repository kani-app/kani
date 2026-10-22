use super::*;

impl AppService {
    /// Returns all preferences for a source as (key, value) pairs.
    pub async fn get_all_preferences(&self, source_id: i64) -> Result<Vec<(String, String)>> {
        let rows = sqlx::query!(
            "SELECT key, value FROM source_preferences WHERE source_id=?",
            source_id
        )
        .fetch_all(&self.db_read)
        .await?
        .into_iter()
        .map(|r| (r.key, r.value))
        .collect();
        Ok(rows)
    }

    pub async fn get_preference(&self, source_id: i64, key: &str) -> Result<Option<String>> {
        sqlx::query_scalar!(
            "SELECT value FROM source_preferences WHERE source_id = ? AND key = ?",
            source_id,
            key
        )
        .fetch_optional(&self.db_read)
        .await
        .map_err(Into::into)
    }

    pub async fn set_preference(&self, source_id: i64, key: &str, value: &str) -> Result<()> {
        sqlx::query!("INSERT INTO source_preferences (source_id, key, value) VALUES (?, ?, ?) ON CONFLICT (source_id, key) DO UPDATE SET value = excluded.value", source_id, key, value)
            .execute(&self.db).await?;
        self.cache.invalidate_source(source_id);
        self.reload_preferences(source_id).await
    }

    pub(crate) async fn reload_preferences(&self, source_id: i64) -> Result<()> {
        let prefs = self.load_pref_map(source_id).await?;
        if let Some(backend) = self.sources.get_backend(source_id) {
            backend.update_preferences(prefs);
        }
        Ok(())
    }

    pub(crate) async fn load_pref_map(&self, source_id: i64) -> Result<HashMap<String, String>> {
        Self::load_pref_map_static(&self.db, source_id).await
    }

    pub(super) async fn load_pref_map_static(
        db: &SqlitePool,
        source_id: i64,
    ) -> Result<HashMap<String, String>> {
        let raw = sqlx::query!(
            "SELECT key, value FROM source_preferences WHERE source_id = ?",
            source_id
        )
        .fetch_all(db)
        .await?;

        let mut map = HashMap::new();
        for row in raw {
            map.insert(row.key, row.value);
        }

        Ok(map)
    }

    pub async fn append_pref_list_item(
        &self,
        source_id: i64,
        key: &str,
        item: String,
    ) -> Result<()> {
        if item.trim().is_empty() {
            return Err(ServiceError::Validation("Item cannot be empty".into()));
        }
        let row = self.get_preference(source_id, key).await?;
        let mut list: Vec<String> = row
            .as_deref()
            .and_then(|v| serde_json::from_str(v).ok())
            .unwrap_or_default();
        if !list.contains(&item) {
            list.push(item);
        }
        self.set_preference(
            source_id,
            key,
            &serde_json::to_string(&list).map_err(|e| ServiceError::Internal(e.to_string()))?,
        )
        .await
    }

    pub async fn remove_pref_list_item(&self, source_id: i64, key: &str, item: &str) -> Result<()> {
        let row = self.get_preference(source_id, key).await?;
        let mut list: Vec<String> = row
            .as_deref()
            .and_then(|v| serde_json::from_str(v).ok())
            .unwrap_or_default();
        list.retain(|x| x != item);
        self.set_preference(
            source_id,
            key,
            &serde_json::to_string(&list).map_err(|e| ServiceError::Internal(e.to_string()))?,
        )
        .await
    }

    pub async fn get_source_pref_schema(
        &self,
        source_id: i64,
    ) -> Result<Vec<kani_core::PreferenceSpec>> {
        if let Some(cached) = self.cache.get_preference_schema(source_id) {
            return Ok(cached);
        }
        let backend = self.sources.get_backend(source_id);
        let raw = if let Some(backend) = backend {
            backend
                .get_preferences()
                .await
                .map_err(ServiceError::Core)?
        } else {
            let name = sqlx::query_scalar!("SELECT name FROM sources WHERE id=?", source_id)
                .fetch_optional(&self.db_read)
                .await?
                .ok_or_else(|| ServiceError::NotFound(format!("Source {source_id} not found")))?;
            let wasm_path = self
                .settings
                .read()
                .await
                .wasm_storage_path
                .join(format!("{}.wasm", name));
            let bytes = tokio::fs::read(&wasm_path)
                .await
                .map_err(|e| ServiceError::Core(kani_core::Error::Io(e)))?;
            let component = self
                .wasm_runtime
                .compile_component(&bytes)
                .map_err(ServiceError::Core)?;
            let mut inst =
                kani_core::sources::SourceInstance::new(self.smart_client.clone(), None, false);
            inst.load(
                self.wasm_runtime.engine(),
                &component,
                self.wasm_runtime.linker(),
            )
            .await
            .map_err(ServiceError::Core)?;
            inst.get_preferences().await.map_err(ServiceError::Core)?
        };
        self.cache.insert_preference_schema(source_id, raw.clone());
        Ok(raw)
    }

    pub async fn toggle_pref_select_item(
        &self,
        source_id: i64,
        key: &str,
        item: String,
        selected: bool,
    ) -> Result<()> {
        let row = self.get_preference(source_id, key).await?;
        let mut list: Vec<String> = row
            .as_deref()
            .and_then(|v| serde_json::from_str(v).ok())
            .unwrap_or_default();
        if selected {
            if !list.contains(&item) {
                list.push(item);
            }
        } else {
            list.retain(|x| x != &item);
        }
        self.set_preference(
            source_id,
            key,
            &serde_json::to_string(&list).map_err(|e| ServiceError::Internal(e.to_string()))?,
        )
        .await
    }
}
