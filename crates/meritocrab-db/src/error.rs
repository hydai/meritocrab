use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("Database error: {0}")]
    SqlxError(#[from] sqlx::Error),

    #[error("Serialization error: {0}")]
    SerdeError(#[from] serde_json::Error),

    #[error("Contributor not found: github_user_id={0}, repo={1}/{2}")]
    ContributorNotFound(i64, String, String),

    #[error("Credit event not found: id={0}")]
    CreditEventNotFound(i64),

    #[error("Evaluation not found: id={0}")]
    EvaluationNotFound(String),

    #[error("Repo config not found: {0}/{1}")]
    RepoConfigNotFound(String, String),

    #[error("Invalid evaluation status: {0}")]
    InvalidStatus(String),
}

pub type DbResult<T> = Result<T, DbError>;
