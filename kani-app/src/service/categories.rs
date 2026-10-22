use super::*;
use crate::ids::MangaId;

impl AppService {
    pub async fn list_categories(&self) -> Result<Vec<kani_shared::types::Category>> {
        sqlx::query_as!(
            kani_shared::types::Category,
            "SELECT id, name, sort_order FROM categories ORDER BY sort_order ASC, name ASC"
        )
        .fetch_all(&self.db_read)
        .await
        .map_err(Into::into)
    }

    pub async fn create_category(&self, name: &str, sort_order: i64) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ServiceError::Validation(
                "Category name cannot be empty".into(),
            ));
        }
        let id = sqlx::query_scalar!(
            "INSERT INTO categories (name, sort_order) VALUES (?,?) RETURNING id",
            name,
            sort_order
        )
        .fetch_one(&self.db)
        .await?;
        Ok(id)
    }

    pub async fn rename_category(&self, id: i64, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ServiceError::Validation(
                "Category name cannot be empty".into(),
            ));
        }
        sqlx::query!("UPDATE categories SET name=? WHERE id=?", name, id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    pub async fn delete_category(&self, id: i64) -> Result<()> {
        sqlx::query!("DELETE FROM categories WHERE id=?", id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    pub async fn reorder_categories(&self, ordered_ids: Vec<i64>) -> Result<()> {
        let mut tx = self.db.begin().await?;
        for (idx, id) in ordered_ids.into_iter().enumerate() {
            let order = idx as i64;
            sqlx::query!("UPDATE categories SET sort_order=? WHERE id=?", order, id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_manga_categories(
        &self,
        manga_id: MangaId,
    ) -> Result<Vec<kani_shared::types::Category>> {
        sqlx::query_as!(
            kani_shared::types::Category,
            "SELECT c.id, c.name, c.sort_order FROM categories c
             JOIN manga_categories mc ON mc.category_id = c.id
             WHERE mc.manga_id=? ORDER BY c.sort_order ASC, c.name ASC",
            manga_id
        )
        .fetch_all(&self.db_read)
        .await
        .map_err(Into::into)
    }

    pub async fn set_manga_categories(
        &self,
        manga_id: MangaId,
        category_ids: Vec<i64>,
    ) -> Result<()> {
        let mut tx = self.db.begin().await?;
        sqlx::query!("DELETE FROM manga_categories WHERE manga_id=?", manga_id)
            .execute(&mut *tx)
            .await?;
        for cat_id in category_ids {
            sqlx::query!(
                "INSERT OR IGNORE INTO manga_categories (manga_id, category_id) VALUES (?,?)",
                manga_id,
                cat_id
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        self.invalidate_library();
        Ok(())
    }
}
