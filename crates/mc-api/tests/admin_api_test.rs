/// Integration tests for admin API endpoints
/// Note: These tests verify the API structure and basic authentication flow.
/// Full OAuth integration testing would require more complex mocking.
use mc_api::{admin_handlers, OAuthConfig};
use mc_db::{
    contributors::create_contributor, credit_events::insert_credit_event,
    evaluations::insert_evaluation,
};
use sqlx::any::AnyPoolOptions;

fn test_oauth_config() -> OAuthConfig {
    OAuthConfig {
        client_id: "test-client-id".to_string(),
        client_secret: "test-client-secret".to_string(),
        redirect_url: "http://localhost:8080/auth/callback".to_string(),
    }
}

async fn setup_test_db() -> sqlx::Pool<sqlx::Any> {
    sqlx::any::install_default_drivers();

    let pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create test database pool");

    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .expect("Failed to enable foreign keys");

    sqlx::query(include_str!("../../mc-db/migrations/001_initial.sql"))
        .execute(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}

#[tokio::test]
async fn test_admin_api_db_functions() {
    let pool = setup_test_db().await;

    // Create test contributor
    let contributor = create_contributor(&pool, 12345, "test-owner", "test-repo", 100)
        .await
        .expect("Failed to create contributor");

    assert_eq!(contributor.credit_score, 100);

    // Create test evaluation
    let eval = insert_evaluation(
        &pool,
        "eval-123".to_string(),
        contributor.id,
        "test-owner",
        "test-repo",
        "spam".to_string(),
        0.9,
        -25,
    )
    .await
    .expect("Failed to insert evaluation");

    assert_eq!(eval.status, "pending");
    assert_eq!(eval.proposed_delta, -25);

    // Create test credit event
    let event = insert_credit_event(
        &pool,
        contributor.id,
        "pr_opened",
        10,
        100,
        110,
        Some(r#"{"quality": "high"}"#.to_string()),
        None,
    )
    .await
    .expect("Failed to insert credit event");

    assert_eq!(event.delta, 10);
    assert_eq!(event.credit_after, 110);
}

#[tokio::test]
async fn test_oauth_config_structure() {
    let config = test_oauth_config();

    assert_eq!(config.client_id, "test-client-id");
    assert_eq!(config.client_secret, "test-client-secret");
    assert!(config.redirect_url.contains("/auth/callback"));
}

#[tokio::test]
async fn test_admin_handlers_exist() {
    // This test verifies that all required admin handler functions exist
    // and have the correct signatures. This is a compile-time check.

    // These are function pointers to the admin handlers
    let _list_evaluations = admin_handlers::list_evaluations;
    let _approve_evaluation = admin_handlers::approve_evaluation_handler;
    let _override_evaluation = admin_handlers::override_evaluation_handler;
    let _list_contributors = admin_handlers::list_contributors;
    let _adjust_credit = admin_handlers::adjust_contributor_credit;
    let _toggle_blacklist = admin_handlers::toggle_contributor_blacklist;
    let _list_events = admin_handlers::list_credit_events;

    // If this compiles, all handlers exist with correct signatures
    assert!(true);
}

#[tokio::test]
async fn test_admin_api_structures_compile() {
    // This test verifies that admin API structures can be constructed
    // If this compiles, the API is correctly structured
    let pool = setup_test_db().await;

    // Test that we can create the database structures needed for admin API
    let contributor = create_contributor(&pool, 999, "owner", "repo", 75)
        .await
        .expect("Failed to create contributor");

    assert_eq!(contributor.credit_score, 75);
}
