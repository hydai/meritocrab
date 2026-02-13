use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Contributor database model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contributor {
    pub id: i64,
    pub github_user_id: i64,
    pub repo_owner: String,
    pub repo_name: String,
    pub credit_score: i32,
    pub role: Option<String>,
    pub is_blacklisted: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Raw contributor model from database (with string timestamps)
#[derive(Debug, Clone, FromRow)]
pub(crate) struct ContributorRaw {
    pub id: i64,
    pub github_user_id: i64,
    pub repo_owner: String,
    pub repo_name: String,
    pub credit_score: i32,
    pub role: Option<String>,
    pub is_blacklisted: i32, // SQLite BOOLEAN as INTEGER
    pub created_at: String,
    pub updated_at: String,
}

impl From<ContributorRaw> for Contributor {
    fn from(raw: ContributorRaw) -> Self {
        Self {
            id: raw.id,
            github_user_id: raw.github_user_id,
            repo_owner: raw.repo_owner,
            repo_name: raw.repo_name,
            credit_score: raw.credit_score,
            role: raw.role,
            is_blacklisted: raw.is_blacklisted != 0,
            created_at: DateTime::parse_from_rfc3339(&raw.created_at)
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&raw.updated_at)
                .unwrap()
                .with_timezone(&Utc),
        }
    }
}

/// Credit event database model (immutable audit log)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditEvent {
    pub id: i64,
    pub contributor_id: i64,
    pub event_type: String,
    pub delta: i32,
    pub credit_before: i32,
    pub credit_after: i32,
    pub llm_evaluation: Option<String>,
    pub maintainer_override: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Raw credit event model from database (with string timestamp)
#[derive(Debug, Clone, FromRow)]
pub(crate) struct CreditEventRaw {
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

impl From<CreditEventRaw> for CreditEvent {
    fn from(raw: CreditEventRaw) -> Self {
        Self {
            id: raw.id,
            contributor_id: raw.contributor_id,
            event_type: raw.event_type,
            delta: raw.delta,
            credit_before: raw.credit_before,
            credit_after: raw.credit_after,
            llm_evaluation: raw.llm_evaluation,
            maintainer_override: raw.maintainer_override,
            created_at: DateTime::parse_from_rfc3339(&raw.created_at)
                .unwrap()
                .with_timezone(&Utc),
        }
    }
}

/// Pending evaluation database model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingEvaluation {
    pub id: String,
    pub contributor_id: i64,
    pub repo_owner: String,
    pub repo_name: String,
    pub llm_classification: String,
    pub confidence: f64,
    pub proposed_delta: i32,
    pub status: String,
    pub maintainer_note: Option<String>,
    pub final_delta: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Raw pending evaluation model from database (with string timestamps)
#[derive(Debug, Clone, FromRow)]
pub(crate) struct PendingEvaluationRaw {
    pub id: String,
    pub contributor_id: i64,
    pub repo_owner: String,
    pub repo_name: String,
    pub llm_classification: String,
    pub confidence: f64,
    pub proposed_delta: i32,
    pub status: String,
    pub maintainer_note: Option<String>,
    pub final_delta: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<PendingEvaluationRaw> for PendingEvaluation {
    fn from(raw: PendingEvaluationRaw) -> Self {
        Self {
            id: raw.id,
            contributor_id: raw.contributor_id,
            repo_owner: raw.repo_owner,
            repo_name: raw.repo_name,
            llm_classification: raw.llm_classification,
            confidence: raw.confidence,
            proposed_delta: raw.proposed_delta,
            status: raw.status,
            maintainer_note: raw.maintainer_note,
            final_delta: raw.final_delta,
            created_at: DateTime::parse_from_rfc3339(&raw.created_at)
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&raw.updated_at)
                .unwrap()
                .with_timezone(&Utc),
        }
    }
}

/// Repo config database model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    pub id: i64,
    pub owner: String,
    pub repo: String,
    pub config_json: String,
    pub cached_at: DateTime<Utc>,
    pub ttl: i64,
}

/// Raw repo config model from database (with string timestamp)
#[derive(Debug, Clone, FromRow)]
pub(crate) struct RepoConfigRaw {
    pub id: i64,
    pub owner: String,
    pub repo: String,
    pub config_json: String,
    pub cached_at: String,
    pub ttl: i64,
}

impl From<RepoConfigRaw> for RepoConfig {
    fn from(raw: RepoConfigRaw) -> Self {
        Self {
            id: raw.id,
            owner: raw.owner,
            repo: raw.repo,
            config_json: raw.config_json,
            cached_at: DateTime::parse_from_rfc3339(&raw.cached_at)
                .unwrap()
                .with_timezone(&Utc),
            ttl: raw.ttl,
        }
    }
}
