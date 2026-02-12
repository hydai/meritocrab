use thiserror::Error;

/// GitHub crate error types
#[derive(Debug, Error)]
pub enum GithubError {
    #[error("HMAC verification failed: {0}")]
    HmacVerificationFailed(String),

    #[error("Missing required header: {0}")]
    MissingHeader(String),

    #[error("Invalid signature format: {0}")]
    InvalidSignatureFormat(String),

    #[error("GitHub API error: {0}")]
    ApiError(String),

    #[error("Authentication error: {0}")]
    AuthError(String),

    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Octocrab error: {0}")]
    OctocrabError(#[from] octocrab::Error),
}

pub type GithubResult<T> = Result<T, GithubError>;
