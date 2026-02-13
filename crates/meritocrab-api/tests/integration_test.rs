/// Integration tests for webhook handler with full flow
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hmac::{Hmac, Mac};
use meritocrab_api::{AppState, OAuthConfig, handle_webhook, health};
use meritocrab_core::RepoConfig;
use meritocrab_db::contributors::get_contributor;
use meritocrab_github::{GithubApiClient, WebhookSecret};
use serde_json::json;
use sha2::Sha256;
use sqlx::any::AnyPoolOptions;
use std::sync::Arc;
use tower::ServiceExt;

type HmacSha256 = Hmac<Sha256>;

fn test_oauth_config() -> OAuthConfig {
    OAuthConfig {
        client_id: "test-client-id".to_string(),
        client_secret: "test-client-secret".to_string(),
        redirect_url: "http://localhost:8080/auth/callback".to_string(),
    }
}

async fn setup_test_state() -> AppState {
    // Install SQLite driver
    sqlx::any::install_default_drivers();

    // Create in-memory database
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
    sqlx::query(include_str!(
        "../../meritocrab-db/migrations/001_initial.sql"
    ))
    .execute(&pool)
    .await
    .expect("Failed to run migrations");

    // Initialize rustls for GitHub client
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Create mock GitHub client
    let github_client =
        GithubApiClient::new("test-token".to_string()).expect("Failed to create GitHub client");

    // Create mock LLM evaluator
    let llm_evaluator = Arc::new(meritocrab_llm::MockEvaluator::new());

    let webhook_secret = WebhookSecret::new("test-secret".to_string());
    let repo_config = RepoConfig::default();

    AppState::new(
        pool,
        github_client,
        repo_config,
        webhook_secret,
        llm_evaluator,
        10,
        test_oauth_config(),
        300,
    )
}

fn compute_signature(body: &[u8], secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    let result = mac.finalize();
    format!("sha256={}", hex::encode(result.into_bytes()))
}

fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/health", axum::routing::get(health))
        .route("/webhooks/github", axum::routing::post(handle_webhook))
        .with_state(state)
}

#[tokio::test]
async fn test_health_endpoint() {
    let state = setup_test_state().await;
    let app = create_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "healthy");
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn test_webhook_invalid_signature() {
    let state = setup_test_state().await;
    let app = create_app(state);

    let payload = json!({
        "action": "opened",
        "number": 1,
        "pull_request": {
            "number": 1,
            "title": "Test PR",
            "user": {
                "id": 12345,
                "login": "testuser"
            },
            "state": "open",
            "html_url": "https://github.com/owner/repo/pull/1"
        },
        "repository": {
            "id": 1,
            "name": "repo",
            "full_name": "owner/repo",
            "owner": {
                "id": 1,
                "login": "owner"
            }
        },
        "sender": {
            "id": 12345,
            "login": "testuser"
        }
    });

    let body = serde_json::to_vec(&payload).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/github")
                .header("Content-Type", "application/json")
                .header("X-Hub-Signature-256", "sha256=invalid")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_webhook_pr_opened_new_contributor() {
    let state = setup_test_state().await;
    let db_pool = state.db_pool.clone();

    let app = create_app(state);

    let payload = json!({
        "action": "opened",
        "number": 1,
        "pull_request": {
            "number": 1,
            "title": "Test PR",
            "body": "Test body",
            "user": {
                "id": 12345,
                "login": "testuser"
            },
            "state": "open",
            "merged": false,
            "html_url": "https://github.com/owner/repo/pull/1"
        },
        "repository": {
            "id": 1,
            "name": "repo",
            "full_name": "owner/repo",
            "owner": {
                "id": 1,
                "login": "owner"
            }
        },
        "sender": {
            "id": 12345,
            "login": "testuser"
        }
    });

    let body = serde_json::to_vec(&payload).unwrap();
    let signature = compute_signature(&body, "test-secret");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/github")
                .header("Content-Type", "application/json")
                .header("X-Hub-Signature-256", signature)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    if status != StatusCode::OK {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        eprintln!("Error response: {}", String::from_utf8_lossy(&body));
        panic!("Expected 200 OK, got {}", status);
    }
    assert_eq!(status, StatusCode::OK);

    // Verify contributor was created with default credit (100)
    let contributor = get_contributor(&db_pool, 12345, "owner", "repo")
        .await
        .unwrap()
        .expect("Contributor should exist");

    assert_eq!(contributor.github_user_id, 12345);
    assert_eq!(contributor.credit_score, 100); // Default starting credit
    assert_eq!(contributor.repo_owner, "owner");
    assert_eq!(contributor.repo_name, "repo");
}

#[tokio::test]
async fn test_webhook_pr_opened_not_processed() {
    let state = setup_test_state().await;
    let app = create_app(state);

    // Test that non-PR events return 200 but don't process
    let payload = json!({
        "action": "created",
        "comment": {
            "id": 1,
            "body": "Test comment",
            "user": {
                "id": 12345,
                "login": "testuser"
            },
            "html_url": "https://github.com/owner/repo/issues/1#issuecomment-1"
        },
        "issue": {
            "number": 1,
            "title": "Test Issue",
            "user": {
                "id": 12345,
                "login": "testuser"
            }
        },
        "repository": {
            "id": 1,
            "name": "repo",
            "full_name": "owner/repo",
            "owner": {
                "id": 1,
                "login": "owner"
            }
        },
        "sender": {
            "id": 12345,
            "login": "testuser"
        }
    });

    let body = serde_json::to_vec(&payload).unwrap();
    let signature = compute_signature(&body, "test-secret");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/github")
                .header("Content-Type", "application/json")
                .header("X-Hub-Signature-256", signature)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
