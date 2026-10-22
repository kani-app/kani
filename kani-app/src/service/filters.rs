use super::*;

impl AppService {
    pub async fn get_filter_tags(&self) -> Result<Vec<kani_shared::types::NamedItem>> {
        sqlx::query_as!(
            kani_shared::types::NamedItem,
            "SELECT id, name FROM tags ORDER BY name"
        )
        .fetch_all(&self.db_read)
        .await
        .map_err(Into::into)
    }

    pub async fn get_filter_authors(&self) -> Result<Vec<kani_shared::types::NamedItem>> {
        sqlx::query_as!(
            kani_shared::types::NamedItem,
            "SELECT p.id, p.name FROM people p \
             JOIN manga_people mp ON mp.person_id = p.id \
             WHERE mp.role = 'author' ORDER BY p.name"
        )
        .fetch_all(&self.db_read)
        .await
        .map_err(Into::into)
    }

    pub async fn get_filter_artists(&self) -> Result<Vec<kani_shared::types::NamedItem>> {
        sqlx::query_as!(
            kani_shared::types::NamedItem,
            "SELECT p.id, p.name FROM people p \
             JOIN manga_people mp ON mp.person_id = p.id \
             WHERE mp.role = 'artist' ORDER BY p.name"
        )
        .fetch_all(&self.db_read)
        .await
        .map_err(Into::into)
    }
}
