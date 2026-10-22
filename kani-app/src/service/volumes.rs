use crate::error::{Result, ServiceError};
use crate::ids::{ChapterId, MangaId};
use crate::service::AppService;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Volume {
    pub id: i64,
    pub manga_id: i64,
    pub name: Option<String>,
    pub volume_num: Option<f64>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
}

impl AppService {
    pub async fn list_volumes(&self, manga_id: MangaId) -> Result<Vec<Volume>> {
        let rows = sqlx::query!(
            r#"SELECT id as "id!", manga_id as "manga_id!", name, volume_num, created_at
               FROM volumes WHERE manga_id = ? ORDER BY volume_num ASC NULLS LAST, id ASC"#,
            manga_id
        )
        .fetch_all(&self.db_read)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Volume {
                id: r.id,
                manga_id: r.manga_id,
                name: r.name,
                volume_num: r.volume_num,
                created_at: r.created_at,
            })
            .collect())
    }

    pub async fn create_volume(
        &self,
        manga_id: MangaId,
        name: Option<String>,
        volume_num: Option<f64>,
    ) -> Result<Volume> {
        let id = sqlx::query_scalar!(
            r#"INSERT INTO volumes (manga_id, name, volume_num) VALUES (?, ?, ?) RETURNING id as "id!""#,
            manga_id,
            name,
            volume_num,
        )
        .fetch_one(&self.db)
        .await?;

        let row = sqlx::query!(
            r#"SELECT id as "id!", manga_id as "manga_id!", name, volume_num, created_at
               FROM volumes WHERE id = ?"#,
            id
        )
        .fetch_one(&self.db_read)
        .await?;

        Ok(Volume {
            id: row.id,
            manga_id: row.manga_id,
            name: row.name,
            volume_num: row.volume_num,
            created_at: row.created_at,
        })
    }

    pub async fn update_volume(
        &self,
        volume_id: i64,
        manga_id: MangaId,
        name: Option<String>,
        volume_num: Option<f64>,
    ) -> Result<Volume> {
        let rows = sqlx::query!(
            "UPDATE volumes SET name = ?, volume_num = ? WHERE id = ? AND manga_id = ?",
            name,
            volume_num,
            volume_id,
            manga_id,
        )
        .execute(&self.db)
        .await?
        .rows_affected();

        if rows == 0 {
            return Err(ServiceError::NotFound(format!(
                "Volume {volume_id} not found"
            )));
        }

        let row = sqlx::query!(
            r#"SELECT id as "id!", manga_id as "manga_id!", name, volume_num, created_at
               FROM volumes WHERE id = ?"#,
            volume_id
        )
        .fetch_one(&self.db_read)
        .await?;

        Ok(Volume {
            id: row.id,
            manga_id: row.manga_id,
            name: row.name,
            volume_num: row.volume_num,
            created_at: row.created_at,
        })
    }

    pub async fn delete_volume(&self, volume_id: i64, manga_id: MangaId) -> Result<()> {
        let rows = sqlx::query!(
            "DELETE FROM volumes WHERE id = ? AND manga_id = ?",
            volume_id,
            manga_id,
        )
        .execute(&self.db)
        .await?
        .rows_affected();

        if rows == 0 {
            return Err(ServiceError::NotFound(format!(
                "Volume {volume_id} not found"
            )));
        }
        Ok(())
    }

    pub async fn assign_chapter_volume(
        &self,
        chapter_id: ChapterId,
        manga_id: MangaId,
        volume_id: Option<i64>,
    ) -> Result<()> {
        if let Some(vid) = volume_id {
            let exists = sqlx::query_scalar!(
                "SELECT COUNT(*) FROM volumes WHERE id = ? AND manga_id = ?",
                vid,
                manga_id,
            )
            .fetch_one(&self.db_read)
            .await?;
            if exists == 0 {
                return Err(ServiceError::NotFound(format!("Volume {vid} not found")));
            }
        }

        let rows = sqlx::query!(
            "UPDATE chapters SET volume_id = ? WHERE id = ? AND manga_id = ?",
            volume_id,
            chapter_id,
            manga_id,
        )
        .execute(&self.db)
        .await?
        .rows_affected();

        if rows == 0 {
            return Err(ServiceError::NotFound(format!(
                "Chapter {chapter_id} not found"
            )));
        }
        Ok(())
    }
}
