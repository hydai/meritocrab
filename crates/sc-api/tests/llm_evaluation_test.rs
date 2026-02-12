/// Integration tests for LLM evaluation in webhooks
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use hmac::{Hmac, Mac};
use sc_api::{handle_webhook, health, AppState};
use sc_core::{QualityLevel, RepoConfig};
use sc_db::{contributors::get_contributor, credit_events::list_events_by_contributor, evaluations::list_evaluations_by_repo_and_status};
use sc_github::{GithubApiClient, WebhookSecret};
use sc_llm::MockEvaluator;
use serde_json::json;
use sha2::Sha256;
use sqlx::any::AnyPoolOptions;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tower::ServiceExt;

type HmacSha256 = Hmac<Sha256>;

async fn setup_test_state_with_evaluator(evaluator: MockEvaluator) -> AppState {
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
    sqlx::query(include_str!("../../sc-db/migrations/001_initial.sql"))
        .execute(&pool)
        .await
        .expect("Failed to run migrations");

    // Initialize rustls for GitHub client
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Create mock GitHub client
    let github_client = GithubApiClient::new("test-token".to_string())
        .expect("Failed to create GitHub client");

    let webhook_secret = WebhookSecret::new("test-secret".to_string());
    let repo_config = RepoConfig::default();

    AppState::new(
        pool,
        github_client,
        repo_config,
        webhook_secret,
        Arc::new(evaluator),
        10,
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
async fn test_pr_opened_high_confidence_applies_credit() {
    // Mock evaluator that returns high quality with high confidence
    let evaluator = MockEvaluator::with_default(QualityLevel::High);
    let state = setup_test_state_with_evaluator(evaluator).await;
    let db_pool = state.db_pool.clone();

    let app = create_app(state);

    let payload = json!({
        "action": "opened",
        "number": 1,
        "pull_request": {
            "number": 1,
            "title": "Implements comprehensive feature with tests",
            "body": "This is a high quality PR with comprehensive implementation",
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

    assert_eq!(response.status(), StatusCode::OK);

    // Wait for async LLM evaluation to complete
    sleep(Duration::from_millis(100)).await;

    // Verify contributor credit was updated
    let contributor = get_contributor(&db_pool, 12345, "owner", "repo")
        .await
        .unwrap()
        .expect("Contributor should exist");

    // Starting credit (100) + high quality PR delta (+15) = 115
    assert_eq!(contributor.credit_score, 115);

    // Verify credit event was logged with LLM evaluation JSON
    let events = list_events_by_contributor(&db_pool, contributor.id, 10, 0)
        .await
        .unwrap();

    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.event_type, "pr_opened");
    assert_eq!(event.delta, 15);
    assert_eq!(event.credit_before, 100);
    assert_eq!(event.credit_after, 115);
    assert!(event.llm_evaluation.is_some());

    // Verify LLM evaluation JSON contains expected fields
    let llm_eval: serde_json::Value = serde_json::from_str(&event.llm_evaluation.as_ref().unwrap()).unwrap();
    // Check the classification - serde_json represents enums as strings
    assert!(llm_eval["classification"].is_string());
    assert!(llm_eval["confidence"].as_f64().unwrap() >= 0.85);
}

#[tokio::test]
async fn test_pr_opened_low_confidence_creates_pending_evaluation() {
    // Mock evaluator with low confidence (below 0.85 threshold)
    use sc_core::config::QualityLevel;
    use sc_llm::{Evaluation, EvalContext, LlmEvaluator};

    // Create a custom mock that returns low confidence
    #[derive(Debug)]
    struct LowConfidenceMock;

    #[async_trait::async_trait]
    impl LlmEvaluator for LowConfidenceMock {
        async fn evaluate(&self, _content: &str, _context: &EvalContext) -> Result<sc_llm::Evaluation, sc_llm::LlmError> {
            Ok(Evaluation::new(
                QualityLevel::Acceptable,
                0.75, // Below 0.85 threshold
                "Uncertain evaluation".to_string(),
            ))
        }
    }

    let state = {
        sqlx::any::install_default_drivers();
        let pool = AnyPoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await.unwrap();
        sqlx::query(include_str!("../../sc-db/migrations/001_initial.sql"))
            .execute(&pool)
            .await
            .unwrap();
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let github_client = GithubApiClient::new("test-token".to_string()).unwrap();
        let webhook_secret = WebhookSecret::new("test-secret".to_string());
        let repo_config = RepoConfig::default();
        AppState::new(
            pool,
            github_client,
            repo_config,
            webhook_secret,
            Arc::new(LowConfidenceMock),
            10,
        )
    };

    let db_pool = state.db_pool.clone();
    let app = create_app(state);

    let payload = json!({
        "action": "opened",
        "number": 1,
        "pull_request": {
            "number": 1,
            "title": "Uncertain change",
            "body": "This might be good or not",
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

    assert_eq!(response.status(), StatusCode::OK);

    // Wait for async LLM evaluation
    sleep(Duration::from_millis(100)).await;

    // Verify contributor credit was NOT updated (still at starting value)
    let contributor = get_contributor(&db_pool, 12345, "owner", "repo")
        .await
        .unwrap()
        .expect("Contributor should exist");

    assert_eq!(contributor.credit_score, 100); // Starting credit unchanged

    // Verify pending evaluation was created
    let pending = list_evaluations_by_repo_and_status(
        &db_pool,
        "owner",
        "repo",
        &sc_core::EvaluationStatus::Pending,
        10,
        0,
    )
    .await
    .unwrap();

    assert_eq!(pending.len(), 1);
    let eval = &pending[0];
    assert_eq!(eval.llm_classification, "Acceptable");
    assert_eq!(eval.confidence, 0.75);
    assert_eq!(eval.proposed_delta, 5); // Acceptable PR = +5
}

#[tokio::test]
async fn test_comment_created_applies_credit() {
    let evaluator = MockEvaluator::with_default(QualityLevel::High);
    let state = setup_test_state_with_evaluator(evaluator).await;
    let db_pool = state.db_pool.clone();

    let app = create_app(state);

    let payload = json!({
        "action": "created",
        "issue": {
            "number": 123,
            "title": "Test Issue",
            "user": {
                "id": 1,
                "login": "owner"
            },
            "pull_request": {
                "url": "https://api.github.com/repos/owner/repo/pulls/123"
            }
        },
        "comment": {
            "id": 456,
            "body": "This is a comprehensive and high quality comment",
            "user": {
                "id": 12345,
                "login": "testuser"
            },
            "html_url": "https://github.com/owner/repo/issues/123#issuecomment-456"
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

    // Wait for async LLM evaluation
    sleep(Duration::from_millis(100)).await;

    // Verify credit was applied: starting 100 + high quality comment (+3) = 103
    let contributor = get_contributor(&db_pool, 12345, "owner", "repo")
        .await
        .unwrap()
        .expect("Contributor should exist");

    assert_eq!(contributor.credit_score, 103);
}

#[tokio::test]
async fn test_review_submitted_grants_fixed_credit() {
    let evaluator = MockEvaluator::new(); // Doesn't matter, reviews don't use LLM
    let state = setup_test_state_with_evaluator(evaluator).await;
    let db_pool = state.db_pool.clone();

    let app = create_app(state);

    let payload = json!({
        "action": "submitted",
        "review": {
            "id": 789,
            "body": "LGTM",
            "user": {
                "id": 12345,
                "login": "testuser"
            },
            "state": "approved",
            "html_url": "https://github.com/owner/repo/pull/123#pullrequestreview-789"
        },
        "pull_request": {
            "number": 123,
            "title": "Test PR",
            "body": null,
            "user": {
                "id": 1,
                "login": "owner"
            },
            "state": "open",
            "merged": false,
            "html_url": "https://github.com/owner/repo/pull/123"
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

    // No need to wait - reviews apply credit synchronously

    // Verify credit was applied immediately: starting 100 + review (+5) = 105
    let contributor = get_contributor(&db_pool, 12345, "owner", "repo")
        .await
        .unwrap()
        .expect("Contributor should exist");

    assert_eq!(contributor.credit_score, 105);

    // Verify event logged
    let events = list_events_by_contributor(&db_pool, contributor.id, 10, 0)
        .await
        .unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "review_submitted");
    assert_eq!(events[0].delta, 5);
    assert!(events[0].llm_evaluation.is_none()); // No LLM for reviews
}

#[tokio::test]
async fn test_spam_pr_deducts_credit() {
    let evaluator = MockEvaluator::with_default(QualityLevel::Spam);
    let state = setup_test_state_with_evaluator(evaluator).await;
    let db_pool = state.db_pool.clone();

    let app = create_app(state);

    let payload = json!({
        "action": "opened",
        "number": 1,
        "pull_request": {
            "number": 1,
            "title": "Buy now! Click here for free money!",
            "body": "spam spam spam",
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

    assert_eq!(response.status(), StatusCode::OK);

    // Wait for async LLM evaluation
    sleep(Duration::from_millis(100)).await;

    // Verify negative credit was applied: starting 100 + spam PR (-25) = 75
    let contributor = get_contributor(&db_pool, 12345, "owner", "repo")
        .await
        .unwrap()
        .expect("Contributor should exist");

    assert_eq!(contributor.credit_score, 75);
}
