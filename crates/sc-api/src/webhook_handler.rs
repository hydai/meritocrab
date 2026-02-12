use crate::{
    error::ApiResult,
    extractors::VerifiedWebhookPayload,
    state::AppState,
};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use sc_core::{check_blacklist, check_pr_gate, GateResult};
use sc_db::{contributors::lookup_or_create_contributor, credit_events::insert_credit_event};
use sc_github::PullRequestEvent;
use serde_json::Value;
use tracing::{info, warn};

/// Webhook handler for GitHub events
///
/// This handler:
/// 1. Verifies HMAC signature (handled by VerifiedWebhook extractor)
/// 2. Parses the event payload
/// 3. Processes pull_request.opened events
/// 4. Returns 200 OK immediately (async processing happens later for LLM)
///
/// Note: LLM evaluation is NOT implemented in this MVP (Task #6).
/// This handler only implements the credit gate check using existing scores.
pub async fn handle_webhook(
    State(state): State<AppState>,
    VerifiedWebhookPayload(body): VerifiedWebhookPayload,
) -> ApiResult<impl IntoResponse> {
    // Parse the event payload
    let payload: Value = serde_json::from_slice(&body)?;

    // Check if this is a pull_request event
    if let Some(action) = payload.get("action").and_then(|v| v.as_str()) {
        if let Some(_pull_request) = payload.get("pull_request") {
            // This is a pull request event
            if action == "opened" {
                // Parse into PullRequestEvent
                let event: PullRequestEvent = serde_json::from_slice(&body)?;

                // Process PR opened event
                process_pr_opened(state, event).await?;

                return Ok((StatusCode::OK, Json(serde_json::json!({
                    "status": "ok",
                    "message": "Webhook processed successfully"
                }))));
            }
        }
    }

    // For now, we only handle pull_request.opened events
    // Other events will be handled in Task #6
    info!("Received webhook event (not pull_request.opened), ignoring");
    Ok((StatusCode::OK, Json(serde_json::json!({
        "status": "ok",
        "message": "Event type not processed yet"
    }))))
}

/// Process a PR opened event
async fn process_pr_opened(state: AppState, event: PullRequestEvent) -> ApiResult<()> {
    let user_id = event.pull_request.user.id;
    let username = &event.pull_request.user.login;
    let repo_owner = &event.repository.owner.login;
    let repo_name = &event.repository.name;
    let pr_number = event.pull_request.number as u64;

    info!(
        "Processing PR #{} opened by {} in {}/{}",
        pr_number, username, repo_owner, repo_name
    );

    // Step 1: Check if user is a maintainer/collaborator (bypass credit check)
    // If role check fails (e.g., GitHub API unavailable), proceed with credit check
    match state
        .github_client
        .check_collaborator_role(repo_owner, repo_name, username)
        .await
    {
        Ok(role) if role.is_maintainer() => {
            info!(
                "User {} has maintainer role {:?}, bypassing credit check",
                username, role
            );
            return Ok(());
        }
        Ok(_) => {
            // User is not a maintainer, proceed with credit check
        }
        Err(e) => {
            // GitHub API error - log and proceed with credit check
            warn!(
                "Failed to check collaborator role for {}: {}. Proceeding with credit check.",
                username, e
            );
        }
    }

    // Step 2: Lookup or create contributor
    let contributor = lookup_or_create_contributor(
        &state.db_pool,
        user_id,
        repo_owner,
        repo_name,
        state.repo_config.starting_credit,
    )
    .await?;

    info!(
        "Contributor {} has credit score {}",
        username, contributor.credit_score
    );

    // Log the PR opened event (with 0 delta for now, LLM evaluation in Task #6)
    insert_credit_event(
        &state.db_pool,
        contributor.id,
        "pr_opened",
        0, // Delta is 0 for now, will be applied after LLM evaluation
        contributor.credit_score,
        contributor.credit_score,
        None, // LLM evaluation not implemented yet
        None,
    )
    .await?;

    // Step 3: Check if contributor is blacklisted
    if check_blacklist(contributor.credit_score, state.repo_config.blacklist_threshold) {
        warn!(
            "Contributor {} is blacklisted (credit: {}), closing PR #{}",
            username, contributor.credit_score, pr_number
        );

        // Note: Shadow blacklist with randomized delay is Task #7
        // For MVP, we just close immediately with a generic message
        close_pr_with_message(
            &state,
            repo_owner,
            repo_name,
            pr_number,
            "Thank you for your contribution. After review, we've determined this PR doesn't align with the project's current direction.",
        )
        .await?;

        return Ok(());
    }

    // Step 4: Check PR gate (credit threshold)
    let gate_result = check_pr_gate(contributor.credit_score, state.repo_config.pr_threshold);

    match gate_result {
        GateResult::Allow => {
            info!(
                "PR #{} allowed (credit: {} >= threshold: {})",
                pr_number, contributor.credit_score, state.repo_config.pr_threshold
            );
        }
        GateResult::Deny => {
            warn!(
                "PR #{} denied (credit: {} < threshold: {}), closing",
                pr_number, contributor.credit_score, state.repo_config.pr_threshold
            );

            close_pr_with_message(
                &state,
                repo_owner,
                repo_name,
                pr_number,
                &format!(
                    "Your contribution score ({}) is below the required threshold ({}). Please build your score through quality comments and reviews.",
                    contributor.credit_score, state.repo_config.pr_threshold
                ),
            )
            .await?;
        }
    }

    Ok(())
}

/// Helper to close PR and add comment
async fn close_pr_with_message(
    state: &AppState,
    repo_owner: &str,
    repo_name: &str,
    pr_number: u64,
    message: &str,
) -> ApiResult<()> {
    // Add comment first
    state
        .github_client
        .add_comment(repo_owner, repo_name, pr_number, message)
        .await?;

    // Then close the PR
    state
        .github_client
        .close_pull_request(repo_owner, repo_name, pr_number)
        .await?;

    info!("Closed PR #{} with message", pr_number);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::ApiError, state::AppState};
    use hmac::{Hmac, Mac};
    use sc_core::RepoConfig;
    use sc_github::{GithubApiClient, WebhookSecret};
    use sha2::Sha256;
    use sqlx::any::AnyPoolOptions;

    type HmacSha256 = Hmac<Sha256>;

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

        // Create mock GitHub client (will need to be updated with actual mock)
        let github_client = create_mock_github_client();

        let webhook_secret = WebhookSecret::new("test-secret".to_string());
        let repo_config = RepoConfig::default();

        AppState::new(pool, github_client, repo_config, webhook_secret)
    }

    fn create_mock_github_client() -> GithubApiClient {
        // Initialize rustls crypto provider for tests
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        // For now, create a client that will fail if called
        // In a real test, we'd use wiremock or similar
        GithubApiClient::new("test-token".to_string()).expect("Failed to create mock client")
    }

    fn compute_signature(body: &[u8], secret: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let result = mac.finalize();
        format!("sha256={}", hex::encode(result.into_bytes()))
    }

    #[tokio::test]
    async fn test_parse_pull_request_event() {
        let payload = serde_json::json!({
            "action": "opened",
            "number": 123,
            "pull_request": {
                "number": 123,
                "title": "Test PR",
                "body": "Test body",
                "user": {
                    "id": 12345,
                    "login": "testuser"
                },
                "state": "open",
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

        let event: Result<PullRequestEvent, _> = serde_json::from_value(payload);
        assert!(event.is_ok());
    }

    #[tokio::test]
    async fn test_webhook_handler_invalid_json() {
        let state = setup_test_state().await;
        let body = b"{invalid json}";

        let webhook_payload = VerifiedWebhookPayload(body.to_vec());
        let result = handle_webhook(State(state), webhook_payload).await;

        assert!(result.is_err());
    }

    #[test]
    fn test_error_conversion() {
        let json_err = serde_json::from_str::<serde_json::Value>("{invalid}").unwrap_err();
        let api_err: ApiError = json_err.into();

        match api_err {
            ApiError::InvalidPayload(_) => {},
            _ => panic!("Expected InvalidPayload error"),
        }
    }
}
