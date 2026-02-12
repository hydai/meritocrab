mod config;

use axum::{
    routing::{get, post},
    Router,
};
use config::AppConfig;
use sc_api::{handle_webhook, health, AppState};
use sc_db::run_migrations;
use sc_github::{GithubApiClient, GithubAppAuth, InstallationTokenManager, WebhookSecret};
use sc_llm::create_evaluator;
use sqlx::any::AnyPoolOptions;
use std::fs;
use tracing::{error, info};
use tracing_subscriber;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Install SQLite driver for sqlx::Any
    sqlx::any::install_default_drivers();

    // Load configuration
    let config = match AppConfig::load() {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };

    info!("Configuration loaded successfully");

    // Create database connection pool
    let db_pool = match AnyPoolOptions::new()
        .max_connections(config.database.max_connections)
        .connect(&config.database.url)
        .await
    {
        Ok(pool) => {
            info!("Database connection pool created: {}", config.database.url);
            pool
        }
        Err(e) => {
            error!("Failed to create database pool: {}", e);
            std::process::exit(1);
        }
    };

    // Run database migrations
    if let Err(e) = run_migrations(&db_pool).await {
        error!("Failed to run database migrations: {}", e);
        std::process::exit(1);
    }
    info!("Database migrations completed successfully");

    // Load GitHub App private key
    let private_key = match fs::read_to_string(&config.github.private_key_path) {
        Ok(key) => key,
        Err(e) => {
            error!(
                "Failed to read GitHub App private key from {}: {}",
                config.github.private_key_path, e
            );
            std::process::exit(1);
        }
    };

    // Create GitHub App authentication
    let github_auth = GithubAppAuth::new(
        config.github.app_id as i64,
        private_key,
    );

    // Create installation token manager
    let mut token_manager = InstallationTokenManager::new(github_auth);

    // Get installation token
    let token = match token_manager
        .get_token(config.github.installation_id as i64)
        .await
    {
        Ok(token) => token,
        Err(e) => {
            error!(
                "Failed to get GitHub installation token for installation {}: {}",
                config.github.installation_id, e
            );
            std::process::exit(1);
        }
    };
    info!(
        "GitHub installation token obtained for installation {}",
        config.github.installation_id
    );

    // Create GitHub API client
    let github_client = match GithubApiClient::new(token) {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to create GitHub API client: {}", e);
            std::process::exit(1);
        }
    };

    // Create webhook secret
    let webhook_secret = WebhookSecret::new(config.github.webhook_secret.clone());

    // Create LLM evaluator
    let llm_evaluator = match create_evaluator(&config.llm) {
        Ok(evaluator) => evaluator,
        Err(e) => {
            error!("Failed to create LLM evaluator: {}", e);
            std::process::exit(1);
        }
    };
    info!("LLM evaluator created successfully");

    // Create application state
    let app_state = AppState::new(
        db_pool,
        github_client,
        config.credit,
        webhook_secret,
        llm_evaluator,
        config.max_concurrent_llm_evals,
    );

    // Build Axum router
    let app = Router::<AppState>::new()
        .route("/health", get(health))
        .route("/webhooks/github", post(handle_webhook))
        .with_state(app_state);

    // Start server
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(e) => {
            error!("Failed to bind to {}: {}", addr, e);
            std::process::exit(1);
        }
    };

    info!("Server listening on http://{}", addr);

    // Run server
    if let Err(e) = axum::serve(listener, app).await {
        error!("Server error: {}", e);
        std::process::exit(1);
    }
}
