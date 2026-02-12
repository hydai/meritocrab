use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::state::AppState;

/// Server start time (shared across all health checks)
static mut SERVER_START_TIME: Option<Instant> = None;

/// Initialize server start time
pub fn init_server_start_time() {
    unsafe {
        SERVER_START_TIME = Some(Instant::now());
    }
}

/// Get server uptime in seconds
fn get_uptime_seconds() -> u64 {
    unsafe {
        SERVER_START_TIME
            .map(|start| start.elapsed().as_secs())
            .unwrap_or(0)
    }
}

/// Health check response
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
    pub database: DatabaseStatus,
    pub llm_provider: LlmProviderStatus,
}

/// Database connectivity status
#[derive(Debug, Serialize, Deserialize)]
pub struct DatabaseStatus {
    pub connected: bool,
    pub driver: String,
}

/// LLM provider status
#[derive(Debug, Serialize, Deserialize)]
pub struct LlmProviderStatus {
    pub provider: String,
    pub available: bool,
}

/// Health check endpoint
///
/// Returns 200 OK with comprehensive server info
pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    // Check database connectivity
    let db_status = check_database_status(&state).await;

    // Check LLM provider status
    let llm_status = check_llm_status(&state);

    let response = HealthResponse {
        status: if db_status.connected && llm_status.available {
            "healthy".to_string()
        } else {
            "degraded".to_string()
        },
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: get_uptime_seconds(),
        database: db_status,
        llm_provider: llm_status,
    };

    (StatusCode::OK, Json(response))
}

/// Check database connectivity
async fn check_database_status(state: &AppState) -> DatabaseStatus {
    // Try a simple query to verify database is accessible
    let connected = sqlx::query("SELECT 1")
        .execute(&state.db_pool)
        .await
        .is_ok();

    // Simplified driver detection - we use sqlx::Any which abstracts the driver
    // In production this will typically be PostgreSQL, in dev it's SQLite
    DatabaseStatus {
        connected,
        driver: "any".to_string(), // sqlx::Any abstracts the actual driver
    }
}

/// Check LLM provider status
fn check_llm_status(state: &AppState) -> LlmProviderStatus {
    // For now, we assume if the evaluator exists, it's available
    // In production, you might want to do a health check API call
    LlmProviderStatus {
        provider: state.llm_evaluator.provider_name(),
        available: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use mc_core::RepoConfig;
    use mc_github::{GithubApiClient, GithubAppAuth, InstallationTokenManager, WebhookSecret};
    use mc_llm::MockEvaluator;
    use sqlx::any::AnyPoolOptions;
    use std::sync::Arc;
    use crate::OAuthConfig;

    #[tokio::test]
    async fn test_health_endpoint() {
        // Initialize server start time
        init_server_start_time();

        // Install SQLite driver for sqlx::Any
        sqlx::any::install_default_drivers();

        // Create test database
        let db_pool = AnyPoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("Failed to create test database");

        // Create test GitHub client
        let github_auth = GithubAppAuth::new(123456, "fake-private-key".to_string());
        let mut token_manager = InstallationTokenManager::new(github_auth);
        // Note: This will fail but we won't use GitHub in health check
        let token = token_manager.get_token(123456).await.unwrap_or_default();
        let github_client = GithubApiClient::new(token).expect("Failed to create GitHub client");

        // Create test state
        let app_state = AppState::new(
            db_pool,
            github_client,
            RepoConfig::default(),
            WebhookSecret::new("test-secret".to_string()),
            Arc::new(MockEvaluator::new()),
            5,
            OAuthConfig {
                client_id: "test".to_string(),
                client_secret: "test".to_string(),
                redirect_url: "http://localhost/callback".to_string(),
            },
            300,
        );

        let response = health(State(app_state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
