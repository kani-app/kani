use super::*;
use crate::ids::UserId;
use crate::models::{DailyActivity, GenreCount, MangaReadCount, PaceEntry, ReadingStats};
use std::sync::Arc;
use time::macros::format_description;

static DATE_FMT: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day]");

impl AppService {
    pub async fn get_reading_stats(
        &self,
        user_id: UserId,
        period_days: i32,
    ) -> Result<Arc<ReadingStats>> {
        let key = (user_id.0, period_days);
        if let Some(cached) = self.cache.stats.get(&key).await {
            return Ok(cached);
        }
        let stats = Arc::new(self.compute_reading_stats(user_id, period_days).await?);
        self.cache.stats.insert(key, stats.clone()).await;
        Ok(stats)
    }

    async fn compute_reading_stats(
        &self,
        user_id: UserId,
        period_days: i32,
    ) -> Result<ReadingStats> {
        // Compute cutoff date in Rust to avoid SQLite string concat in datemod arg
        let cutoff = (time::OffsetDateTime::now_utc() - time::Duration::days(period_days as i64))
            .date()
            .format(DATE_FMT)
            .unwrap_or_default();

        let (totals, completed_manga, daily_rows, top_rows, genre_rows, pace_rows) = tokio::try_join!(
            sqlx::query!(
                r#"
                SELECT
                    COUNT(*) as "chapters_read!: i64",
                    COUNT(DISTINCT c.manga_id) as "manga_read!: i64"
                FROM user_chapter_tracking uct
                JOIN chapters c ON c.id = uct.chapter_id
                WHERE uct.is_read = TRUE AND uct.user_id = ?
                "#,
                user_id
            )
            .fetch_one(&self.db_read),
            sqlx::query_scalar!(
                r#"SELECT COUNT(*) as "count: i64" FROM user_manga_tracking WHERE user_id = ? AND status = 4"#,
                user_id
            )
            .fetch_one(&self.db_read),
            sqlx::query!(
                r#"
                SELECT
                    DATE(last_read_at) as "date!: String",
                    COUNT(*) as "chapters_read!: i64"
                FROM user_chapter_tracking
                WHERE is_read = TRUE AND user_id = ? AND last_read_at >= ?
                GROUP BY DATE(last_read_at)
                ORDER BY 1 ASC
                "#,
                user_id,
                cutoff
            )
            .fetch_all(&self.db_read),
            sqlx::query!(
                r#"
                SELECT
                    m.id as "manga_id!: i64",
                    COALESCE(m.local_name, m.name) as "manga_name!: String",
                    COUNT(uct.chapter_id) as "chapters_read!: i64"
                FROM user_chapter_tracking uct
                JOIN chapters c ON c.id = uct.chapter_id
                JOIN manga m ON m.id = c.manga_id
                WHERE uct.is_read = TRUE AND uct.user_id = ?
                GROUP BY m.id
                ORDER BY 3 DESC
                LIMIT 10
                "#,
                user_id
            )
            .fetch_all(&self.db_read),
            sqlx::query!(
                r#"
                SELECT
                    t.name as "genre!: String",
                    COUNT(uct.chapter_id) as "chapters_read!: i64"
                FROM user_chapter_tracking uct
                JOIN chapters c ON c.id = uct.chapter_id
                JOIN manga_tags mt ON mt.manga_id = c.manga_id
                JOIN tags t ON t.id = mt.tag_id
                WHERE uct.is_read = TRUE AND uct.user_id = ?
                GROUP BY t.name
                ORDER BY 2 DESC
                LIMIT 15
                "#,
                user_id
            )
            .fetch_all(&self.db_read),
            sqlx::query!(
                r#"
                SELECT
                    DATE(uct.last_read_at)         AS "date!: String",
                    COALESCE(SUM(c.page_count), 0)  AS "pages!: i64"
                FROM user_chapter_tracking uct
                JOIN chapters c ON c.id = uct.chapter_id
                WHERE uct.is_read = TRUE AND uct.user_id = ? AND uct.last_read_at >= ?
                  AND c.page_count IS NOT NULL
                GROUP BY DATE(uct.last_read_at)
                ORDER BY 1 ASC
                "#,
                user_id,
                cutoff
            )
            .fetch_all(&self.db_read),
        )?;

        let daily_activity: Vec<DailyActivity> = daily_rows
            .into_iter()
            .map(|r| DailyActivity {
                date: r.date,
                chapters_read: r.chapters_read,
            })
            .collect();
        let top_manga: Vec<MangaReadCount> = top_rows
            .into_iter()
            .map(|r| MangaReadCount {
                manga_id: r.manga_id,
                manga_name: r.manga_name,
                chapters_read: r.chapters_read,
            })
            .collect();
        let genre_breakdown: Vec<GenreCount> = genre_rows
            .into_iter()
            .map(|r| GenreCount {
                genre: r.genre,
                chapters_read: r.chapters_read,
            })
            .collect();

        let reading_pace: Vec<PaceEntry> = pace_rows
            .into_iter()
            .map(|r| PaceEntry {
                date: r.date,
                pages: r.pages,
            })
            .collect();

        let (current_streak, longest_streak) = calculate_streaks(&daily_activity);

        Ok(ReadingStats {
            total_chapters_read: totals.chapters_read,
            total_manga_read: totals.manga_read,
            completed_manga,
            current_streak,
            longest_streak,
            daily_activity,
            top_manga,
            genre_breakdown,
            reading_pace,
        })
    }

    pub(crate) async fn capture_storage_snapshot(&self) -> crate::error::Result<()> {
        let library_path = self.settings.read().await.library_path.clone();

        let library_used = dir_size_bytes(&library_path).await;
        let data_path = library_path.parent().unwrap_or(&library_path).to_path_buf();
        let data_used = dir_size_bytes(&data_path.join("kani.db")).await;
        let cover_used = dir_size_bytes(&library_path.join("covers")).await;
        let chapter_used = library_used.saturating_sub(cover_used);

        let free_bytes = fs2::free_space(&library_path).unwrap_or(0);

        let total_manga: i64 =
            sqlx::query_scalar!("SELECT COUNT(*) FROM manga WHERE deleted_at IS NULL")
                .fetch_one(&self.db_read)
                .await?;

        let total_chapters: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM chapters")
            .fetch_one(&self.db_read)
            .await?;

        let library_used_i64 = library_used as i64;
        let cover_used_i64 = cover_used as i64;
        let chapter_used_i64 = chapter_used as i64;
        let data_used_i64 = data_used as i64;
        let free_bytes_i64 = free_bytes as i64;

        sqlx::query!(
            "INSERT INTO storage_history \
             (library_used_bytes, cover_used_bytes, chapter_used_bytes, \
              data_used_bytes, free_bytes, total_manga, total_chapters) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            library_used_i64,
            cover_used_i64,
            chapter_used_i64,
            data_used_i64,
            free_bytes_i64,
            total_manga,
            total_chapters
        )
        .execute(&self.db)
        .await?;

        let warn_threshold: f64 = self.settings.read().await.disk_warn_threshold;

        let total_bytes = (free_bytes as i64).saturating_add(library_used as i64);
        if total_bytes > 0 {
            let free_fraction = free_bytes as f64 / total_bytes as f64;
            if free_fraction < warn_threshold {
                tracing::warn!(
                    "Low disk space on library path: {:.1}% free ({} bytes)",
                    free_fraction * 100.0,
                    free_bytes
                );
            }
        }

        sqlx::query!("DELETE FROM storage_history WHERE captured_at < datetime('now', '-90 days')")
            .execute(&self.db)
            .await?;

        Ok(())
    }

    pub async fn get_storage_stats(&self) -> crate::error::Result<StorageStats> {
        let library_path = self.settings.read().await.library_path.clone();

        let library_used = dir_size_bytes(&library_path).await;
        let cover_used = dir_size_bytes(&library_path.join("covers")).await;
        let chapter_used = library_used.saturating_sub(cover_used);
        let free_bytes = fs2::free_space(&library_path).unwrap_or(0);

        let data_path = library_path.parent().unwrap_or(&library_path).to_path_buf();
        let data_used = dir_size_bytes(&data_path.join("kani.db")).await;

        let total_manga: i64 =
            sqlx::query_scalar!("SELECT COUNT(*) FROM manga WHERE deleted_at IS NULL")
                .fetch_one(&self.db_read)
                .await?;

        let total_chapters: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM chapters")
            .fetch_one(&self.db_read)
            .await?;

        let total_downloads: i64 =
            sqlx::query_scalar!("SELECT COUNT(*) FROM chapters WHERE download_status = 2")
                .fetch_one(&self.db_read)
                .await?;

        Ok(StorageStats {
            library_used_bytes: library_used,
            library_free_bytes: free_bytes,
            cover_used_bytes: cover_used,
            chapter_used_bytes: chapter_used,
            data_used_bytes: data_used,
            total_manga: total_manga as u64,
            total_chapters: total_chapters as u64,
            total_downloads: total_downloads as u64,
        })
    }
}

#[derive(Debug, serde::Serialize)]
pub struct StorageStats {
    pub library_used_bytes: u64,
    pub library_free_bytes: u64,
    pub cover_used_bytes: u64,
    pub chapter_used_bytes: u64,
    pub data_used_bytes: u64,
    pub total_manga: u64,
    pub total_chapters: u64,
    pub total_downloads: u64,
}

async fn dir_size_bytes(path: &std::path::Path) -> u64 {
    use tokio::fs;
    let mut total = 0u64;
    let Ok(mut rd) = fs::read_dir(path).await else {
        return 0;
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        if meta.is_file() {
            total += meta.len();
        } else if meta.is_dir() {
            total += dir_size_bytes_sync(&entry.path());
        }
    }
    total
}

fn dir_size_bytes_sync(path: &std::path::Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in rd.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_file() {
            total += meta.len();
        } else if meta.is_dir() {
            total += dir_size_bytes_sync(&entry.path());
        }
    }
    total
}

fn calculate_streaks(activity: &[DailyActivity]) -> (i64, i64) {
    if activity.is_empty() {
        return (0, 0);
    }

    let mut dates: Vec<&str> = activity.iter().map(|d| d.date.as_str()).collect();
    dates.sort_unstable();
    dates.dedup();

    if dates.is_empty() {
        return (0, 0);
    }

    let mut longest: i64 = 1;
    let mut run: i64 = 1;
    for i in 1..dates.len() {
        if is_next_day(dates[i - 1], dates[i]) {
            run += 1;
            if run > longest {
                longest = run;
            }
        } else {
            run = 1;
        }
    }

    let now = time::OffsetDateTime::now_utc();
    let today = now.date().format(DATE_FMT).unwrap_or_default();
    let yesterday = (now - time::Duration::days(1))
        .date()
        .format(DATE_FMT)
        .unwrap_or_default();

    let last = *dates.last().expect("non-empty");
    if last != today && last != yesterday {
        return (0, longest);
    }

    let mut current: i64 = 1;
    let mut i = dates.len();
    while i > 1 {
        if is_next_day(dates[i - 2], dates[i - 1]) {
            current += 1;
            i -= 1;
        } else {
            break;
        }
    }

    (current, longest)
}

fn is_next_day(a: &str, b: &str) -> bool {
    let Some((ay, am, ad)) = parse_ymd(a) else {
        return false;
    };
    let Some((by_, bm, bd)) = parse_ymd(b) else {
        return false;
    };

    let dim = |y: u32, m: u32| -> u32 {
        match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400) {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        }
    };

    let (ny, nm, nd) = if ad < dim(ay, am) {
        (ay, am, ad + 1)
    } else if am < 12 {
        (ay, am + 1, 1)
    } else {
        (ay + 1, 1, 1)
    };

    by_ == ny && bm == nm && bd == nd
}

fn parse_ymd(s: &str) -> Option<(u32, u32, u32)> {
    if s.len() < 10 {
        return None;
    }
    Some((
        s[0..4].parse().ok()?,
        s[5..7].parse().ok()?,
        s[8..10].parse().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acts(dates: &[&str]) -> Vec<DailyActivity> {
        dates
            .iter()
            .map(|d| DailyActivity {
                date: d.to_string(),
                chapters_read: 1,
            })
            .collect()
    }

    #[test]
    fn streak_empty() {
        assert_eq!(calculate_streaks(&[]), (0, 0));
    }

    #[test]
    fn streak_single_old_day() {
        let (cur, best) = calculate_streaks(&acts(&["2020-01-01"]));
        assert_eq!(cur, 0);
        assert_eq!(best, 1);
    }

    #[test]
    fn streak_gap_resets_longest() {
        let (_, best) = calculate_streaks(&acts(&["2026-01-01", "2026-01-02", "2026-01-04"]));
        assert_eq!(best, 2);
    }

    #[test]
    fn is_next_day_month_boundary() {
        assert!(is_next_day("2026-01-31", "2026-02-01"));
    }

    #[test]
    fn is_next_day_year_boundary() {
        assert!(is_next_day("2025-12-31", "2026-01-01"));
    }

    #[test]
    fn is_next_day_leap_year() {
        assert!(is_next_day("2024-02-28", "2024-02-29"));
        assert!(!is_next_day("2025-02-28", "2025-02-29"));
        assert!(is_next_day("2025-02-28", "2025-03-01"));
    }
}
