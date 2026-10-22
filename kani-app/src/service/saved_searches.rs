use crate::error::{Result, ServiceError};
use crate::ids::UserId;
use crate::service::AppService;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SavedSearch {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub query_json: String,
}

impl AppService {
    pub async fn list_saved_searches(&self, user_id: UserId) -> Result<Vec<SavedSearch>> {
        let rows = sqlx::query!(
            r#"SELECT id as "id!", user_id, name, query_json
               FROM saved_searches WHERE user_id = ? ORDER BY id ASC"#,
            user_id
        )
        .fetch_all(&self.db_read)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| SavedSearch {
                id: r.id,
                user_id: r.user_id,
                name: r.name,
                query_json: r.query_json,
            })
            .collect())
    }

    pub async fn create_saved_search(
        &self,
        user_id: UserId,
        name: String,
        query_json: String,
    ) -> Result<SavedSearch> {
        let id = sqlx::query_scalar!(
            r#"INSERT INTO saved_searches (user_id, name, query_json) VALUES (?, ?, ?) RETURNING id as "id!""#,
            user_id,
            name,
            query_json,
        )
        .fetch_one(&self.db)
        .await?;

        Ok(SavedSearch {
            id,
            user_id: user_id.0,
            name,
            query_json,
        })
    }

    pub async fn update_saved_search(
        &self,
        id: i64,
        user_id: UserId,
        name: String,
        query_json: String,
    ) -> Result<SavedSearch> {
        let rows = sqlx::query!(
            "UPDATE saved_searches SET name = ?, query_json = ? WHERE id = ? AND user_id = ?",
            name,
            query_json,
            id,
            user_id,
        )
        .execute(&self.db)
        .await?
        .rows_affected();

        if rows == 0 {
            return Err(ServiceError::NotFound(format!(
                "Saved search {id} not found"
            )));
        }

        Ok(SavedSearch {
            id,
            user_id: user_id.0,
            name,
            query_json,
        })
    }

    pub async fn delete_saved_search(&self, id: i64, user_id: UserId) -> Result<()> {
        let rows = sqlx::query!(
            "DELETE FROM saved_searches WHERE id = ? AND user_id = ?",
            id,
            user_id,
        )
        .execute(&self.db)
        .await?
        .rows_affected();

        if rows == 0 {
            return Err(ServiceError::NotFound(format!(
                "Saved search {id} not found"
            )));
        }
        Ok(())
    }
}
