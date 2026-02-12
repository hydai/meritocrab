use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use tracing::{error, info};

use crate::error::{ApiError, ApiResult};
use crate::state::OAuthConfig;

const SESSION_USER_KEY: &str = "github_user";
const SESSION_CSRF_KEY: &str = "oauth_csrf";

/// GitHub user information from OAuth
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubUser {
    pub id: i64,
    pub login: String,
    pub name: Option<String>,
    pub email: Option<String>,
}

/// OAuth callback query parameters
#[derive(Debug, Deserialize)]
pub struct AuthCallbackParams {
    code: String,
    state: String,
}

/// Generate a random CSRF token
fn generate_csrf_token() -> String {
    use rand::Rng;
    let random_bytes: Vec<u8> = (0..32).map(|_| rand::rng().random()).collect();
    hex::encode(random_bytes)
}

/// GET /auth/github - Redirect to GitHub OAuth
pub async fn github_auth(
    State(config): State<OAuthConfig>,
    session: Session,
) -> ApiResult<Response> {
    let csrf_token = generate_csrf_token();

    // Store CSRF token in session
    session
        .insert(SESSION_CSRF_KEY, csrf_token.clone())
        .await
        .map_err(|e| ApiError::InternalError(format!("Session error: {}", e)))?;

    // Build GitHub OAuth URL manually
    let auth_url = format!(
        "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope={}&state={}",
        config.client_id,
        urlencoding::encode(&config.redirect_url),
        "read:user user:email read:org",
        csrf_token
    );

    info!("Redirecting to GitHub OAuth: {}", auth_url);

    Ok(Redirect::temporary(&auth_url).into_response())
}

/// GET /auth/callback - Handle GitHub OAuth callback
pub async fn github_callback(
    State(config): State<OAuthConfig>,
    Query(params): Query<AuthCallbackParams>,
    session: Session,
) -> ApiResult<Response> {
    // Verify CSRF token
    let stored_csrf: Option<String> = session
        .get(SESSION_CSRF_KEY)
        .await
        .map_err(|e| ApiError::InternalError(format!("Session error: {}", e)))?;

    let stored_csrf = stored_csrf.ok_or_else(|| {
        ApiError::Unauthorized("Invalid OAuth state: no CSRF token in session".to_string())
    })?;

    if stored_csrf != params.state {
        return Err(ApiError::Unauthorized(
            "Invalid OAuth state: CSRF mismatch".to_string(),
        ));
    }

    // Exchange code for token
    let access_token = exchange_code_for_token(&config, &params.code).await?;

    // Fetch user info from GitHub API
    let github_user = fetch_github_user(&access_token).await?;

    info!(
        "User authenticated: {} (ID: {})",
        github_user.login, github_user.id
    );

    // Store user in session
    session
        .insert(SESSION_USER_KEY, github_user)
        .await
        .map_err(|e| ApiError::InternalError(format!("Session error: {}", e)))?;

    // Remove CSRF token from session
    session.remove::<String>(SESSION_CSRF_KEY).await.ok();

    // Redirect to dashboard or home
    Ok(Redirect::to("/").into_response())
}

/// Exchange authorization code for access token
async fn exchange_code_for_token(config: &OAuthConfig, code: &str) -> ApiResult<String> {
    // Make a manual HTTP request to exchange the code for a token
    let client = reqwest::Client::new();

    let body_str = format!(
        "client_id={}&client_secret={}&code={}",
        config.client_id, config.client_secret, code
    );

    let response = client
        .post("https://github.com/login/oauth/access_token")
        .header(header::ACCEPT, "application/json")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body_str)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to exchange code for token: {}", e);
            ApiError::InternalError(format!("GitHub OAuth error: {}", e))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("GitHub OAuth error: {} - {}", status, body);
        return Err(ApiError::Unauthorized(format!(
            "GitHub OAuth returned error: {}",
            status
        )));
    }

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
    }

    let token_response: TokenResponse = response.json().await.map_err(|e| {
        error!("Failed to parse token response: {}", e);
        ApiError::InternalError(format!("Failed to parse OAuth response: {}", e))
    })?;

    Ok(token_response.access_token)
}

/// Fetch GitHub user information using access token
async fn fetch_github_user(access_token: &str) -> ApiResult<GithubUser> {
    let client = reqwest::Client::new();

    let response = client
        .get("https://api.github.com/user")
        .header(header::AUTHORIZATION, format!("Bearer {}", access_token))
        .header(header::USER_AGENT, "meritocrab-app")
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch GitHub user: {}", e);
            ApiError::InternalError(format!("GitHub API error: {}", e))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("GitHub API error: {} - {}", status, body);
        return Err(ApiError::InternalError(format!(
            "GitHub API returned error: {}",
            status
        )));
    }

    let user: GithubUser = response.json().await.map_err(|e| {
        error!("Failed to parse GitHub user response: {}", e);
        ApiError::InternalError(format!("Failed to parse GitHub user: {}", e))
    })?;

    Ok(user)
}

/// Extract authenticated user from session
pub async fn get_session_user(session: &Session) -> ApiResult<GithubUser> {
    let user: Option<GithubUser> = session
        .get(SESSION_USER_KEY)
        .await
        .map_err(|e| ApiError::InternalError(format!("Session error: {}", e)))?;

    user.ok_or_else(|| ApiError::Unauthorized("Not authenticated".to_string()))
}

/// GET /auth/logout - Log out the user
pub async fn logout(session: Session) -> ApiResult<Response> {
    session.delete().await.map_err(|e| {
        error!("Failed to delete session: {}", e);
        ApiError::InternalError(format!("Session error: {}", e))
    })?;

    Ok((StatusCode::OK, "Logged out").into_response())
}
