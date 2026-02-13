mod config;

use axum::{
    Router, middleware,
    routing::{get, post},
};
use config::AppConfig;
use meritocrab_api::{
    AppState, OAuthConfig, admin_handlers, auth_middleware, handle_webhook, health,
    init_server_start_time, oauth,
};
use meritocrab_db::run_migrations;
use meritocrab_github::{GithubApiClient, GithubAppAuth, InstallationTokenManager, WebhookSecret};
use meritocrab_llm::create_evaluator;
use sqlx::any::AnyPoolOptions;
use std::fs;
use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};
use tracing::{error, info};

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Initialize server start time for health endpoint
    init_server_start_time();

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
    let github_auth = GithubAppAuth::new(config.github.app_id as i64, private_key);

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

    // Create OAuth configuration
    let oauth_config = OAuthConfig {
        client_id: config.github.oauth_client_id.clone(),
        client_secret: config.github.oauth_client_secret.clone(),
        redirect_url: config.github.oauth_redirect_url.clone(),
    };

    // Create session store (using in-memory for simplicity)
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_expiry(Expiry::OnInactivity(time::Duration::hours(24)));

    // Create application state
    let app_state = AppState::new(
        db_pool,
        github_client,
        config.credit,
        webhook_secret,
        llm_evaluator,
        config.max_concurrent_llm_evals,
        oauth_config,
        300, // config cache TTL in seconds (5 minutes)
    );

    // Build admin API router (protected)
    let admin_routes = Router::new()
        .route(
            "/api/repos/:owner/:repo/evaluations",
            get(admin_handlers::list_evaluations),
        )
        .route(
            "/api/repos/:owner/:repo/evaluations/:id/approve",
            post(admin_handlers::approve_evaluation_handler),
        )
        .route(
            "/api/repos/:owner/:repo/evaluations/:id/override",
            post(admin_handlers::override_evaluation_handler),
        )
        .route(
            "/api/repos/:owner/:repo/contributors",
            get(admin_handlers::list_contributors),
        )
        .route(
            "/api/repos/:owner/:repo/contributors/:user_id/adjust",
            post(admin_handlers::adjust_contributor_credit),
        )
        .route(
            "/api/repos/:owner/:repo/contributors/:user_id/blacklist",
            post(admin_handlers::toggle_contributor_blacklist),
        )
        .route(
            "/api/repos/:owner/:repo/events",
            get(admin_handlers::list_credit_events),
        )
        .route_layer(middleware::from_fn_with_state(
            app_state.clone(),
            auth_middleware::require_maintainer,
        ));

    // Build Axum router
    let app = Router::new()
        .route("/health", get(health))
        .route("/webhooks/github", post(handle_webhook))
        .route("/auth/github", get(oauth::github_auth))
        .route("/auth/callback", get(oauth::github_callback))
        .route("/auth/logout", post(oauth::logout))
        .merge(admin_routes)
        .layer(session_layer)
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

    // Run server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Server error");

    info!("Server shutdown complete");
}

/// Wait for SIGTERM signal for graceful shutdown
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received SIGINT (Ctrl+C), initiating graceful shutdown...");
        },
        _ = terminate => {
            info!("Received SIGTERM, initiating graceful shutdown...");
        },
    }
}
