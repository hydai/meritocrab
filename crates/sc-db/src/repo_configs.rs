use crate::error::{DbError, DbResult};
use crate::models::{RepoConfig, RepoConfigRaw};
use chrono::{Duration, Utc};
use sqlx::{Any, Pool};

/// Upsert (insert or update) a repository configuration
pub async fn upsert_repo_config(
    pool: &Pool<Any>,
    owner: &str,
    repo: &str,
    config_json: &str,
    ttl: i64,
) -> DbResult<RepoConfig> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    // Try to update first
    let result = sqlx::query(
        "UPDATE repo_configs SET config_json = ?, cached_at = ?, ttl = ? WHERE owner = ? AND repo = ?"
    )
    .bind(config_json)
    .bind(&now_str)
    .bind(ttl)
    .bind(owner)
    .bind(repo)
    .execute(pool)
    .await?;

    if result.rows_affected() > 0 {
        // Updated successfully, fetch and return
        let config = sqlx::query_as::<_, RepoConfigRaw>(
            "SELECT id, owner, repo, config_json, cached_at, ttl FROM repo_configs WHERE owner = ? AND repo = ?"
        )
        .bind(owner)
        .bind(repo)
        .fetch_one(pool)
        .await?;

        return Ok(config.into());
    }

    // Insert if update didn't affect any rows
    sqlx::query(
        "INSERT INTO repo_configs (owner, repo, config_json, cached_at, ttl) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(owner)
    .bind(repo)
    .bind(config_json)
    .bind(&now_str)
    .bind(ttl)
    .execute(pool)
    .await?;

    // Fetch the created config to get the actual ID
    get_repo_config_raw(pool, owner, repo)
        .await?
        .ok_or_else(|| DbError::SqlxError(sqlx::Error::RowNotFound))
}

/// Get repository configuration with TTL check (returns None if expired)
pub async fn get_repo_config(
    pool: &Pool<Any>,
    owner: &str,
    repo: &str,
) -> DbResult<Option<RepoConfig>> {
    let config_raw = sqlx::query_as::<_, RepoConfigRaw>(
        "SELECT id, owner, repo, config_json, cached_at, ttl FROM repo_configs WHERE owner = ? AND repo = ?"
    )
    .bind(owner)
    .bind(repo)
    .fetch_optional(pool)
    .await?;

    if let Some(raw) = config_raw {
        let config: RepoConfig = raw.into();
        let now = Utc::now();
        let expiry = config.cached_at + Duration::seconds(config.ttl);

        if now <= expiry {
            return Ok(Some(config));
        }
        // Config expired, return None
        return Ok(None);
    }

    Ok(None)
}

/// Get repository configuration without TTL check (returns even if expired)
pub async fn get_repo_config_raw(
    pool: &Pool<Any>,
    owner: &str,
    repo: &str,
) -> DbResult<Option<RepoConfig>> {
    let config = sqlx::query_as::<_, RepoConfigRaw>(
        "SELECT id, owner, repo, config_json, cached_at, ttl FROM repo_configs WHERE owner = ? AND repo = ?"
    )
    .bind(owner)
    .bind(repo)
    .fetch_optional(pool)
    .await?
    .map(|raw| raw.into());

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::any::AnyPoolOptions;
    use std::thread;

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
    async fn test_upsert_repo_config_insert() {
        let pool = setup_test_db().await;

        let config = upsert_repo_config(&pool, "owner", "repo", r#"{"threshold": 50}"#, 3600)
            .await
            .expect("Failed to upsert config");

        assert_eq!(config.owner, "owner");
        assert_eq!(config.repo, "repo");
        assert_eq!(config.config_json, r#"{"threshold": 50}"#);
        assert_eq!(config.ttl, 3600);
    }

    #[tokio::test]
    async fn test_upsert_repo_config_update() {
        let pool = setup_test_db().await;

        // Insert initial config
        let config1 = upsert_repo_config(&pool, "owner", "repo", r#"{"threshold": 50}"#, 3600)
            .await
            .expect("Failed to upsert config");

        // Update with new config
        let config2 = upsert_repo_config(&pool, "owner", "repo", r#"{"threshold": 75}"#, 7200)
            .await
            .expect("Failed to upsert config");

        // Should have same ID but updated values
        assert_eq!(config1.id, config2.id);
        assert_eq!(config2.config_json, r#"{"threshold": 75}"#);
        assert_eq!(config2.ttl, 7200);
    }

    #[tokio::test]
    async fn test_get_repo_config_valid() {
        let pool = setup_test_db().await;

        // Insert config with 3600 second TTL
        upsert_repo_config(&pool, "owner", "repo", r#"{"threshold": 50}"#, 3600)
            .await
            .expect("Failed to upsert config");

        // Should return config (not expired)
        let config = get_repo_config(&pool, "owner", "repo")
            .await
            .expect("Failed to get config")
            .expect("Config not found");

        assert_eq!(config.config_json, r#"{"threshold": 50}"#);
    }

    #[tokio::test]
    async fn test_get_repo_config_expired() {
        let pool = setup_test_db().await;

        // Insert config with 0 second TTL (immediately expired)
        upsert_repo_config(&pool, "owner", "repo", r#"{"threshold": 50}"#, 0)
            .await
            .expect("Failed to upsert config");

        // Wait a tiny bit to ensure expiry
        thread::sleep(std::time::Duration::from_millis(10));

        // Should return None (expired)
        let config = get_repo_config(&pool, "owner", "repo")
            .await
            .expect("Failed to get config");

        assert!(config.is_none());
    }

    #[tokio::test]
    async fn test_get_repo_config_not_found() {
        let pool = setup_test_db().await;

        // Query non-existent config
        let config = get_repo_config(&pool, "owner", "repo")
            .await
            .expect("Failed to get config");

        assert!(config.is_none());
    }

    #[tokio::test]
    async fn test_get_repo_config_raw() {
        let pool = setup_test_db().await;

        // Insert config with 0 second TTL (immediately expired)
        upsert_repo_config(&pool, "owner", "repo", r#"{"threshold": 50}"#, 0)
            .await
            .expect("Failed to upsert config");

        // Wait a tiny bit to ensure expiry
        thread::sleep(std::time::Duration::from_millis(10));

        // get_repo_config should return None
        let config = get_repo_config(&pool, "owner", "repo")
            .await
            .expect("Failed to get config");
        assert!(config.is_none());

        // get_repo_config_raw should still return the config
        let config_raw = get_repo_config_raw(&pool, "owner", "repo")
            .await
            .expect("Failed to get config raw")
            .expect("Config not found");

        assert_eq!(config_raw.config_json, r#"{"threshold": 50}"#);
    }
}
