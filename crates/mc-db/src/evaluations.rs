use crate::error::{DbError, DbResult};
use crate::models::{PendingEvaluation, PendingEvaluationRaw};
use chrono::Utc;
use mc_core::EvaluationStatus;
use sqlx::{Any, Pool};

/// Convert EvaluationStatus to string for database storage
fn status_to_string(status: &EvaluationStatus) -> &'static str {
    match status {
        EvaluationStatus::Pending => "pending",
        EvaluationStatus::Approved => "approved",
        EvaluationStatus::Overridden => "overridden",
        EvaluationStatus::AutoApplied => "auto_applied",
    }
}

/// Convert string from database to EvaluationStatus
fn string_to_status(s: &str) -> DbResult<EvaluationStatus> {
    match s {
        "pending" => Ok(EvaluationStatus::Pending),
        "approved" => Ok(EvaluationStatus::Approved),
        "overridden" => Ok(EvaluationStatus::Overridden),
        "auto_applied" => Ok(EvaluationStatus::AutoApplied),
        _ => Err(DbError::InvalidStatus(s.to_string())),
    }
}

/// Insert a new pending evaluation
pub async fn insert_evaluation(
    pool: &Pool<Any>,
    id: String,
    contributor_id: i64,
    repo_owner: &str,
    repo_name: &str,
    llm_classification: String,
    confidence: f64,
    proposed_delta: i32,
) -> DbResult<PendingEvaluation> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let status = status_to_string(&EvaluationStatus::Pending);

    sqlx::query(
        "INSERT INTO pending_evaluations (id, contributor_id, repo_owner, repo_name, llm_classification, confidence, proposed_delta, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(contributor_id)
    .bind(repo_owner)
    .bind(repo_name)
    .bind(&llm_classification)
    .bind(confidence)
    .bind(proposed_delta)
    .bind(status)
    .bind(&now_str)
    .bind(&now_str)
    .execute(pool)
    .await?;

    Ok(PendingEvaluation {
        id,
        contributor_id,
        repo_owner: repo_owner.to_string(),
        repo_name: repo_name.to_string(),
        llm_classification,
        confidence,
        proposed_delta,
        status: status.to_string(),
        maintainer_note: None,
        final_delta: None,
        created_at: now,
        updated_at: now,
    })
}

/// Get an evaluation by ID
pub async fn get_evaluation(
    pool: &Pool<Any>,
    id: &str,
) -> DbResult<Option<PendingEvaluation>> {
    let eval = sqlx::query_as::<_, PendingEvaluationRaw>(
        "SELECT id, contributor_id, repo_owner, repo_name, llm_classification, confidence, proposed_delta, status, maintainer_note, final_delta, created_at, updated_at
         FROM pending_evaluations
         WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .map(|raw| raw.into());

    Ok(eval)
}

/// List evaluations by repo and status with pagination
pub async fn list_evaluations_by_repo_and_status(
    pool: &Pool<Any>,
    repo_owner: &str,
    repo_name: &str,
    status: &EvaluationStatus,
    limit: i64,
    offset: i64,
) -> DbResult<Vec<PendingEvaluation>> {
    let status_str = status_to_string(status);

    let evals = sqlx::query_as::<_, PendingEvaluationRaw>(
        "SELECT id, contributor_id, repo_owner, repo_name, llm_classification, confidence, proposed_delta, status, maintainer_note, final_delta, created_at, updated_at
         FROM pending_evaluations
         WHERE repo_owner = ? AND repo_name = ? AND status = ?
         ORDER BY created_at DESC
         LIMIT ? OFFSET ?"
    )
    .bind(repo_owner)
    .bind(repo_name)
    .bind(status_str)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|raw| raw.into())
    .collect();

    Ok(evals)
}

/// Update evaluation status to approved
pub async fn approve_evaluation(
    pool: &Pool<Any>,
    id: &str,
    maintainer_note: Option<String>,
) -> DbResult<()> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let status = status_to_string(&EvaluationStatus::Approved);

    // First get the proposed_delta
    let eval = get_evaluation(pool, id).await?.ok_or_else(|| {
        DbError::EvaluationNotFound(id.to_string())
    })?;

    let result = sqlx::query(
        "UPDATE pending_evaluations SET status = ?, maintainer_note = ?, final_delta = ?, updated_at = ? WHERE id = ?"
    )
    .bind(status)
    .bind(maintainer_note)
    .bind(eval.proposed_delta)
    .bind(&now_str)
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(DbError::EvaluationNotFound(id.to_string()));
    }

    Ok(())
}

/// Update evaluation status to overridden with new delta
pub async fn override_evaluation(
    pool: &Pool<Any>,
    id: &str,
    new_delta: i32,
    maintainer_note: String,
) -> DbResult<()> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let status = status_to_string(&EvaluationStatus::Overridden);

    let result = sqlx::query(
        "UPDATE pending_evaluations SET status = ?, maintainer_note = ?, final_delta = ?, updated_at = ? WHERE id = ?"
    )
    .bind(status)
    .bind(maintainer_note)
    .bind(new_delta)
    .bind(&now_str)
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(DbError::EvaluationNotFound(id.to_string()));
    }

    Ok(())
}

/// Update evaluation status to auto-applied
pub async fn auto_apply_evaluation(
    pool: &Pool<Any>,
    id: &str,
) -> DbResult<()> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let status = status_to_string(&EvaluationStatus::AutoApplied);

    // First get the proposed_delta
    let eval = get_evaluation(pool, id).await?.ok_or_else(|| {
        DbError::EvaluationNotFound(id.to_string())
    })?;

    let result = sqlx::query(
        "UPDATE pending_evaluations SET status = ?, final_delta = ?, updated_at = ? WHERE id = ?"
    )
    .bind(status)
    .bind(eval.proposed_delta)
    .bind(&now_str)
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(DbError::EvaluationNotFound(id.to_string()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contributors::create_contributor;
    use sqlx::any::AnyPoolOptions;

    async fn setup_test_db() -> Pool<Any> {
        // Install the SQLite driver for Any
        sqlx::any::install_default_drivers();

        let pool = AnyPoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("Failed to create test database pool");

        // Enable foreign keys
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .expect("Failed to enable foreign keys");

        // Run migrations
        sqlx::query(include_str!("../migrations/001_initial.sql"))
            .execute(&pool)
            .await
            .expect("Failed to run migrations");

        pool
    }

    #[tokio::test]
    async fn test_insert_evaluation() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        let eval = insert_evaluation(
            &pool,
            "eval-123".to_string(),
            contributor.id,
            "owner",
            "repo",
            "high_quality".to_string(),
            0.95,
            15,
        )
        .await
        .expect("Failed to insert evaluation");

        assert_eq!(eval.id, "eval-123");
        assert_eq!(eval.contributor_id, contributor.id);
        assert_eq!(eval.llm_classification, "high_quality");
        assert_eq!(eval.confidence, 0.95);
        assert_eq!(eval.proposed_delta, 15);
        assert_eq!(eval.status, "pending");
        assert_eq!(eval.maintainer_note, None);
        assert_eq!(eval.final_delta, None);
    }

    #[tokio::test]
    async fn test_get_evaluation() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        insert_evaluation(
            &pool,
            "eval-123".to_string(),
            contributor.id,
            "owner",
            "repo",
            "high_quality".to_string(),
            0.95,
            15,
        )
        .await
        .expect("Failed to insert evaluation");

        let eval = get_evaluation(&pool, "eval-123")
            .await
            .expect("Failed to get evaluation")
            .expect("Evaluation not found");

        assert_eq!(eval.id, "eval-123");

        // Non-existent evaluation
        let result = get_evaluation(&pool, "nonexistent")
            .await
            .expect("Failed to query evaluation");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_evaluations_by_repo_and_status() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        // Insert multiple evaluations
        insert_evaluation(
            &pool,
            "eval-1".to_string(),
            contributor.id,
            "owner",
            "repo",
            "high_quality".to_string(),
            0.95,
            15,
        )
        .await
        .expect("Failed to insert evaluation");

        insert_evaluation(
            &pool,
            "eval-2".to_string(),
            contributor.id,
            "owner",
            "repo",
            "acceptable".to_string(),
            0.75,
            5,
        )
        .await
        .expect("Failed to insert evaluation");

        // List pending evaluations
        let evals = list_evaluations_by_repo_and_status(
            &pool,
            "owner",
            "repo",
            &EvaluationStatus::Pending,
            10,
            0,
        )
        .await
        .expect("Failed to list evaluations");

        assert_eq!(evals.len(), 2);
    }

    #[tokio::test]
    async fn test_approve_evaluation() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        insert_evaluation(
            &pool,
            "eval-123".to_string(),
            contributor.id,
            "owner",
            "repo",
            "high_quality".to_string(),
            0.95,
            15,
        )
        .await
        .expect("Failed to insert evaluation");

        approve_evaluation(&pool, "eval-123", Some("Looks good".to_string()))
            .await
            .expect("Failed to approve evaluation");

        let eval = get_evaluation(&pool, "eval-123")
            .await
            .expect("Failed to get evaluation")
            .expect("Evaluation not found");

        assert_eq!(eval.status, "approved");
        assert_eq!(eval.maintainer_note, Some("Looks good".to_string()));
        assert_eq!(eval.final_delta, Some(15));
    }

    #[tokio::test]
    async fn test_override_evaluation() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        insert_evaluation(
            &pool,
            "eval-123".to_string(),
            contributor.id,
            "owner",
            "repo",
            "acceptable".to_string(),
            0.75,
            5,
        )
        .await
        .expect("Failed to insert evaluation");

        override_evaluation(&pool, "eval-123", 10, "Bumping to high quality".to_string())
            .await
            .expect("Failed to override evaluation");

        let eval = get_evaluation(&pool, "eval-123")
            .await
            .expect("Failed to get evaluation")
            .expect("Evaluation not found");

        assert_eq!(eval.status, "overridden");
        assert_eq!(
            eval.maintainer_note,
            Some("Bumping to high quality".to_string())
        );
        assert_eq!(eval.final_delta, Some(10));
    }

    #[tokio::test]
    async fn test_auto_apply_evaluation() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        insert_evaluation(
            &pool,
            "eval-123".to_string(),
            contributor.id,
            "owner",
            "repo",
            "high_quality".to_string(),
            0.95,
            15,
        )
        .await
        .expect("Failed to insert evaluation");

        auto_apply_evaluation(&pool, "eval-123")
            .await
            .expect("Failed to auto-apply evaluation");

        let eval = get_evaluation(&pool, "eval-123")
            .await
            .expect("Failed to get evaluation")
            .expect("Evaluation not found");

        assert_eq!(eval.status, "auto_applied");
        assert_eq!(eval.final_delta, Some(15));
    }
}
