use crate::error::{DbError, DbResult};
use crate::models::{Contributor, ContributorRaw};
use chrono::Utc;
use sqlx::{Any, Pool};

/// Create a new contributor with default credit score
pub async fn create_contributor(
    pool: &Pool<Any>,
    github_user_id: i64,
    repo_owner: &str,
    repo_name: &str,
    starting_credit: i32,
) -> DbResult<Contributor> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    sqlx::query(
        "INSERT INTO contributors (github_user_id, repo_owner, repo_name, credit_score, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(github_user_id)
    .bind(repo_owner)
    .bind(repo_name)
    .bind(starting_credit)
    .bind(&now_str)
    .bind(&now_str)
    .execute(pool)
    .await?;

    // Fetch the created contributor to get the actual ID
    get_contributor(pool, github_user_id, repo_owner, repo_name)
        .await?
        .ok_or_else(|| DbError::SqlxError(sqlx::Error::RowNotFound))
}

/// Get contributor by github_user_id and repo
pub async fn get_contributor(
    pool: &Pool<Any>,
    github_user_id: i64,
    repo_owner: &str,
    repo_name: &str,
) -> DbResult<Option<Contributor>> {
    let contributor = sqlx::query_as::<_, ContributorRaw>(
        "SELECT id, github_user_id, repo_owner, repo_name, credit_score, role, is_blacklisted, created_at, updated_at
         FROM contributors
         WHERE github_user_id = ? AND repo_owner = ? AND repo_name = ?"
    )
    .bind(github_user_id)
    .bind(repo_owner)
    .bind(repo_name)
    .fetch_optional(pool)
    .await?
    .map(|raw| raw.into());

    Ok(contributor)
}

/// Lookup or create contributor atomically
pub async fn lookup_or_create_contributor(
    pool: &Pool<Any>,
    github_user_id: i64,
    repo_owner: &str,
    repo_name: &str,
    starting_credit: i32,
) -> DbResult<Contributor> {
    // Try to get existing contributor
    if let Some(contributor) = get_contributor(pool, github_user_id, repo_owner, repo_name).await? {
        return Ok(contributor);
    }

    // Create new contributor if not found
    // Note: There's a potential race condition here in concurrent scenarios
    // SQLite will handle the UNIQUE constraint and return an error if another
    // transaction created the same contributor. We catch that and retry the lookup.
    match create_contributor(pool, github_user_id, repo_owner, repo_name, starting_credit).await {
        Ok(contributor) => Ok(contributor),
        Err(DbError::SqlxError(sqlx::Error::Database(db_err))) => {
            // Check if this is a UNIQUE constraint violation
            if db_err.message().contains("UNIQUE") {
                // Another transaction created it, retry lookup
                get_contributor(pool, github_user_id, repo_owner, repo_name)
                    .await?
                    .ok_or_else(|| {
                        DbError::ContributorNotFound(
                            github_user_id,
                            repo_owner.to_string(),
                            repo_name.to_string(),
                        )
                    })
            } else {
                Err(DbError::SqlxError(sqlx::Error::Database(db_err)))
            }
        }
        Err(e) => Err(e),
    }
}

/// Update contributor credit score
pub async fn update_credit_score(
    pool: &Pool<Any>,
    contributor_id: i64,
    new_score: i32,
) -> DbResult<()> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    let result =
        sqlx::query("UPDATE contributors SET credit_score = ?, updated_at = ? WHERE id = ?")
            .bind(new_score)
            .bind(&now_str)
            .bind(contributor_id)
            .execute(pool)
            .await?;

    if result.rows_affected() == 0 {
        return Err(DbError::SqlxError(sqlx::Error::RowNotFound));
    }

    Ok(())
}

/// Update contributor role
pub async fn update_role(
    pool: &Pool<Any>,
    contributor_id: i64,
    role: Option<String>,
) -> DbResult<()> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    let result = sqlx::query("UPDATE contributors SET role = ?, updated_at = ? WHERE id = ?")
        .bind(role)
        .bind(&now_str)
        .bind(contributor_id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(DbError::SqlxError(sqlx::Error::RowNotFound));
    }

    Ok(())
}

/// Set contributor blacklist status
pub async fn set_blacklisted(
    pool: &Pool<Any>,
    contributor_id: i64,
    is_blacklisted: bool,
) -> DbResult<()> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let is_blacklisted_int = if is_blacklisted { 1 } else { 0 };

    let result =
        sqlx::query("UPDATE contributors SET is_blacklisted = ?, updated_at = ? WHERE id = ?")
            .bind(is_blacklisted_int)
            .bind(&now_str)
            .bind(contributor_id)
            .execute(pool)
            .await?;

    if result.rows_affected() == 0 {
        return Err(DbError::SqlxError(sqlx::Error::RowNotFound));
    }

    Ok(())
}

/// List contributors by repo with pagination
pub async fn list_contributors_by_repo(
    pool: &Pool<Any>,
    repo_owner: &str,
    repo_name: &str,
    limit: i64,
    offset: i64,
) -> DbResult<Vec<Contributor>> {
    let contributors = sqlx::query_as::<_, ContributorRaw>(
        "SELECT id, github_user_id, repo_owner, repo_name, credit_score, role, is_blacklisted, created_at, updated_at
         FROM contributors
         WHERE repo_owner = ? AND repo_name = ?
         ORDER BY credit_score DESC, updated_at DESC
         LIMIT ? OFFSET ?"
    )
    .bind(repo_owner)
    .bind(repo_name)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|raw| raw.into())
    .collect();

    Ok(contributors)
}

/// Count total contributors for a repo
pub async fn count_contributors_by_repo(
    pool: &Pool<Any>,
    repo_owner: &str,
    repo_name: &str,
) -> DbResult<i64> {
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM contributors WHERE repo_owner = ? AND repo_name = ?")
            .bind(repo_owner)
            .bind(repo_name)
            .fetch_one(pool)
            .await?;

    Ok(count.0)
}

/// Get contributor by ID
pub async fn get_contributor_by_id(
    pool: &Pool<Any>,
    contributor_id: i64,
) -> DbResult<Option<Contributor>> {
    let contributor = sqlx::query_as::<_, ContributorRaw>(
        "SELECT id, github_user_id, repo_owner, repo_name, credit_score, role, is_blacklisted, created_at, updated_at
         FROM contributors
         WHERE id = ?"
    )
    .bind(contributor_id)
    .fetch_optional(pool)
    .await?
    .map(|raw| raw.into());

    Ok(contributor)
}

#[cfg(test)]
mod tests {
    use super::*;
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
    async fn test_create_contributor() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        assert_eq!(contributor.github_user_id, 12345);
        assert_eq!(contributor.repo_owner, "owner");
        assert_eq!(contributor.repo_name, "repo");
        assert_eq!(contributor.credit_score, 100);
        assert_eq!(contributor.role, None);
        assert!(!contributor.is_blacklisted);
    }

    #[tokio::test]
    async fn test_get_contributor() {
        let pool = setup_test_db().await;

        // Create a contributor
        create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        // Get the contributor
        let contributor = get_contributor(&pool, 12345, "owner", "repo")
            .await
            .expect("Failed to get contributor")
            .expect("Contributor not found");

        assert_eq!(contributor.github_user_id, 12345);
        assert_eq!(contributor.credit_score, 100);

        // Try to get non-existent contributor
        let result = get_contributor(&pool, 99999, "owner", "repo")
            .await
            .expect("Failed to query contributor");

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_lookup_or_create_contributor() {
        let pool = setup_test_db().await;

        // First call should create
        let contributor1 = lookup_or_create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to lookup or create contributor");

        assert_eq!(contributor1.github_user_id, 12345);
        assert_eq!(contributor1.credit_score, 100);

        // Second call should return existing
        let contributor2 = lookup_or_create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to lookup or create contributor");

        assert_eq!(contributor1.id, contributor2.id);
        assert_eq!(contributor2.credit_score, 100);
    }

    #[tokio::test]
    async fn test_update_credit_score() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        update_credit_score(&pool, contributor.id, 75)
            .await
            .expect("Failed to update credit score");

        let updated = get_contributor(&pool, 12345, "owner", "repo")
            .await
            .expect("Failed to get contributor")
            .expect("Contributor not found");

        assert_eq!(updated.credit_score, 75);
    }

    #[tokio::test]
    async fn test_update_role() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        update_role(&pool, contributor.id, Some("maintainer".to_string()))
            .await
            .expect("Failed to update role");

        let updated = get_contributor(&pool, 12345, "owner", "repo")
            .await
            .expect("Failed to get contributor")
            .expect("Contributor not found");

        assert_eq!(updated.role, Some("maintainer".to_string()));
    }

    #[tokio::test]
    async fn test_set_blacklisted() {
        let pool = setup_test_db().await;

        let contributor = create_contributor(&pool, 12345, "owner", "repo", 100)
            .await
            .expect("Failed to create contributor");

        set_blacklisted(&pool, contributor.id, true)
            .await
            .expect("Failed to set blacklist status");

        let updated = get_contributor(&pool, 12345, "owner", "repo")
            .await
            .expect("Failed to get contributor")
            .expect("Contributor not found");

        assert!(updated.is_blacklisted);
    }
}
