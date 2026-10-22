use std::collections::BTreeSet;

use sha2::{Digest, Sha384};
use sqlx::migrate::Migrator;
use sqlx::{Row, SqlitePool};

use crate::error::{Result, ServiceError};

pub(super) static MIGRATOR: Migrator = sqlx::migrate!("../migrations");

/// The squash that replaced the pre-1.0 migration history.
///
/// Deliberately a version no installation ever applied: a database that reaches
/// `MIGRATOR` without being adopted first fails with an unrecognised-history
/// error rather than comparing the baseline's checksum against a row recording a
/// different migration.
const BASELINE_VERSION: i64 = 20260818000002;

/// Every version the baseline folded in, as recorded by a pre-squash install.
///
/// Adoption demands this set exactly, so a database missing any of them is
/// refused rather than stamped at a schema it does not have.
const FOLDED_VERSIONS: &[i64] = &[
    20260204111500,
    20260204111742,
    20260204111832,
    20260204112719,
    20260204120000,
    20260204120100,
    20260219143715,
    20260310061948,
    20260310135015,
    20260310135117,
    20260310135349,
    20260318095636,
    20260319023827,
    20260321034417,
    20260321070250,
    20260322121106,
    20260325000000,
    20260326000000,
    20260404144845,
    20260405120208,
    20260407000001,
    20260408000001,
    20260408000002,
    20260408000003,
    20260414000001,
    20260414000002,
    20260416000001,
    20260429000001,
    20260429000002,
    20260505000001,
    20260505000002,
    20260505000003,
    20260505000004,
    20260505000005,
    20260505000006,
    20260506000001,
    20260506000002,
    20260506000003,
    20260506000004,
    20260507000001,
    20260507000002,
    20260507000003,
    20260507000004,
    20260511000001,
    20260512000001,
    20260512000002,
    20260512000003,
    20260519000000,
    20260520000001,
    20260522000001,
    20260603000001,
    20260604000001,
    20260604000002,
    20260604000003,
    20260604000004,
    20260604000005,
    20260613000001,
    20260613000002,
    20260613000003,
    20260614000001,
    20260614000002,
    20260614000003,
    20260614000004,
    20260614000005,
    20260615000001,
    20260615000002,
    20260615000003,
    20260616233000,
    20260620000001,
    20260620000002,
    20260620000003,
    20260621000001,
    20260621000002,
    20260622000001,
    20260625000001,
    20260626000001,
    20260626000002,
    20260626000003,
    20260626000004,
    20260626000005,
    20260626000006,
    20260626000007,
    20260626000008,
    20260630000001,
    20260630000002,
    20260630000003,
    20260630000004,
    20260709000001,
    20260720000001,
    20260720000002,
    20260721000001,
    20260721000002,
    20260721000003,
    20260721000004,
    20260722000001,
    20260722000002,
    20260722000003,
    20260722000004,
    20260722000005,
    20260722000006,
    20260722000007,
    20260722000008,
    20260722000009,
    20260722000010,
    20260722000011,
    20260722000012,
    20260727000001,
    20260728000001,
    20260729000001,
    20260729000002,
    20260731000001,
    20260731000002,
    20260818000001,
];

/// A migration edited in place after release, proven comment-only by `semantic`.
struct Transition<'a> {
    version: i64,
    legacy: &'a str,
    current: &'a str,
    semantic: &'a str,
}

/// Empty since the squash: every recorded transition applied to a migration the
/// baseline folded in. The next in-place edit to a post-baseline migration adds
/// an entry here rather than reviving the mechanism.
const TRANSITIONS: &[Transition<'static>] = &[];

pub(super) async fn run(pool: &SqlitePool) -> Result<()> {
    adopt_baseline(pool).await?;
    reconcile(pool, TRANSITIONS).await?;
    MIGRATOR.run(pool).await?;
    Ok(())
}

async fn migrations_table_exists(pool: &SqlitePool) -> Result<bool> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations')",
    )
    .fetch_one(pool)
    .await?;
    Ok(exists == 1)
}

/// Replaces a complete pre-squash history with the single baseline row.
///
/// Checksum-blind by design: the rows being deleted record migrations that no
/// longer exist, so only their versions and success carry meaning. Any history
/// that is not exactly the folded set is refused, because stamping the baseline
/// onto a database missing one of them would claim a schema it does not have.
async fn adopt_baseline(pool: &SqlitePool) -> Result<bool> {
    validate_baseline()?;
    let Some(replacing) = plan_adoption(pool).await? else {
        return Ok(false);
    };
    apply_adoption(pool, replacing).await?;
    tracing::info!(
        folded = replacing,
        "Adopted the squashed migration baseline"
    );
    Ok(true)
}

/// Decides whether the recorded history is the one the baseline replaced.
///
/// Returns the number of rows adoption expects to remove, or `None` when there is
/// nothing to adopt. Every state that is not exactly the folded set is an error,
/// because stamping the baseline onto a database missing one of those migrations
/// would claim a schema it does not have.
async fn plan_adoption(pool: &SqlitePool) -> Result<Option<usize>> {
    if !migrations_table_exists(pool).await? {
        return Ok(None);
    }
    let rows = sqlx::query("SELECT version, success FROM _sqlx_migrations")
        .fetch_all(pool)
        .await?;
    if rows.is_empty() {
        return Ok(None);
    }

    let applied: BTreeSet<i64> = rows.iter().map(|row| row.get("version")).collect();
    if applied.contains(&BASELINE_VERSION) {
        return Ok(None);
    }

    let folded: BTreeSet<i64> = FOLDED_VERSIONS.iter().copied().collect();
    let missing: Vec<i64> = folded.difference(&applied).copied().collect();
    let unknown: Vec<i64> = applied.difference(&folded).copied().collect();
    if !missing.is_empty() || !unknown.is_empty() {
        return Err(ServiceError::Internal(format!(
            "this database predates the {BASELINE_VERSION} baseline but its migration history is \
             not the one the baseline replaced, so it cannot be adopted: {} of {} folded \
             migrations were never applied (first missing: {}), and {} unrecognised version(s) \
             are present (first: {}). Restore a backup taken before the interrupted upgrade.",
            missing.len(),
            FOLDED_VERSIONS.len(),
            missing
                .first()
                .map_or_else(|| "none".to_owned(), i64::to_string),
            unknown.len(),
            unknown
                .first()
                .map_or_else(|| "none".to_owned(), i64::to_string),
        )));
    }
    if let Some(row) = rows.iter().find(|row| !row.get::<bool, _>("success")) {
        return Err(ServiceError::Internal(format!(
            "migration {} is recorded as failed, so the pre-{BASELINE_VERSION} history is \
             incomplete and cannot be adopted into the baseline",
            row.get::<i64, _>("version"),
        )));
    }

    Ok(Some(rows.len()))
}

/// Swaps the planned history for the baseline row, or fails if the history moved
/// since it was planned. `replacing` is what the plan counted: a different number
/// here means a concurrent writer, and overwriting it would discard rows nothing
/// has inspected.
async fn apply_adoption(pool: &SqlitePool, replacing: usize) -> Result<()> {
    let baseline = MIGRATOR
        .iter()
        .find(|migration| migration.version == BASELINE_VERSION)
        .ok_or_else(|| {
            ServiceError::Internal(format!("baseline migration {BASELINE_VERSION} is missing"))
        })?;

    let mut tx = pool.begin().await?;
    let deleted = sqlx::query("DELETE FROM _sqlx_migrations")
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if deleted != replacing as u64 {
        return Err(ServiceError::Internal(format!(
            "baseline adoption raced: expected to replace {replacing} migration rows, found {deleted}"
        )));
    }

    let inserted = sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (?, ?, TRUE, ?, 0)",
    )
    .bind(BASELINE_VERSION)
    .bind(baseline.description.as_ref())
    .bind(baseline.checksum.as_ref())
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if inserted != 1 {
        return Err(ServiceError::Internal(
            "baseline adoption raced: the baseline row was not written".to_owned(),
        ));
    }

    tx.commit().await?;
    Ok(())
}

/// Rejects a baseline that no longer covers the versions it claims to have folded in.
fn validate_baseline() -> Result<()> {
    if !MIGRATOR
        .iter()
        .any(|migration| migration.version == BASELINE_VERSION)
    {
        return Err(ServiceError::Internal(format!(
            "baseline migration {BASELINE_VERSION} is missing from the embedded migrations"
        )));
    }
    let folded: BTreeSet<i64> = FOLDED_VERSIONS.iter().copied().collect();
    if folded.len() != FOLDED_VERSIONS.len() {
        return Err(ServiceError::Internal(
            "the folded version list contains a duplicate".to_owned(),
        ));
    }
    if let Some(migration) = MIGRATOR
        .iter()
        .find(|migration| folded.contains(&migration.version))
    {
        return Err(ServiceError::Internal(format!(
            "migration {} is both folded into the baseline and still present as a file",
            migration.version
        )));
    }
    if folded.last().is_some_and(|last| *last >= BASELINE_VERSION) {
        return Err(ServiceError::Internal(format!(
            "a folded version sorts at or after the baseline {BASELINE_VERSION}"
        )));
    }
    Ok(())
}

async fn reconcile(pool: &SqlitePool, transitions: &[Transition<'_>]) -> Result<u64> {
    validate_transitions(transitions)?;
    if !migrations_table_exists(pool).await? {
        return Ok(0);
    }

    let mut tx = pool.begin().await?;
    let rows = sqlx::query("SELECT version, checksum, success FROM _sqlx_migrations")
        .fetch_all(&mut *tx)
        .await?;
    let mut changed = 0;
    for transition in transitions {
        let Some(row) = rows
            .iter()
            .find(|row| row.get::<i64, _>("version") == transition.version)
        else {
            continue;
        };
        if !row.get::<bool, _>("success") {
            continue;
        }
        let legacy = decode(transition.legacy)?;
        if row.get::<Vec<u8>, _>("checksum") != legacy {
            continue;
        }
        let current = decode(transition.current)?;
        let result = sqlx::query(
            "UPDATE _sqlx_migrations SET checksum = ? WHERE version = ? AND checksum = ? AND success = TRUE",
        )
        .bind(&current)
        .bind(transition.version)
        .bind(&legacy)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() != 1 {
            return Err(ServiceError::Internal(format!(
                "migration checksum transition raced for version {}",
                transition.version
            )));
        }
        changed += 1;
    }
    tx.commit().await?;
    if changed > 0 {
        tracing::info!(changed, "Reconciled comment-only migration checksums");
    }
    Ok(changed)
}

fn validate_transitions(transitions: &[Transition<'_>]) -> Result<()> {
    for transition in transitions {
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == transition.version)
            .ok_or_else(|| {
                ServiceError::Internal(format!(
                    "migration checksum transition references missing version {}",
                    transition.version
                ))
            })?;
        if migration.checksum.as_ref() != decode(transition.current)? {
            return Err(ServiceError::Internal(format!(
                "migration {} changed after its checksum transition was recorded",
                transition.version
            )));
        }
        let semantic = hex::encode(Sha384::digest(normalize_sql(&migration.sql).as_bytes()));
        if semantic != transition.semantic {
            return Err(ServiceError::Internal(format!(
                "migration {} contains an executable change",
                transition.version
            )));
        }
    }
    Ok(())
}

fn decode(value: &str) -> Result<Vec<u8>> {
    hex::decode(value).map_err(|error| ServiceError::Internal(error.to_string()))
}

fn normalize_sql(source: &str) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Normal,
        LineComment,
        BlockComment,
        Single,
        Double,
        Bracket,
    }

    let chars: Vec<char> = source.chars().collect();
    let mut output = String::new();
    let mut state = State::Normal;
    let mut whitespace = false;
    let mut index = 0;
    while index < chars.len() {
        let current = chars[index];
        let next = chars.get(index + 1).copied();
        match state {
            State::Normal if current == '-' && next == Some('-') => {
                state = State::LineComment;
                whitespace = true;
                index += 2;
            }
            State::Normal if current == '/' && next == Some('*') => {
                state = State::BlockComment;
                whitespace = true;
                index += 2;
            }
            State::Normal if current.is_whitespace() => {
                whitespace = true;
                index += 1;
            }
            State::Normal => {
                if whitespace && !output.is_empty() && !output.ends_with(' ') {
                    output.push(' ');
                }
                whitespace = false;
                output.push(current);
                state = match current {
                    '\'' => State::Single,
                    '"' => State::Double,
                    '[' => State::Bracket,
                    _ => State::Normal,
                };
                index += 1;
            }
            State::LineComment => {
                if current == '\n' {
                    state = State::Normal;
                }
                index += 1;
            }
            State::BlockComment if current == '*' && next == Some('/') => {
                state = State::Normal;
                whitespace = true;
                index += 2;
            }
            State::BlockComment => index += 1,
            quoted => {
                let close = match quoted {
                    State::Single => '\'',
                    State::Double => '"',
                    State::Bracket => ']',
                    _ => unreachable!(),
                };
                output.push(current);
                if current == close && next == Some(close) {
                    output.push(close);
                    index += 2;
                } else {
                    if current == close {
                        state = State::Normal;
                    }
                    index += 1;
                }
            }
        }
    }
    output.trim().to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    async fn pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    /// A database as a pre-squash installation left it: the baseline's schema,
    /// because that is what applying all of the folded migrations produced, and a
    /// history of exactly those versions. The checksums are deliberately junk —
    /// adoption must not depend on them, since the migrations they record are gone.
    async fn pre_squash_pool() -> SqlitePool {
        let pool = pool().await;
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version <= BASELINE_VERSION)
        {
            sqlx::raw_sql(&migration.sql).execute(&pool).await.unwrap();
        }
        let mut conn = pool.acquire().await.unwrap();
        sqlx::migrate::Migrate::ensure_migrations_table(&mut *conn)
            .await
            .unwrap();
        drop(conn);
        for (index, version) in FOLDED_VERSIONS.iter().enumerate() {
            sqlx::query(
                "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
                 VALUES (?, 'legacy', TRUE, ?, 0)",
            )
            .bind(version)
            .bind(vec![u8::try_from(index % 256).unwrap(); 48])
            .execute(&pool)
            .await
            .unwrap();
        }
        pool
    }

    /// Every version a fully migrated database should record: the baseline plus
    /// each migration added after it. Derived from `MIGRATOR` so a new migration
    /// does not require editing these tests.
    fn expected_history() -> Vec<i64> {
        let mut versions: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
        versions.sort_unstable();
        versions
    }

    async fn history(pool: &SqlitePool) -> Vec<i64> {
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[test]
    fn baseline_manifest_matches_embedded_migrations() {
        validate_baseline().unwrap();
    }

    #[test]
    fn transition_manifest_matches_embedded_migrations() {
        validate_transitions(TRANSITIONS).unwrap();
    }

    #[test]
    fn normalization_ignores_comments_but_preserves_quoted_markers() {
        let left = "SELECT '--x', \"/*y*/\", [--z] FROM t; -- old";
        let right = "SELECT '--x', \"/*y*/\", [--z] FROM t; /* new */";
        assert_eq!(normalize_sql(left), normalize_sql(right));
        assert_ne!(normalize_sql("SELECT 'a'"), normalize_sql("SELECT 'b'"));
    }

    #[tokio::test]
    async fn a_complete_pre_squash_history_becomes_the_baseline_row() {
        let pool = pre_squash_pool().await;

        run(&pool).await.unwrap();

        assert_eq!(history(&pool).await, expected_history());
        let checksum: Vec<u8> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = ?")
                .bind(BASELINE_VERSION)
                .fetch_one(&pool)
                .await
                .unwrap();
        let baseline = MIGRATOR
            .iter()
            .find(|migration| migration.version == BASELINE_VERSION)
            .unwrap();
        assert_eq!(
            checksum,
            baseline.checksum.as_ref(),
            "the stamped row must carry the baseline's own checksum, or the next \
             startup reruns it"
        );
    }

    #[tokio::test]
    async fn adopting_an_already_adopted_database_changes_nothing() {
        let pool = pre_squash_pool().await;
        run(&pool).await.unwrap();

        assert!(!adopt_baseline(&pool).await.unwrap());
        run(&pool).await.unwrap();

        assert_eq!(history(&pool).await, expected_history());
    }

    #[tokio::test]
    async fn a_partially_applied_history_is_refused_without_touching_it() {
        let pool = pre_squash_pool().await;
        let dropped = FOLDED_VERSIONS[FOLDED_VERSIONS.len() - 2];
        sqlx::query("DELETE FROM _sqlx_migrations WHERE version = ?")
            .bind(dropped)
            .execute(&pool)
            .await
            .unwrap();

        let error = run(&pool).await.unwrap_err();

        let message = error.to_string();
        assert!(
            message.contains(&dropped.to_string()),
            "the error must name the migration that is missing, got: {message}"
        );
        assert_eq!(
            history(&pool).await.len(),
            FOLDED_VERSIONS.len() - 1,
            "a refused adoption must leave the history exactly as it found it"
        );
    }

    #[tokio::test]
    async fn an_unrecognised_version_is_refused() {
        let pool = pre_squash_pool().await;
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
             VALUES (20260101000000, 'stranger', TRUE, zeroblob(48), 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let error = run(&pool).await.unwrap_err();

        assert!(
            error.to_string().contains("20260101000000"),
            "the error must name the version it did not recognise, got: {error}"
        );
        assert_eq!(history(&pool).await.len(), FOLDED_VERSIONS.len() + 1);
    }

    #[tokio::test]
    async fn a_failed_migration_row_is_refused() {
        let pool = pre_squash_pool().await;
        let failed = FOLDED_VERSIONS[0];
        sqlx::query("UPDATE _sqlx_migrations SET success = FALSE WHERE version = ?")
            .bind(failed)
            .execute(&pool)
            .await
            .unwrap();

        let error = run(&pool).await.unwrap_err();

        assert!(
            error.to_string().contains(&failed.to_string()),
            "the error must name the migration recorded as failed, got: {error}"
        );
        assert_eq!(history(&pool).await.len(), FOLDED_VERSIONS.len());
    }

    #[tokio::test]
    async fn a_history_that_moves_after_planning_is_not_overwritten() {
        let pool = pre_squash_pool().await;
        let replacing = plan_adoption(&pool).await.unwrap().unwrap();

        sqlx::query("DELETE FROM _sqlx_migrations WHERE version = ?")
            .bind(FOLDED_VERSIONS[0])
            .execute(&pool)
            .await
            .unwrap();
        let error = apply_adoption(&pool, replacing).await.unwrap_err();

        assert!(
            error.to_string().contains("raced"),
            "a history that changed after planning must not be replaced, got: {error}"
        );
        assert_eq!(history(&pool).await.len(), FOLDED_VERSIONS.len() - 1);
    }

    #[tokio::test]
    async fn fresh_database_runs_only_the_baseline() {
        let pool = pool().await;
        assert!(!adopt_baseline(&pool).await.unwrap());

        run(&pool).await.unwrap();

        assert_eq!(history(&pool).await, expected_history());
    }

    #[tokio::test]
    async fn an_unknown_checksum_still_fails_closed() {
        let pool = pool().await;
        run(&pool).await.unwrap();
        sqlx::query("UPDATE _sqlx_migrations SET checksum = zeroblob(48) WHERE version = ?")
            .bind(BASELINE_VERSION)
            .execute(&pool)
            .await
            .unwrap();

        let error = run(&pool).await.unwrap_err();

        assert!(matches!(
            error,
            ServiceError::Migration(sqlx::migrate::MigrateError::VersionMismatch(found))
                if found == BASELINE_VERSION
        ));
    }

    /// The transition table is empty after the squash, so the mechanism is exercised
    /// against a synthetic entry for the baseline itself. Without this the next
    /// comment-only edit would be the first thing to test it.
    #[tokio::test]
    async fn a_legacy_checksum_is_reconciled_before_validation() {
        let baseline = MIGRATOR
            .iter()
            .find(|migration| migration.version == BASELINE_VERSION)
            .unwrap();
        let current = hex::encode(baseline.checksum.as_ref());
        let semantic = hex::encode(Sha384::digest(normalize_sql(&baseline.sql).as_bytes()));
        let transitions = &[Transition {
            version: BASELINE_VERSION,
            legacy: "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff\
                     00112233445566778899aabbccddeeff",
            current: &current,
            semantic: &semantic,
        }];

        let pool = pool().await;
        run(&pool).await.unwrap();
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
            .bind(decode(transitions[0].legacy).unwrap())
            .bind(BASELINE_VERSION)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(reconcile(&pool, transitions).await.unwrap(), 1);

        let checksum: Vec<u8> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = ?")
                .bind(BASELINE_VERSION)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(checksum, baseline.checksum.as_ref());
        MIGRATOR.run(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn a_transition_claiming_an_executable_change_is_rejected() {
        let baseline = MIGRATOR
            .iter()
            .find(|migration| migration.version == BASELINE_VERSION)
            .unwrap();
        let zeroes = hex::encode([0u8; 48]);
        let current = hex::encode(baseline.checksum.as_ref());
        let transitions = &[Transition {
            version: BASELINE_VERSION,
            legacy: &zeroes,
            current: &current,
            semantic: &zeroes,
        }];

        let pool = pool().await;
        let error = reconcile(&pool, transitions).await.unwrap_err();

        assert!(
            error.to_string().contains("executable change"),
            "a wrong semantic hash must be rejected, got: {error}"
        );
    }
}
