use crate::error::{Result, ServiceError};
use crate::ids::{MangaId, UserId};
use crate::service::AppService;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SmartCollectionRule {
    And { rules: Vec<SmartCollectionRule> },
    Or { rules: Vec<SmartCollectionRule> },
    Status { value: i64 },
    Tag { name: String },
    HasUnread,
    ChapterCountGt { value: i64 },
    ChapterCountLt { value: i64 },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SmartCollection {
    pub id: i64,
    pub name: String,
    pub rule_json: String,
    pub sort_order: i64,
}

impl AppService {
    pub async fn list_collections(&self) -> Result<Vec<SmartCollection>> {
        let rows = sqlx::query!(
            r#"SELECT id as "id!", name, rule_json, sort_order
               FROM smart_collections ORDER BY sort_order ASC, id ASC"#
        )
        .fetch_all(&self.db_read)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| SmartCollection {
                id: r.id,
                name: r.name,
                rule_json: r.rule_json,
                sort_order: r.sort_order,
            })
            .collect())
    }

    pub async fn get_collection(&self, id: i64) -> Result<SmartCollection> {
        let row = sqlx::query!(
            r#"SELECT id as "id!", name, rule_json, sort_order
               FROM smart_collections WHERE id = ?"#,
            id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("Collection {id} not found")))?;

        Ok(SmartCollection {
            id: row.id,
            name: row.name,
            rule_json: row.rule_json,
            sort_order: row.sort_order,
        })
    }

    pub async fn create_collection(
        &self,
        name: String,
        rule: &SmartCollectionRule,
        sort_order: i64,
    ) -> Result<SmartCollection> {
        let rule_json = serde_json::to_string(rule)
            .map_err(|e| ServiceError::Internal(format!("Rule serialization: {e}")))?;

        let id = sqlx::query_scalar!(
            r#"INSERT INTO smart_collections (name, rule_json, sort_order) VALUES (?, ?, ?) RETURNING id as "id!""#,
            name,
            rule_json,
            sort_order,
        )
        .fetch_one(&self.db)
        .await?;

        Ok(SmartCollection {
            id,
            name,
            rule_json,
            sort_order,
        })
    }

    pub async fn update_collection(
        &self,
        id: i64,
        name: String,
        rule: &SmartCollectionRule,
        sort_order: i64,
    ) -> Result<SmartCollection> {
        let rule_json = serde_json::to_string(rule)
            .map_err(|e| ServiceError::Internal(format!("Rule serialization: {e}")))?;

        let rows = sqlx::query!(
            "UPDATE smart_collections SET name = ?, rule_json = ?, sort_order = ? WHERE id = ?",
            name,
            rule_json,
            sort_order,
            id,
        )
        .execute(&self.db)
        .await?
        .rows_affected();

        if rows == 0 {
            return Err(ServiceError::NotFound(format!("Collection {id} not found")));
        }

        Ok(SmartCollection {
            id,
            name,
            rule_json,
            sort_order,
        })
    }

    pub async fn delete_collection(&self, id: i64) -> Result<()> {
        let rows = sqlx::query!("DELETE FROM smart_collections WHERE id = ?", id)
            .execute(&self.db)
            .await?
            .rows_affected();

        if rows == 0 {
            return Err(ServiceError::NotFound(format!("Collection {id} not found")));
        }
        Ok(())
    }

    pub async fn evaluate_collection(
        &self,
        rule: &SmartCollectionRule,
        user_id: UserId,
    ) -> Result<Vec<MangaId>> {
        let all_ids: Vec<MangaId> = sqlx::query_scalar!(
            r#"SELECT id as "id: MangaId" FROM manga WHERE deleted_at IS NULL"#
        )
        .fetch_all(&self.db_read)
        .await?;

        self.filter_by_rule(all_ids, rule.clone(), user_id).await
    }

    fn filter_by_rule(
        &self,
        candidates: Vec<MangaId>,
        rule: SmartCollectionRule,
        user_id: UserId,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<MangaId>>> + Send + '_>>
    {
        Box::pin(async move {
            match rule {
                SmartCollectionRule::And { rules } => {
                    let mut result = candidates;
                    for sub in rules {
                        result = self.filter_by_rule(result, sub, user_id).await?;
                    }
                    Ok(result)
                }
                SmartCollectionRule::Or { rules } => {
                    let mut seen = std::collections::HashSet::new();
                    let mut out = Vec::new();
                    for sub in rules {
                        for id in self
                            .filter_by_rule(candidates.clone(), sub, user_id)
                            .await?
                        {
                            if seen.insert(id) {
                                out.push(id);
                            }
                        }
                    }
                    Ok(out)
                }
                SmartCollectionRule::Status { value } => {
                    let matched: std::collections::HashSet<MangaId> = sqlx::query_scalar!(
                    r#"SELECT id as "id: MangaId" FROM manga WHERE status = ? AND deleted_at IS NULL"#,
                    value
                )
                .fetch_all(&self.db_read)
                .await?
                .into_iter()
                .collect();
                    Ok(candidates
                        .iter()
                        .copied()
                        .filter(|id| matched.contains(id))
                        .collect())
                }
                SmartCollectionRule::Tag { name } => {
                    let matched: std::collections::HashSet<MangaId> = sqlx::query_scalar!(
                        r#"SELECT DISTINCT mt.manga_id as "manga_id: MangaId"
                       FROM manga_tags mt
                       JOIN tags t ON t.id = mt.tag_id
                       WHERE t.name = ?"#,
                        name
                    )
                    .fetch_all(&self.db_read)
                    .await?
                    .into_iter()
                    .collect();
                    Ok(candidates
                        .iter()
                        .copied()
                        .filter(|id| matched.contains(id))
                        .collect())
                }
                SmartCollectionRule::HasUnread => {
                    let matched: std::collections::HashSet<MangaId> = sqlx::query_scalar!(
                        r#"SELECT DISTINCT c.manga_id as "manga_id: MangaId"
                       FROM chapters c
                       LEFT JOIN user_chapter_tracking uct
                         ON uct.chapter_id = c.id AND uct.user_id = ?
                       WHERE uct.is_read IS NULL OR uct.is_read = 0"#,
                        user_id
                    )
                    .fetch_all(&self.db_read)
                    .await?
                    .into_iter()
                    .collect();
                    Ok(candidates
                        .iter()
                        .copied()
                        .filter(|id| matched.contains(id))
                        .collect())
                }
                SmartCollectionRule::ChapterCountGt { value } => {
                    let matched: std::collections::HashSet<MangaId> = sqlx::query_scalar!(
                        r#"SELECT manga_id as "manga_id: MangaId"
                       FROM chapters
                       GROUP BY manga_id HAVING COUNT(*) > ?"#,
                        value
                    )
                    .fetch_all(&self.db_read)
                    .await?
                    .into_iter()
                    .collect();
                    Ok(candidates
                        .iter()
                        .copied()
                        .filter(|id| matched.contains(id))
                        .collect())
                }
                SmartCollectionRule::ChapterCountLt { value } => {
                    let matched: std::collections::HashSet<MangaId> = sqlx::query_scalar!(
                        r#"SELECT manga_id as "manga_id: MangaId"
                       FROM chapters
                       GROUP BY manga_id HAVING COUNT(*) < ?"#,
                        value
                    )
                    .fetch_all(&self.db_read)
                    .await?
                    .into_iter()
                    .collect();
                    Ok(candidates
                        .iter()
                        .copied()
                        .filter(|id| matched.contains(id))
                        .collect())
                }
            }
        })
    }
}
