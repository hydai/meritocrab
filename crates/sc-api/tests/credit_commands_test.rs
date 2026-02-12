use sc_api::{credit_commands::*, handle_webhook, AppState, OAuthConfig, VerifiedWebhookPayload};
use sc_core::RepoConfig;
use sc_github::{GithubApiClient, WebhookSecret};
use sqlx::any::AnyPoolOptions;
use std::sync::Arc;

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
    sqlx::query(include_str!("../../sc-db/migrations/001_initial.sql"))
        .execute(&pool)
        .await
        .expect("Failed to run migrations");

    // Initialize rustls for GitHub client
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Create mock GitHub client
    let github_client = GithubApiClient::new("test-token".to_string())
        .expect("Failed to create GitHub client");

    // Create mock LLM evaluator
    let llm_evaluator = Arc::new(sc_llm::MockEvaluator::new());

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

#[tokio::test]
async fn test_parse_credit_check_command() {
    let comment = "/credit check @user123";
    let cmd = parse_credit_command(comment);
    assert!(matches!(cmd, Some(CreditCommand::Check { .. })));

    if let Some(CreditCommand::Check { username }) = cmd {
        assert_eq!(username, "user123");
    }
}

#[tokio::test]
async fn test_parse_credit_override_command() {
    let comment = r#"/credit override @user123 +10 "good work""#;
    let cmd = parse_credit_command(comment);
    assert!(matches!(cmd, Some(CreditCommand::Override { .. })));

    if let Some(CreditCommand::Override { username, delta, reason }) = cmd {
        assert_eq!(username, "user123");
        assert_eq!(delta, 10);
        assert_eq!(reason, "good work");
    }
}

#[tokio::test]
async fn test_parse_credit_override_negative() {
    let comment = r#"/credit override @spammer -25 "spam content""#;
    let cmd = parse_credit_command(comment);
    assert!(matches!(cmd, Some(CreditCommand::Override { .. })));

    if let Some(CreditCommand::Override { username, delta, reason }) = cmd {
        assert_eq!(username, "spammer");
        assert_eq!(delta, -25);
        assert_eq!(reason, "spam content");
    }
}

#[tokio::test]
async fn test_parse_credit_blacklist_command() {
    let comment = "/credit blacklist @badactor";
    let cmd = parse_credit_command(comment);
    assert!(matches!(cmd, Some(CreditCommand::Blacklist { .. })));

    if let Some(CreditCommand::Blacklist { username }) = cmd {
        assert_eq!(username, "badactor");
    }
}

#[tokio::test]
async fn test_credit_command_not_found() {
    let comment = "This is a regular comment without any credit command";
    let cmd = parse_credit_command(comment);
    assert!(cmd.is_none());
}

#[tokio::test]
async fn test_credit_command_in_multiline_comment() {
    let comment = r#"Some discussion here

/credit check @user123

More discussion after the command"#;
    let cmd = parse_credit_command(comment);
    assert!(matches!(cmd, Some(CreditCommand::Check { .. })));
}

#[tokio::test]
async fn test_repo_config_loader_returns_defaults() {
    let state = setup_test_state().await;

    // Get config for a non-existent repo (should return defaults)
    let config = state.repo_config_loader.get_config("test-owner", "test-repo").await;

    // Verify defaults
    assert_eq!(config.starting_credit, 100);
    assert_eq!(config.pr_threshold, 50);
    assert_eq!(config.blacklist_threshold, 0);
}

#[tokio::test]
async fn test_credit_override_applies_delta() {
    let state = setup_test_state().await;

    // Create a contributor
    let contributor = sc_db::contributors::create_contributor(
        &state.db_pool,
        12345,
        "test-owner",
        "test-repo",
        100,
    )
    .await
    .expect("Failed to create contributor");

    assert_eq!(contributor.credit_score, 100);

    // Apply credit delta
    let new_score = sc_core::apply_credit(contributor.credit_score, 15);
    assert_eq!(new_score, 115);

    // Update contributor
    sc_db::contributors::update_credit_score(&state.db_pool, contributor.id, new_score)
        .await
        .expect("Failed to update credit score");

    // Verify update
    let updated = sc_db::contributors::get_contributor(&state.db_pool, 12345, "test-owner", "test-repo")
        .await
        .expect("Failed to get contributor")
        .expect("Contributor not found");

    assert_eq!(updated.credit_score, 115);
}

#[tokio::test]
async fn test_credit_override_triggers_auto_blacklist() {
    let state = setup_test_state().await;

    // Create a contributor with low credit
    let contributor = sc_db::contributors::create_contributor(
        &state.db_pool,
        12345,
        "test-owner",
        "test-repo",
        10,
    )
    .await
    .expect("Failed to create contributor");

    assert_eq!(contributor.credit_score, 10);
    assert!(!contributor.is_blacklisted);

    // Apply negative delta that drops credit to 0
    let new_score = sc_core::apply_credit(contributor.credit_score, -15);
    assert_eq!(new_score, 0);

    // Auto-blacklist should trigger when credit <= 0
    if new_score <= state.repo_config.blacklist_threshold {
        sc_db::contributors::set_blacklisted(&state.db_pool, contributor.id, true)
            .await
            .expect("Failed to set blacklist");
    }

    // Verify blacklist
    let updated = sc_db::contributors::get_contributor(&state.db_pool, 12345, "test-owner", "test-repo")
        .await
        .expect("Failed to get contributor")
        .expect("Contributor not found");

    assert_eq!(updated.credit_score, 10); // Credit not updated yet in this test
    assert!(updated.is_blacklisted);
}

#[tokio::test]
async fn test_blacklist_command_sets_flag() {
    let state = setup_test_state().await;

    // Create a contributor
    let contributor = sc_db::contributors::create_contributor(
        &state.db_pool,
        12345,
        "test-owner",
        "test-repo",
        100,
    )
    .await
    .expect("Failed to create contributor");

    assert!(!contributor.is_blacklisted);

    // Blacklist the contributor
    sc_db::contributors::set_blacklisted(&state.db_pool, contributor.id, true)
        .await
        .expect("Failed to set blacklist");

    // Verify blacklist
    let updated = sc_db::contributors::get_contributor(&state.db_pool, 12345, "test-owner", "test-repo")
        .await
        .expect("Failed to get contributor")
        .expect("Contributor not found");

    assert!(updated.is_blacklisted);
}

#[tokio::test]
async fn test_credit_events_logged_for_override() {
    let state = setup_test_state().await;

    // Create a contributor
    let contributor = sc_db::contributors::create_contributor(
        &state.db_pool,
        12345,
        "test-owner",
        "test-repo",
        100,
    )
    .await
    .expect("Failed to create contributor");

    // Log a credit event
    let event = sc_db::credit_events::insert_credit_event(
        &state.db_pool,
        contributor.id,
        "manual_adjustment",
        10,
        100,
        110,
        None,
        Some("Good contribution".to_string()),
    )
    .await
    .expect("Failed to insert credit event");

    assert_eq!(event.event_type, "manual_adjustment");
    assert_eq!(event.delta, 10);
    assert_eq!(event.credit_before, 100);
    assert_eq!(event.credit_after, 110);
    assert_eq!(event.maintainer_override, Some("Good contribution".to_string()));
}

#[tokio::test]
async fn test_list_credit_events_for_contributor() {
    let state = setup_test_state().await;

    // Create a contributor
    let contributor = sc_db::contributors::create_contributor(
        &state.db_pool,
        12345,
        "test-owner",
        "test-repo",
        100,
    )
    .await
    .expect("Failed to create contributor");

    // Insert multiple credit events
    for i in 0..5 {
        sc_db::credit_events::insert_credit_event(
            &state.db_pool,
            contributor.id,
            "comment",
            i,
            100 + i,
            100 + i + 1,
            None,
            None,
        )
        .await
        .expect("Failed to insert credit event");
    }

    // List events (last 5)
    let events = sc_db::credit_events::list_events_by_contributor(
        &state.db_pool,
        contributor.id,
        5,
        0,
    )
    .await
    .expect("Failed to list events");

    assert_eq!(events.len(), 5);

    // Events should be in reverse chronological order (most recent first)
    // The last inserted event (delta=4) should be first
    assert_eq!(events[0].delta, 4);
    assert_eq!(events[4].delta, 0);
}
