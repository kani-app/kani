use super::*;
use crate::models::AuditEntry;

impl AppService {
    /// Paginated audit log query with optional filters.
    /// Returns (entries, has_next_page, total_pages).
    #[allow(clippy::too_many_arguments)]
    pub async fn get_audit_log(
        &self,
        user_id_filter: Option<i64>,
        action_filter: Option<&str>,
        from: Option<&str>,
        to: Option<&str>,
        search: Option<&str>,
        page: i32,
        page_size: i32,
    ) -> Result<(Vec<AuditEntry>, bool, Option<u32>)> {
        let offset = ((page - 1).max(0) as i64) * page_size as i64;
        let fetch_size = page_size as i64 + 1;

        let mut qb = sqlx::QueryBuilder::new(
            r#"
            SELECT
                a.id,
                a.user_id,
                u.username,
                a.action,
                a.target,
                a.details,
                a.created_at
            FROM audit_log a
            LEFT JOIN users u ON u.id = a.user_id
            WHERE 1=1
            "#,
        );

        if let Some(uid) = user_id_filter {
            qb.push(" AND a.user_id = ");
            qb.push_bind(uid);
        }
        if let Some(action) = action_filter
            && !action.is_empty()
        {
            qb.push(" AND LOWER(a.action) LIKE '%' || LOWER(");
            qb.push_bind(action);
            qb.push(") || '%'");
        }
        if let Some(from_str) = from
            && !from_str.is_empty()
        {
            qb.push(" AND a.created_at >= ");
            qb.push_bind(from_str);
        }
        if let Some(to_str) = to
            && !to_str.is_empty()
        {
            qb.push(" AND a.created_at <= ");
            qb.push_bind(to_str);
        }
        if let Some(s) = search
            && !s.is_empty()
        {
            qb.push(" AND (LOWER(a.action) LIKE '%' || LOWER(");
            qb.push_bind(s);
            qb.push(") || '%'");
            qb.push(" OR LOWER(COALESCE(a.target,'')) LIKE '%' || LOWER(");
            qb.push_bind(s);
            qb.push(") || '%'");
            qb.push(" OR LOWER(COALESCE(a.details,'')) LIKE '%' || LOWER(");
            qb.push_bind(s);
            qb.push(") || '%')");
        }

        qb.push(" ORDER BY a.created_at DESC LIMIT ");
        qb.push_bind(fetch_size);
        qb.push(" OFFSET ");
        qb.push_bind(offset);

        let mut rows: Vec<AuditEntry> = qb
            .build_query_as::<AuditEntry>()
            .fetch_all(&self.db_read)
            .await?;

        let has_next = rows.len() > page_size as usize;
        if has_next {
            rows.pop();
        }

        let total_pages = if has_next || page > 1 {
            None
        } else {
            Some(page as u32)
        };

        Ok((rows, has_next, total_pages))
    }

    pub(crate) async fn prune_audit_log(&self) -> Result<u64> {
        let (retention_days, security_retention_days) = {
            let s = self.settings.read().await;
            let sec = if s.audit_security_retention_days > 0 {
                Some(s.audit_security_retention_days)
            } else {
                None
            };
            (s.audit_retention_days, sec)
        };

        let security_actions = ["login_failed", "permission_denied", "user_created"];

        let op_interval = format!("-{retention_days} days");
        let result = sqlx::query(
            "DELETE FROM audit_log \
             WHERE action NOT IN ('login_failed', 'permission_denied', 'user_created') \
             AND created_at < datetime('now', ?)",
        )
        .bind(&op_interval)
        .execute(&self.db)
        .await?;

        let mut deleted = result.rows_affected();

        if let Some(sec_days) = security_retention_days {
            let sec_interval = format!("-{sec_days} days");
            for action in &security_actions {
                let r = sqlx::query(
                    "DELETE FROM audit_log WHERE action = ? AND created_at < datetime('now', ?)",
                )
                .bind(action)
                .bind(&sec_interval)
                .execute(&self.db)
                .await?;
                deleted += r.rows_affected();
            }
        }

        Ok(deleted)
    }
}
