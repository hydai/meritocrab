use axum::extract::FromRef;
use sc_core::RepoConfig;
use sc_github::{GithubApiClient, WebhookSecret};
use sqlx::{Any, Pool};
use std::sync::Arc;

/// Application state for Axum dependency injection
///
/// This is the DI root that contains all shared resources needed by handlers:
/// - Database connection pool
/// - GitHub API client
/// - Repository configuration
/// - Webhook secret for HMAC verification
#[derive(Clone)]
pub struct AppState {
    /// Database connection pool
    pub db_pool: Pool<Any>,

    /// GitHub API client for operations like closing PRs
    pub github_client: Arc<GithubApiClient>,

    /// Repository credit configuration
    pub repo_config: RepoConfig,

    /// Webhook secret for HMAC verification
    pub webhook_secret: WebhookSecret,
}

impl AppState {
    /// Create new application state
    pub fn new(
        db_pool: Pool<Any>,
        github_client: GithubApiClient,
        repo_config: RepoConfig,
        webhook_secret: WebhookSecret,
    ) -> Self {
        Self {
            db_pool,
            github_client: Arc::new(github_client),
            repo_config,
            webhook_secret,
        }
    }
}

/// Implement FromRef to allow VerifiedWebhook extractor to access WebhookSecret
impl FromRef<AppState> for WebhookSecret {
    fn from_ref(state: &AppState) -> Self {
        state.webhook_secret.clone()
    }
}
