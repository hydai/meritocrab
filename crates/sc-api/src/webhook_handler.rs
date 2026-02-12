use crate::{
    error::ApiResult,
    extractors::VerifiedWebhookPayload,
    state::AppState,
};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use rand::Rng;
use sc_core::{check_blacklist, check_pr_gate, calculate_delta_with_config, apply_credit, EventType, GateResult};
use sc_db::{
    contributors::{lookup_or_create_contributor, update_credit_score, set_blacklisted},
    credit_events::insert_credit_event,
    evaluations::insert_evaluation,
};
use sc_github::{PullRequestEvent, IssueCommentEvent, PullRequestReviewEvent};
use sc_llm::{ContentType, EvalContext};
use serde_json::Value;
use std::time::Duration;
use tracing::{info, warn, error};

/// Webhook handler for GitHub events
///
/// This handler:
/// 1. Verifies HMAC signature (handled by VerifiedWebhook extractor)
/// 2. Parses the event payload
/// 3. Processes pull_request, issue_comment, and pull_request_review events
/// 4. Returns 200 OK immediately (async LLM processing happens in background)
pub async fn handle_webhook(
    State(state): State<AppState>,
    VerifiedWebhookPayload(body): VerifiedWebhookPayload,
) -> ApiResult<impl IntoResponse> {
    // Parse the event payload
    let payload: Value = serde_json::from_slice(&body)?;

    // Check event type and action
    if let Some(action) = payload.get("action").and_then(|v| v.as_str()) {
        // Handle pull_request events
        if let Some(_pull_request) = payload.get("pull_request") {
            // Check if this is a review event
            if let Some(_review) = payload.get("review") {
                // This is a pull_request_review event
                if action == "submitted" {
                    let event: PullRequestReviewEvent = serde_json::from_slice(&body)?;
                    process_pr_review_submitted(state, event).await?;
                    return Ok((StatusCode::OK, Json(serde_json::json!({
                        "status": "ok",
                        "message": "Review processed successfully"
                    }))));
                }
            } else if action == "opened" {
                // This is a pull_request.opened event
                let event: PullRequestEvent = serde_json::from_slice(&body)?;
                process_pr_opened(state, event).await?;
                return Ok((StatusCode::OK, Json(serde_json::json!({
                    "status": "ok",
                    "message": "PR processed successfully"
                }))));
            }
        }

        // Handle issue_comment events
        if let Some(_issue) = payload.get("issue") {
            if let Some(_comment) = payload.get("comment") {
                if action == "created" {
                    let event: IssueCommentEvent = serde_json::from_slice(&body)?;
                    // Only process comments on pull requests
                    if event.issue.pull_request.is_some() {
                        process_comment_created(state, event).await?;
                        return Ok((StatusCode::OK, Json(serde_json::json!({
                            "status": "ok",
                            "message": "Comment processed successfully"
                        }))));
                    }
                }
            }
        }
    }

    // Return 200 OK for unhandled events
    info!("Received webhook event (not handled), ignoring");
    Ok((StatusCode::OK, Json(serde_json::json!({
        "status": "ok",
        "message": "Event type not processed"
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

    // Step 3: Check if contributor is blacklisted (or check is_blacklisted field)
    if contributor.is_blacklisted || check_blacklist(contributor.credit_score, state.repo_config.blacklist_threshold) {
        warn!(
            "Contributor {} is blacklisted (credit: {}, is_blacklisted: {}), scheduling delayed PR close for #{}",
            username, contributor.credit_score, contributor.is_blacklisted, pr_number
        );

        // Shadow blacklist: schedule delayed PR close with randomized delay (30-120 seconds)
        schedule_delayed_pr_close(
            state.clone(),
            repo_owner.to_string(),
            repo_name.to_string(),
            pr_number,
            username.to_string(),
        );

        // Return 200 OK immediately (delay happens in background)
        return Ok(());
    }

    // Step 4: Check PR gate (credit threshold)
    let gate_result = check_pr_gate(contributor.credit_score, state.repo_config.pr_threshold);

    match gate_result {
        GateResult::Allow => {
            info!(
                "PR #{} allowed (credit: {} >= threshold: {}), spawning LLM evaluation",
                pr_number, contributor.credit_score, state.repo_config.pr_threshold
            );

            // Step 5: Spawn async LLM evaluation
            spawn_pr_evaluation(
                state.clone(),
                contributor.id,
                user_id,
                username.to_string(),
                repo_owner.to_string(),
                repo_name.to_string(),
                event.pull_request.title,
                event.pull_request.body.unwrap_or_default(),
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

/// Schedule delayed PR close for shadow blacklist
///
/// This spawns a background task that waits a randomized delay (30-120 seconds)
/// before closing the PR with a generic message. This makes the blacklist less
/// obvious to bad actors.
fn schedule_delayed_pr_close(
    state: AppState,
    repo_owner: String,
    repo_name: String,
    pr_number: u64,
    username: String,
) {
    tokio::spawn(async move {
        // Generate random delay between 30 and 120 seconds
        let delay_secs = rand::rng().random_range(30..=120);
        let delay = Duration::from_secs(delay_secs);

        info!(
            "Scheduled PR #{} close for blacklisted user {} with delay of {} seconds",
            pr_number, username, delay_secs
        );

        // Wait for the randomized delay
        tokio::time::sleep(delay).await;

        // Close PR with generic message (no mention of blacklist/credit/spam)
        let generic_message = "Thank you for your contribution. Unfortunately, we are unable to accept this pull request at this time.";

        if let Err(e) = close_pr_with_message(
            &state,
            &repo_owner,
            &repo_name,
            pr_number,
            generic_message,
        )
        .await
        {
            error!(
                "Failed to close blacklisted PR #{} for {}: {}",
                pr_number, username, e
            );
        } else {
            info!(
                "Successfully closed blacklisted PR #{} for {} after {} second delay",
                pr_number, username, delay_secs
            );
        }
    });
}

/// Process a pull request review submitted event
async fn process_pr_review_submitted(state: AppState, event: PullRequestReviewEvent) -> ApiResult<()> {
    let user_id = event.review.user.id;
    let username = &event.review.user.login;
    let repo_owner = &event.repository.owner.login;
    let repo_name = &event.repository.name;

    info!(
        "Processing review submitted by {} in {}/{}",
        username, repo_owner, repo_name
    );

    // Check if user is a maintainer/collaborator (skip credit for privileged roles)
    match state
        .github_client
        .check_collaborator_role(repo_owner, repo_name, username)
        .await
    {
        Ok(role) if role.is_maintainer() || role.has_write_access() => {
            info!(
                "User {} has privileged role {:?}, skipping credit for review",
                username, role
            );
            return Ok(());
        }
        Ok(_) => {
            // User is not privileged, proceed with credit grant
        }
        Err(e) => {
            warn!(
                "Failed to check collaborator role for {}: {}. Proceeding with credit grant.",
                username, e
            );
        }
    }

    // Lookup or create contributor
    let contributor = lookup_or_create_contributor(
        &state.db_pool,
        user_id,
        repo_owner,
        repo_name,
        state.repo_config.starting_credit,
    )
    .await?;

    // Check if blacklisted (skip credit for blacklisted users)
    if check_blacklist(contributor.credit_score, state.repo_config.blacklist_threshold) {
        info!(
            "Contributor {} is blacklisted, skipping credit for review",
            username
        );
        return Ok(());
    }

    // Reviews always grant +5 credit (no LLM evaluation needed)
    let delta = 5;
    let credit_before = contributor.credit_score;
    let credit_after = apply_credit(credit_before, delta);

    // Update contributor credit
    update_credit_score(&state.db_pool, contributor.id, credit_after).await?;

    // Log credit event
    insert_credit_event(
        &state.db_pool,
        contributor.id,
        "review_submitted",
        delta,
        credit_before,
        credit_after,
        None, // No LLM evaluation for reviews
        None,
    )
    .await?;

    info!(
        "Granted +{} credit to {} for review (new score: {})",
        delta, username, credit_after
    );

    // Note: Reviews always have positive delta, so no auto-blacklist check needed

    Ok(())
}

/// Process an issue comment created event
async fn process_comment_created(state: AppState, event: IssueCommentEvent) -> ApiResult<()> {
    let user_id = event.comment.user.id;
    let username = &event.comment.user.login;
    let repo_owner = &event.repository.owner.login;
    let repo_name = &event.repository.name;
    let comment_body = &event.comment.body;

    info!(
        "Processing comment by {} in {}/{} on PR #{}",
        username, repo_owner, repo_name, event.issue.number
    );

    // Check if user is a maintainer/collaborator (skip credit for privileged roles)
    match state
        .github_client
        .check_collaborator_role(repo_owner, repo_name, username)
        .await
    {
        Ok(role) if role.is_maintainer() || role.has_write_access() => {
            info!(
                "User {} has privileged role {:?}, skipping credit for comment",
                username, role
            );
            return Ok(());
        }
        Ok(_) => {
            // User is not privileged, proceed with credit evaluation
        }
        Err(e) => {
            warn!(
                "Failed to check collaborator role for {}: {}. Proceeding with credit evaluation.",
                username, e
            );
        }
    }

    // Lookup or create contributor
    let contributor = lookup_or_create_contributor(
        &state.db_pool,
        user_id,
        repo_owner,
        repo_name,
        state.repo_config.starting_credit,
    )
    .await?;

    // Check if blacklisted (comment stays but no credit earned)
    if check_blacklist(contributor.credit_score, state.repo_config.blacklist_threshold) {
        info!(
            "Contributor {} is blacklisted, skipping credit for comment",
            username
        );
        return Ok(());
    }

    // Spawn async LLM evaluation for the comment
    spawn_comment_evaluation(
        state.clone(),
        contributor.id,
        user_id,
        username.to_string(),
        repo_owner.to_string(),
        repo_name.to_string(),
        comment_body.clone(),
        event.issue.title,
    );

    Ok(())
}

/// Spawn async PR evaluation task
fn spawn_pr_evaluation(
    state: AppState,
    contributor_id: i64,
    user_id: i64,
    username: String,
    repo_owner: String,
    repo_name: String,
    pr_title: String,
    pr_body: String,
) {
    tokio::spawn(async move {
        if let Err(e) = evaluate_and_apply_credit(
            state,
            contributor_id,
            user_id,
            username,
            repo_owner,
            repo_name,
            EventType::PrOpened,
            ContentType::PullRequest,
            Some(pr_title.clone()),
            pr_body.clone(),
            None,
            None,
        )
        .await
        {
            error!("Failed to evaluate PR: {}", e);
        }
    });
}

/// Spawn async comment evaluation task
fn spawn_comment_evaluation(
    state: AppState,
    contributor_id: i64,
    user_id: i64,
    username: String,
    repo_owner: String,
    repo_name: String,
    comment_body: String,
    thread_context: String,
) {
    tokio::spawn(async move {
        if let Err(e) = evaluate_and_apply_credit(
            state,
            contributor_id,
            user_id,
            username,
            repo_owner,
            repo_name,
            EventType::Comment,
            ContentType::Comment,
            None,
            comment_body.clone(),
            None,
            Some(thread_context),
        )
        .await
        {
            error!("Failed to evaluate comment: {}", e);
        }
    });
}

/// Evaluate content and apply credit based on confidence
async fn evaluate_and_apply_credit(
    state: AppState,
    contributor_id: i64,
    user_id: i64,
    username: String,
    repo_owner: String,
    repo_name: String,
    event_type: EventType,
    content_type: ContentType,
    title: Option<String>,
    body: String,
    diff_summary: Option<String>,
    thread_context: Option<String>,
) -> ApiResult<()> {
    // Acquire semaphore permit to limit concurrent evaluations
    let _permit = state.llm_semaphore.acquire().await.map_err(|e| {
        crate::error::ApiError::Internal(format!("Failed to acquire semaphore: {}", e))
    })?;

    info!(
        "Evaluating {} for user {} in {}/{}",
        match content_type {
            ContentType::PullRequest => "PR",
            ContentType::Comment => "comment",
            ContentType::Review => "review",
        },
        username,
        repo_owner,
        repo_name
    );

    // Create evaluation context
    let context = EvalContext {
        content_type,
        title: title.clone(),
        body: body.clone(),
        diff_summary,
        thread_context,
    };

    // Perform LLM evaluation
    let evaluation = state
        .llm_evaluator
        .evaluate(&body, &context)
        .await
        .map_err(|e| crate::error::ApiError::Internal(format!("LLM evaluation failed: {}", e)))?;

    info!(
        "LLM evaluation for {}: {:?} (confidence: {})",
        username, evaluation.classification, evaluation.confidence
    );

    // Calculate credit delta
    let delta = calculate_delta_with_config(
        &state.repo_config,
        event_type,
        evaluation.classification,
    );

    // Serialize LLM evaluation to JSON string
    let llm_eval_json_str = serde_json::to_string(&evaluation)
        .map_err(|e| crate::error::ApiError::Internal(format!("Failed to serialize LLM evaluation: {}", e)))?;

    // Get current contributor state
    let contributor = sc_db::contributors::get_contributor(&state.db_pool, user_id, &repo_owner, &repo_name)
        .await?
        .ok_or_else(|| crate::error::ApiError::Internal("Contributor not found".to_string()))?;

    let credit_before = contributor.credit_score;

    // Check confidence threshold
    if evaluation.confidence >= 0.85 {
        // High confidence: apply credit automatically
        let credit_after = apply_credit(credit_before, delta);

        // Update contributor credit
        update_credit_score(&state.db_pool, contributor_id, credit_after).await?;

        // Log credit event with LLM evaluation
        insert_credit_event(
            &state.db_pool,
            contributor_id,
            match event_type {
                EventType::PrOpened => "pr_opened",
                EventType::Comment => "comment",
                EventType::PrMerged => "pr_merged",
                EventType::ReviewSubmitted => "review_submitted",
            },
            delta,
            credit_before,
            credit_after,
            Some(llm_eval_json_str),
            None,
        )
        .await?;

        info!(
            "Applied {} credit to {} (confidence {:.2}, new score: {})",
            delta, username, evaluation.confidence, credit_after
        );

        // Auto-blacklist if credit drops to 0 or below
        if credit_after <= state.repo_config.blacklist_threshold && credit_before > state.repo_config.blacklist_threshold {
            warn!(
                "Auto-blacklisting user {} (credit dropped to {})",
                username, credit_after
            );

            // Set blacklist flag
            set_blacklisted(&state.db_pool, contributor_id, true).await?;

            // Log auto-blacklist event
            insert_credit_event(
                &state.db_pool,
                contributor_id,
                "auto_blacklist",
                0, // No delta for blacklist event
                credit_after,
                credit_after,
                None,
                Some(format!("Auto-blacklisted due to credit dropping to {}", credit_after)),
            )
            .await?;

            info!(
                "Successfully auto-blacklisted user {} (credit: {})",
                username, credit_after
            );
        }
    } else {
        // Low confidence: create pending evaluation
        let eval_id = format!(
            "eval-{}-{}-{}",
            user_id,
            repo_name,
            chrono::Utc::now().timestamp()
        );

        insert_evaluation(
            &state.db_pool,
            eval_id.clone(),
            contributor_id,
            &repo_owner,
            &repo_name,
            format!("{:?}", evaluation.classification),
            evaluation.confidence,
            delta,
        )
        .await?;

        info!(
            "Created pending evaluation {} for {} (confidence {:.2}, proposed delta: {})",
            eval_id, username, evaluation.confidence, delta
        );
    }

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
    use std::sync::Arc;

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

        // Create mock LLM evaluator
        let llm_evaluator = Arc::new(sc_llm::MockEvaluator::new());

        let webhook_secret = WebhookSecret::new("test-secret".to_string());
        let repo_config = RepoConfig::default();

        AppState::new(pool, github_client, repo_config, webhook_secret, llm_evaluator, 10)
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
