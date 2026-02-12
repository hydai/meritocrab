use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use sc_github::{CollaboratorRole, GithubApiClient};
use tower_sessions::Session;
use tracing::{error, warn};

use crate::error::ApiError;
use crate::oauth::{get_session_user, GithubUser};
use std::sync::Arc;

/// Auth middleware that checks if user is authenticated
pub async fn require_auth(session: Session, request: Request, next: Next) -> Response {
    match get_session_user(&session).await {
        Ok(_user) => {
            // User is authenticated, proceed
            next.run(request).await
        }
        Err(e) => {
            // User is not authenticated
            warn!("Unauthorized access attempt: {}", e);
            (StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
        }
    }
}

/// Auth middleware that checks if user is a maintainer of the repo
pub async fn require_maintainer(
    State(github_client): State<Arc<GithubApiClient>>,
    session: Session,
    mut request: Request,
    next: Next,
) -> Response {
    // First check if user is authenticated
    let user = match get_session_user(&session).await {
        Ok(user) => user,
        Err(e) => {
            warn!("Unauthorized access attempt: {}", e);
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }
    };

    // Extract repo owner and name from path
    let path = request.uri().path();
    let (repo_owner, repo_name) = match extract_repo_from_path(path) {
        Some(repo) => repo,
        None => {
            error!("Failed to extract repo from path: {}", path);
            return (StatusCode::BAD_REQUEST, "Invalid path").into_response();
        }
    };

    // Check if user is a maintainer of the repo
    match check_user_is_maintainer(&github_client, &user, repo_owner, repo_name).await {
        Ok(true) => {
            // User is a maintainer, store user in request extensions
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Ok(false) => {
            warn!(
                "User {} is not a maintainer of {}/{}",
                user.login, repo_owner, repo_name
            );
            (
                StatusCode::FORBIDDEN,
                "Forbidden: not a maintainer of this repository",
            )
                .into_response()
        }
        Err(e) => {
            error!("Error checking maintainer status: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
        }
    }
}

/// Extract repo owner and name from API path
/// Expects paths like /api/repos/{owner}/{repo}/...
fn extract_repo_from_path(path: &str) -> Option<(&str, &str)> {
    let parts: Vec<&str> = path.split('/').collect();

    // Expected pattern: ["", "api", "repos", "{owner}", "{repo}", ...]
    if parts.len() < 5 || parts[1] != "api" || parts[2] != "repos" {
        return None;
    }

    Some((parts[3], parts[4]))
}

/// Check if user is a maintainer of the repository
async fn check_user_is_maintainer(
    github_client: &GithubApiClient,
    user: &GithubUser,
    repo_owner: &str,
    repo_name: &str,
) -> Result<bool, ApiError> {
    // Use GitHub API to check user's role
    match github_client
        .check_collaborator_role(repo_owner, repo_name, &user.login)
        .await
    {
        Ok(role) => {
            // Maintainers, admins, and write access have permission
            Ok(matches!(
                role,
                CollaboratorRole::Admin | CollaboratorRole::Maintain | CollaboratorRole::Write
            ))
        }
        Err(e) => {
            error!(
                "Failed to check role for user {} in {}/{}: {}",
                user.login, repo_owner, repo_name, e
            );
            // If we can't check the role, deny access
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_repo_from_path() {
        assert_eq!(
            extract_repo_from_path("/api/repos/owner/repo/evaluations"),
            Some(("owner", "repo"))
        );

        assert_eq!(
            extract_repo_from_path("/api/repos/my-org/my-repo/contributors"),
            Some(("my-org", "my-repo"))
        );

        assert_eq!(extract_repo_from_path("/api/repos/owner"), None);

        assert_eq!(extract_repo_from_path("/webhooks/github"), None);

        assert_eq!(extract_repo_from_path("/health"), None);
    }
}
