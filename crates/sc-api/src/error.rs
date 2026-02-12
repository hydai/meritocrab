use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use sc_core::CoreError;
use sc_db::DbError;
use sc_github::GithubError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// API error type
#[derive(Debug)]
pub enum ApiError {
    /// Database error
    Database(DbError),

    /// GitHub API error
    Github(GithubError),

    /// Core logic error
    Core(CoreError),

    /// Invalid request payload
    InvalidPayload(String),

    /// Invalid webhook signature (HMAC verification failed)
    InvalidSignature(String),

    /// Internal server error
    Internal(String),

    /// Unauthorized (401)
    Unauthorized(String),

    /// Not found (404)
    NotFound(String),

    /// Bad request (400)
    BadRequest(String),

    /// Forbidden (403)
    Forbidden(String),

    /// Internal error (alias for backward compatibility)
    InternalError(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Database(e) => write!(f, "Database error: {}", e),
            ApiError::Github(e) => write!(f, "GitHub error: {}", e),
            ApiError::Core(e) => write!(f, "Core error: {}", e),
            ApiError::InvalidPayload(msg) => write!(f, "Invalid payload: {}", msg),
            ApiError::InvalidSignature(msg) => write!(f, "Invalid signature: {}", msg),
            ApiError::Internal(msg) => write!(f, "Internal error: {}", msg),
            ApiError::Unauthorized(msg) => write!(f, "Unauthorized: {}", msg),
            ApiError::NotFound(msg) => write!(f, "Not found: {}", msg),
            ApiError::BadRequest(msg) => write!(f, "Bad request: {}", msg),
            ApiError::Forbidden(msg) => write!(f, "Forbidden: {}", msg),
            ApiError::InternalError(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for ApiError {}

/// Error response JSON structure
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_type, message) = match &self {
            ApiError::Database(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                e.to_string(),
            ),
            ApiError::Github(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "github_error",
                e.to_string(),
            ),
            ApiError::Core(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "core_error",
                e.to_string(),
            ),
            ApiError::InvalidPayload(msg) => (
                StatusCode::BAD_REQUEST,
                "invalid_payload",
                msg.clone(),
            ),
            ApiError::InvalidSignature(msg) => (
                StatusCode::UNAUTHORIZED,
                "invalid_signature",
                msg.clone(),
            ),
            ApiError::Internal(msg) | ApiError::InternalError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                msg.clone(),
            ),
            ApiError::Unauthorized(msg) => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                msg.clone(),
            ),
            ApiError::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                "not_found",
                msg.clone(),
            ),
            ApiError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                "bad_request",
                msg.clone(),
            ),
            ApiError::Forbidden(msg) => (
                StatusCode::FORBIDDEN,
                "forbidden",
                msg.clone(),
            ),
        };

        let error_response = ErrorResponse {
            error: error_type.to_string(),
            message,
        };

        (status, Json(error_response)).into_response()
    }
}

// Conversions from domain errors to ApiError
impl From<DbError> for ApiError {
    fn from(e: DbError) -> Self {
        ApiError::Database(e)
    }
}

impl From<GithubError> for ApiError {
    fn from(e: GithubError) -> Self {
        ApiError::Github(e)
    }
}

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        ApiError::Core(e)
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError::InvalidPayload(format!("JSON parsing error: {}", e))
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ApiError::InvalidPayload("test error".to_string());
        assert_eq!(err.to_string(), "Invalid payload: test error");
    }

    #[test]
    fn test_error_response_invalid_payload() {
        let err = ApiError::InvalidPayload("bad json".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_error_response_invalid_signature() {
        let err = ApiError::InvalidSignature("signature mismatch".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_error_response_internal() {
        let err = ApiError::Internal("something went wrong".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_from_serde_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("{invalid}").unwrap_err();
        let api_err: ApiError = json_err.into();
        match api_err {
            ApiError::InvalidPayload(msg) => assert!(msg.contains("JSON parsing error")),
            _ => panic!("Expected InvalidPayload error"),
        }
    }
}
