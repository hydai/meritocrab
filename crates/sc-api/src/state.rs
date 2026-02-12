use axum::extract::FromRef;
use sc_core::RepoConfig;
use sc_github::{GithubApiClient, WebhookSecret};
use sc_llm::LlmEvaluator;
use serde::{Deserialize, Serialize};
use sqlx::{Any, Pool};
use std::sync::Arc;
use tokio::sync::Semaphore;

/// OAuth configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: String,
}

/// Application state for Axum dependency injection
///
/// This is the DI root that contains all shared resources needed by handlers:
/// - Database connection pool
/// - GitHub API client
/// - Repository configuration
/// - Webhook secret for HMAC verification
/// - LLM evaluator for content quality assessment
/// - Semaphore for limiting concurrent LLM evaluations
/// - OAuth configuration for admin authentication
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

    /// LLM evaluator for content quality assessment
    pub llm_evaluator: Arc<dyn LlmEvaluator>,

    /// Semaphore for limiting concurrent LLM evaluations
    pub llm_semaphore: Arc<Semaphore>,

    /// OAuth configuration for admin authentication
    pub oauth_config: OAuthConfig,
}

impl AppState {
    /// Create new application state
    pub fn new(
        db_pool: Pool<Any>,
        github_client: GithubApiClient,
        repo_config: RepoConfig,
        webhook_secret: WebhookSecret,
        llm_evaluator: Arc<dyn LlmEvaluator>,
        max_concurrent_llm_evals: usize,
        oauth_config: OAuthConfig,
    ) -> Self {
        Self {
            db_pool,
            github_client: Arc::new(github_client),
            repo_config,
            webhook_secret,
            llm_evaluator,
            llm_semaphore: Arc::new(Semaphore::new(max_concurrent_llm_evals)),
            oauth_config,
        }
    }
}

/// Implement FromRef to allow VerifiedWebhook extractor to access WebhookSecret
impl FromRef<AppState> for WebhookSecret {
    fn from_ref(state: &AppState) -> Self {
        state.webhook_secret.clone()
    }
}

/// Implement FromRef to allow OAuth handlers to access OAuthConfig
impl FromRef<AppState> for OAuthConfig {
    fn from_ref(state: &AppState) -> Self {
        state.oauth_config.clone()
    }
}

/// Implement FromRef to allow auth middleware to access GithubApiClient
impl FromRef<AppState> for Arc<GithubApiClient> {
    fn from_ref(state: &AppState) -> Self {
        state.github_client.clone()
    }
}
