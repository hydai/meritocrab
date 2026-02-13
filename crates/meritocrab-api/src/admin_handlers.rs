use axum::{
    Extension,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use meritocrab_core::{EvaluationStatus, credit::apply_credit};
use meritocrab_db::{
    contributors::{
        count_contributors_by_repo, get_contributor_by_id, list_contributors_by_repo,
        set_blacklisted, update_credit_score,
    },
    credit_events::{count_events_by_repo, insert_credit_event, list_events_by_repo},
    evaluations::{
        approve_evaluation, get_evaluation, list_evaluations_by_repo_and_status,
        override_evaluation,
    },
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::error::{ApiError, ApiResult};
use crate::oauth::GithubUser;
use crate::state::AppState;

/// Pagination query parameters
#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_per_page")]
    per_page: i64,
    #[serde(default)]
    status: Option<String>,
}

fn default_page() -> i64 {
    1
}

fn default_per_page() -> i64 {
    20
}

/// Events filter query parameters
#[derive(Debug, Deserialize)]
pub struct EventsFilterQuery {
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_per_page")]
    per_page: i64,
    #[serde(default)]
    contributor_id: Option<i64>,
    #[serde(default)]
    event_type: Option<String>,
}

/// Paginated response wrapper
#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    data: Vec<T>,
    page: i64,
    per_page: i64,
    total: i64,
    total_pages: i64,
}

/// Evaluation response with contributor info
#[derive(Debug, Serialize)]
pub struct EvaluationResponse {
    pub id: String,
    pub contributor_id: i64,
    pub contributor_login: String,
    pub repo_owner: String,
    pub repo_name: String,
    pub llm_classification: String,
    pub confidence: f64,
    pub proposed_delta: i32,
    pub status: String,
    pub created_at: String,
}

/// Contributor response with last activity
#[derive(Debug, Serialize)]
pub struct ContributorResponse {
    pub id: i64,
    pub github_user_id: i64,
    pub username: String,
    pub credit_score: i32,
    pub role: Option<String>,
    pub is_blacklisted: bool,
    pub last_activity: String,
}

/// Credit event response
#[derive(Debug, Serialize)]
pub struct CreditEventResponse {
    pub id: i64,
    pub contributor_id: i64,
    pub event_type: String,
    pub delta: i32,
    pub credit_before: i32,
    pub credit_after: i32,
    pub llm_evaluation: Option<String>,
    pub maintainer_override: Option<String>,
    pub created_at: String,
}

/// Override evaluation request
#[derive(Debug, Deserialize)]
pub struct OverrideRequest {
    pub delta: i32,
    pub reason: String,
}

/// Adjust credit request
#[derive(Debug, Deserialize)]
pub struct AdjustCreditRequest {
    pub delta: i32,
    pub reason: String,
}

/// GET /api/repos/{owner}/{repo}/evaluations
/// List pending evaluations with pagination
pub async fn list_evaluations(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(pagination): Query<PaginationQuery>,
    Extension(_user): Extension<GithubUser>,
) -> ApiResult<Json<PaginatedResponse<EvaluationResponse>>> {
    let status_str = pagination.status.as_deref().unwrap_or("pending");
    let status = match status_str {
        "pending" => EvaluationStatus::Pending,
        "approved" => EvaluationStatus::Approved,
        "overridden" => EvaluationStatus::Overridden,
        "auto_applied" => EvaluationStatus::AutoApplied,
        _ => EvaluationStatus::Pending,
    };
    let offset = (pagination.page - 1) * pagination.per_page;

    // Fetch evaluations from database
    let evaluations = list_evaluations_by_repo_and_status(
        &state.db_pool,
        &owner,
        &repo,
        &status,
        pagination.per_page,
        offset,
    )
    .await
    .map_err(|e| {
        error!("Failed to list evaluations: {}", e);
        ApiError::InternalError(format!("Database error: {}", e))
    })?;

    // Count total evaluations
    let total = evaluations.len() as i64; // For simplicity, we're not implementing count separately

    // Convert to response format
    let data: Vec<EvaluationResponse> = evaluations
        .into_iter()
        .map(|eval| EvaluationResponse {
            id: eval.id,
            contributor_id: eval.contributor_id,
            contributor_login: format!("user-{}", eval.contributor_id), // TODO: fetch from GitHub API
            repo_owner: eval.repo_owner,
            repo_name: eval.repo_name,
            llm_classification: eval.llm_classification,
            confidence: eval.confidence,
            proposed_delta: eval.proposed_delta,
            status: eval.status,
            created_at: eval.created_at.to_rfc3339(),
        })
        .collect();

    let total_pages = (total + pagination.per_page - 1) / pagination.per_page;

    Ok(Json(PaginatedResponse {
        data,
        page: pagination.page,
        per_page: pagination.per_page,
        total,
        total_pages,
    }))
}

/// POST /api/repos/{owner}/{repo}/evaluations/{id}/approve
/// Approve a pending evaluation
pub async fn approve_evaluation_handler(
    State(state): State<AppState>,
    Path((owner, repo, eval_id)): Path<(String, String, String)>,
    Extension(_user): Extension<GithubUser>,
) -> ApiResult<Response> {
    // Fetch evaluation
    let evaluation = get_evaluation(&state.db_pool, &eval_id)
        .await
        .map_err(|e| {
            error!("Failed to get evaluation: {}", e);
            ApiError::InternalError(format!("Database error: {}", e))
        })?
        .ok_or_else(|| ApiError::NotFound(format!("Evaluation not found: {}", eval_id)))?;

    // Verify evaluation belongs to this repo
    if evaluation.repo_owner != owner || evaluation.repo_name != repo {
        return Err(ApiError::NotFound("Evaluation not found".to_string()));
    }

    // Verify evaluation is pending
    if evaluation.status != "pending" {
        return Err(ApiError::BadRequest(format!(
            "Evaluation is not pending: {}",
            evaluation.status
        )));
    }

    // Get contributor
    let contributor = get_contributor_by_id(&state.db_pool, evaluation.contributor_id)
        .await
        .map_err(|e| {
            error!("Failed to get contributor: {}", e);
            ApiError::InternalError(format!("Database error: {}", e))
        })?
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "Contributor not found: {}",
                evaluation.contributor_id
            ))
        })?;

    // Apply credit delta
    let credit_before = contributor.credit_score;
    let credit_after = apply_credit(credit_before, evaluation.proposed_delta);

    // Update credit score
    update_credit_score(&state.db_pool, contributor.id, credit_after)
        .await
        .map_err(|e| {
            error!("Failed to update credit score: {}", e);
            ApiError::InternalError(format!("Database error: {}", e))
        })?;

    // Log credit event
    insert_credit_event(
        &state.db_pool,
        contributor.id,
        "evaluation_approved",
        evaluation.proposed_delta,
        credit_before,
        credit_after,
        Some(format!(
            r#"{{"evaluation_id": "{}", "classification": "{}"}}"#,
            evaluation.id, evaluation.llm_classification
        )),
        Some("false".to_string()), // maintainer_override = false
    )
    .await
    .map_err(|e| {
        error!("Failed to insert credit event: {}", e);
        ApiError::InternalError(format!("Database error: {}", e))
    })?;

    // Approve evaluation
    approve_evaluation(&state.db_pool, &eval_id, None)
        .await
        .map_err(|e| {
            error!("Failed to approve evaluation: {}", e);
            ApiError::InternalError(format!("Database error: {}", e))
        })?;

    info!(
        "Evaluation {} approved by maintainer for contributor {}",
        eval_id, contributor.id
    );

    Ok((StatusCode::OK, "Evaluation approved").into_response())
}

/// POST /api/repos/{owner}/{repo}/evaluations/{id}/override
/// Override a pending evaluation with custom delta
pub async fn override_evaluation_handler(
    State(state): State<AppState>,
    Path((owner, repo, eval_id)): Path<(String, String, String)>,
    Extension(_user): Extension<GithubUser>,
    Json(req): Json<OverrideRequest>,
) -> ApiResult<Response> {
    // Fetch evaluation
    let evaluation = get_evaluation(&state.db_pool, &eval_id)
        .await
        .map_err(|e| {
            error!("Failed to get evaluation: {}", e);
            ApiError::InternalError(format!("Database error: {}", e))
        })?
        .ok_or_else(|| ApiError::NotFound(format!("Evaluation not found: {}", eval_id)))?;

    // Verify evaluation belongs to this repo
    if evaluation.repo_owner != owner || evaluation.repo_name != repo {
        return Err(ApiError::NotFound("Evaluation not found".to_string()));
    }

    // Verify evaluation is pending
    if evaluation.status != "pending" {
        return Err(ApiError::BadRequest(format!(
            "Evaluation is not pending: {}",
            evaluation.status
        )));
    }

    // Get contributor
    let contributor = get_contributor_by_id(&state.db_pool, evaluation.contributor_id)
        .await
        .map_err(|e| {
            error!("Failed to get contributor: {}", e);
            ApiError::InternalError(format!("Database error: {}", e))
        })?
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "Contributor not found: {}",
                evaluation.contributor_id
            ))
        })?;

    // Apply custom delta
    let credit_before = contributor.credit_score;
    let credit_after = apply_credit(credit_before, req.delta);

    // Update credit score
    update_credit_score(&state.db_pool, contributor.id, credit_after)
        .await
        .map_err(|e| {
            error!("Failed to update credit score: {}", e);
            ApiError::InternalError(format!("Database error: {}", e))
        })?;

    // Log credit event with maintainer override
    insert_credit_event(
        &state.db_pool,
        contributor.id,
        "evaluation_overridden",
        req.delta,
        credit_before,
        credit_after,
        Some(format!(
            r#"{{"evaluation_id": "{}", "classification": "{}"}}"#,
            evaluation.id, evaluation.llm_classification
        )),
        Some(req.reason.clone()),
    )
    .await
    .map_err(|e| {
        error!("Failed to insert credit event: {}", e);
        ApiError::InternalError(format!("Database error: {}", e))
    })?;

    // Override evaluation
    override_evaluation(&state.db_pool, &eval_id, req.delta, req.reason.clone())
        .await
        .map_err(|e| {
            error!("Failed to override evaluation: {}", e);
            ApiError::InternalError(format!("Database error: {}", e))
        })?;

    info!(
        "Evaluation {} overridden by maintainer for contributor {} with delta {} (reason: {})",
        eval_id, contributor.id, req.delta, req.reason
    );

    Ok((StatusCode::OK, "Evaluation overridden").into_response())
}

/// GET /api/repos/{owner}/{repo}/contributors
/// List contributors with pagination
pub async fn list_contributors(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(pagination): Query<PaginationQuery>,
    Extension(_user): Extension<GithubUser>,
) -> ApiResult<Json<PaginatedResponse<ContributorResponse>>> {
    let offset = (pagination.page - 1) * pagination.per_page;

    // Fetch contributors from database
    let contributors =
        list_contributors_by_repo(&state.db_pool, &owner, &repo, pagination.per_page, offset)
            .await
            .map_err(|e| {
                error!("Failed to list contributors: {}", e);
                ApiError::InternalError(format!("Database error: {}", e))
            })?;

    // Count total contributors
    let total = count_contributors_by_repo(&state.db_pool, &owner, &repo)
        .await
        .map_err(|e| {
            error!("Failed to count contributors: {}", e);
            ApiError::InternalError(format!("Database error: {}", e))
        })?;

    // Convert to response format
    let data: Vec<ContributorResponse> = contributors
        .into_iter()
        .map(|contrib| ContributorResponse {
            id: contrib.id,
            github_user_id: contrib.github_user_id,
            username: format!("user-{}", contrib.github_user_id), // TODO: fetch from GitHub API
            credit_score: contrib.credit_score,
            role: contrib.role,
            is_blacklisted: contrib.is_blacklisted,
            last_activity: contrib.updated_at.to_rfc3339(),
        })
        .collect();

    let total_pages = (total + pagination.per_page - 1) / pagination.per_page;

    Ok(Json(PaginatedResponse {
        data,
        page: pagination.page,
        per_page: pagination.per_page,
        total,
        total_pages,
    }))
}

/// POST /api/repos/{owner}/{repo}/contributors/{user_id}/adjust
/// Manually adjust contributor credit
pub async fn adjust_contributor_credit(
    State(state): State<AppState>,
    Path((owner, repo, user_id)): Path<(String, String, i64)>,
    Extension(_user): Extension<GithubUser>,
    Json(req): Json<AdjustCreditRequest>,
) -> ApiResult<Response> {
    // Get contributor
    let contributor = get_contributor_by_id(&state.db_pool, user_id)
        .await
        .map_err(|e| {
            error!("Failed to get contributor: {}", e);
            ApiError::InternalError(format!("Database error: {}", e))
        })?
        .ok_or_else(|| ApiError::NotFound(format!("Contributor not found: {}", user_id)))?;

    // Verify contributor belongs to this repo
    if contributor.repo_owner != owner || contributor.repo_name != repo {
        return Err(ApiError::NotFound("Contributor not found".to_string()));
    }

    // Apply credit delta
    let credit_before = contributor.credit_score;
    let credit_after = apply_credit(credit_before, req.delta);

    // Update credit score
    update_credit_score(&state.db_pool, contributor.id, credit_after)
        .await
        .map_err(|e| {
            error!("Failed to update credit score: {}", e);
            ApiError::InternalError(format!("Database error: {}", e))
        })?;

    // Log credit event
    insert_credit_event(
        &state.db_pool,
        contributor.id,
        "manual_adjustment",
        req.delta,
        credit_before,
        credit_after,
        None,
        Some(req.reason.clone()),
    )
    .await
    .map_err(|e| {
        error!("Failed to insert credit event: {}", e);
        ApiError::InternalError(format!("Database error: {}", e))
    })?;

    info!(
        "Credit manually adjusted for contributor {} by maintainer: delta {} (reason: {})",
        contributor.id, req.delta, req.reason
    );

    Ok((StatusCode::OK, "Credit adjusted").into_response())
}

/// POST /api/repos/{owner}/{repo}/contributors/{user_id}/blacklist
/// Toggle contributor blacklist status
pub async fn toggle_contributor_blacklist(
    State(state): State<AppState>,
    Path((owner, repo, user_id)): Path<(String, String, i64)>,
    Extension(_user): Extension<GithubUser>,
) -> ApiResult<Response> {
    // Get contributor
    let contributor = get_contributor_by_id(&state.db_pool, user_id)
        .await
        .map_err(|e| {
            error!("Failed to get contributor: {}", e);
            ApiError::InternalError(format!("Database error: {}", e))
        })?
        .ok_or_else(|| ApiError::NotFound(format!("Contributor not found: {}", user_id)))?;

    // Verify contributor belongs to this repo
    if contributor.repo_owner != owner || contributor.repo_name != repo {
        return Err(ApiError::NotFound("Contributor not found".to_string()));
    }

    // Toggle blacklist status
    let new_status = !contributor.is_blacklisted;
    set_blacklisted(&state.db_pool, contributor.id, new_status)
        .await
        .map_err(|e| {
            error!("Failed to set blacklist status: {}", e);
            ApiError::InternalError(format!("Database error: {}", e))
        })?;

    // Log credit event
    let event_type = if new_status {
        "blacklist_added"
    } else {
        "blacklist_removed"
    };
    insert_credit_event(
        &state.db_pool,
        contributor.id,
        event_type,
        0,
        contributor.credit_score,
        contributor.credit_score,
        None,
        Some(format!(
            "Blacklist toggled by maintainer to: {}",
            new_status
        )),
    )
    .await
    .map_err(|e| {
        error!("Failed to insert credit event: {}", e);
        ApiError::InternalError(format!("Database error: {}", e))
    })?;

    info!(
        "Blacklist status toggled for contributor {}: {}",
        contributor.id, new_status
    );

    Ok((
        StatusCode::OK,
        format!("Blacklist status set to: {}", new_status),
    )
        .into_response())
}

/// GET /api/repos/{owner}/{repo}/events
/// List credit events with pagination and filters
pub async fn list_credit_events(
    State(state): State<AppState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(filter): Query<EventsFilterQuery>,
    Extension(_user): Extension<GithubUser>,
) -> ApiResult<Json<PaginatedResponse<CreditEventResponse>>> {
    let offset = (filter.page - 1) * filter.per_page;

    // Fetch events from database
    let events = list_events_by_repo(
        &state.db_pool,
        &owner,
        &repo,
        filter.contributor_id,
        filter.event_type.as_deref(),
        filter.per_page,
        offset,
    )
    .await
    .map_err(|e| {
        error!("Failed to list events: {}", e);
        ApiError::InternalError(format!("Database error: {}", e))
    })?;

    // Count total events
    let total = count_events_by_repo(
        &state.db_pool,
        &owner,
        &repo,
        filter.contributor_id,
        filter.event_type.as_deref(),
    )
    .await
    .map_err(|e| {
        error!("Failed to count events: {}", e);
        ApiError::InternalError(format!("Database error: {}", e))
    })?;

    // Convert to response format
    let data: Vec<CreditEventResponse> = events
        .into_iter()
        .map(|event| CreditEventResponse {
            id: event.id,
            contributor_id: event.contributor_id,
            event_type: event.event_type,
            delta: event.delta,
            credit_before: event.credit_before,
            credit_after: event.credit_after,
            llm_evaluation: event.llm_evaluation,
            maintainer_override: event.maintainer_override,
            created_at: event.created_at.to_rfc3339(),
        })
        .collect();

    let total_pages = (total + filter.per_page - 1) / filter.per_page;

    Ok(Json(PaginatedResponse {
        data,
        page: filter.page,
        per_page: filter.per_page,
        total,
        total_pages,
    }))
}
