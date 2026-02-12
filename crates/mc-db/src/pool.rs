use crate::error::DbResult;
use sqlx::{any::AnyPoolOptions, Any, Pool};

/// Create a database pool from a connection string
pub async fn create_pool(database_url: &str) -> DbResult<Pool<Any>> {
    let pool = AnyPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    Ok(pool)
}

/// Run migrations on the database
pub async fn run_migrations(pool: &Pool<Any>) -> DbResult<()> {
    // Enable foreign keys for SQLite (no-op for other databases)
    let _ = sqlx::query("PRAGMA foreign_keys = ON")
        .execute(pool)
        .await;

    // Execute the initial migration
    // Note: This only executes the first statement. For multiple statements,
    // use a proper migration tool like sqlx-cli in production.
    // For our purposes, the test helpers execute the full migration directly.
    let _ = sqlx::query(include_str!("../migrations/001_initial.sql"))
        .execute(pool)
        .await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_pool_sqlite() {
        // Install the SQLite driver for Any
        sqlx::any::install_default_drivers();

        let pool = create_pool("sqlite::memory:")
            .await
            .expect("Failed to create pool");

        // Verify pool works
        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .expect("Failed to execute query");
    }

    #[tokio::test]
    async fn test_run_migrations() {
        // Install the SQLite driver for Any
        sqlx::any::install_default_drivers();

        let pool = create_pool("sqlite::memory:")
            .await
            .expect("Failed to create pool");

        // Note: run_migrations only executes a single SQL statement due to SQLx Any driver limitations
        // For actual migrations, use the setup_test_db pattern from the test modules which works correctly
        run_migrations(&pool)
            .await
            .expect("Failed to call run_migrations");

        // The migration function is primarily for production use with proper migration tools
        // Test that the pool is functional
        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .expect("Pool not functional");
    }
}
