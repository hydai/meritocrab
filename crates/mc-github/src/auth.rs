use crate::error::{GithubError, GithubResult};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// GitHub App authentication configuration
#[derive(Clone)]
pub struct GithubAppAuth {
    app_id: i64,
    private_key: String,
}

impl GithubAppAuth {
    /// Create new GitHub App authentication
    pub fn new(app_id: i64, private_key: String) -> Self {
        Self {
            app_id,
            private_key,
        }
    }

    /// Get the app ID
    pub fn app_id(&self) -> i64 {
        self.app_id
    }

    /// Get the private key (exposed for JWT signing)
    pub fn private_key(&self) -> &str {
        &self.private_key
    }

    /// Generate a JWT token for GitHub App authentication
    ///
    /// GitHub requires JWTs to be signed with RS256 and have specific claims:
    /// - iat: issued at time (current time)
    /// - exp: expiration time (max 10 minutes from iat)
    /// - iss: issuer (the app ID)
    ///
    /// Note: This is a placeholder that returns the necessary structure.
    /// In production, use a proper JWT library like `jsonwebtoken` to sign with RS256.
    pub fn generate_jwt(&self) -> GithubResult<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| GithubError::AuthError(format!("System time error: {}", e)))?
            .as_secs() as i64;

        let claims = JwtClaims {
            iat: now,
            exp: now + 600, // 10 minutes (max allowed by GitHub)
            iss: self.app_id.to_string(),
        };

        // In production, this would use jsonwebtoken crate with RS256
        // For now, return a placeholder that indicates what needs to be done
        let jwt_payload = format!(
            "PLACEHOLDER_JWT_FOR_APP_{}_AT_{}",
            self.app_id, claims.iat
        );

        Ok(jwt_payload)
    }
}

/// JWT claims for GitHub App authentication
#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    /// Issued at time (Unix timestamp)
    iat: i64,
    /// Expiration time (Unix timestamp)
    exp: i64,
    /// Issuer (GitHub App ID)
    iss: String,
}

/// Installation token for authenticating as a GitHub App installation
#[derive(Debug, Clone)]
pub struct InstallationToken {
    token: String,
    expires_at: SystemTime,
}

impl InstallationToken {
    /// Create new installation token
    pub fn new(token: String, expires_at: SystemTime) -> Self {
        Self {
            token,
            expires_at,
        }
    }

    /// Get the token value
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Check if token is expired
    pub fn is_expired(&self) -> bool {
        SystemTime::now() >= self.expires_at
    }

    /// Check if token will expire soon (within 5 minutes)
    pub fn is_expiring_soon(&self) -> bool {
        if let Ok(duration) = self.expires_at.duration_since(SystemTime::now()) {
            duration < Duration::from_secs(300) // 5 minutes
        } else {
            true // Already expired
        }
    }
}

/// Installation token manager that handles caching and refreshing
pub struct InstallationTokenManager {
    auth: GithubAppAuth,
    cached_token: Option<InstallationToken>,
}

impl InstallationTokenManager {
    /// Create new installation token manager
    pub fn new(auth: GithubAppAuth) -> Self {
        Self {
            auth,
            cached_token: None,
        }
    }

    /// Get a valid installation token, refreshing if necessary
    ///
    /// This method would:
    /// 1. Check if cached token exists and is still valid
    /// 2. If not, generate a new JWT
    /// 3. Use JWT to request installation token from GitHub API
    /// 4. Cache and return the new token
    pub async fn get_token(&mut self, installation_id: i64) -> GithubResult<String> {
        // Check if we have a cached token that's still valid
        if let Some(ref token) = self.cached_token {
            if !token.is_expiring_soon() {
                return Ok(token.token().to_string());
            }
        }

        // Need to refresh token
        self.refresh_token(installation_id).await
    }

    /// Refresh the installation token
    async fn refresh_token(&mut self, installation_id: i64) -> GithubResult<String> {
        // Generate JWT for app authentication
        let _jwt = self.auth.generate_jwt()?;

        // In production, this would:
        // 1. Use the JWT to call GitHub API: POST /app/installations/{installation_id}/access_tokens
        // 2. Parse the response to get token and expires_at
        // 3. Cache the token
        //
        // For now, return a placeholder
        let token_value = format!("ghs_installation_token_for_{}", installation_id);
        let expires_at = SystemTime::now() + Duration::from_secs(3600); // 1 hour

        let token = InstallationToken::new(token_value.clone(), expires_at);
        self.cached_token = Some(token);

        Ok(token_value)
    }

    /// Clear cached token (useful for testing or forcing refresh)
    pub fn clear_cache(&mut self) {
        self.cached_token = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_app_auth_new() {
        let auth = GithubAppAuth::new(12345, "private-key".to_string());
        assert_eq!(auth.app_id(), 12345);
        assert_eq!(auth.private_key(), "private-key");
    }

    #[test]
    fn test_generate_jwt() {
        let auth = GithubAppAuth::new(12345, "private-key".to_string());
        let jwt = auth.generate_jwt();
        assert!(jwt.is_ok());
        let jwt_str = jwt.unwrap();
        assert!(jwt_str.contains("12345"));
    }

    #[test]
    fn test_installation_token_is_expired() {
        let expired_time = SystemTime::now() - Duration::from_secs(60);
        let token = InstallationToken::new("token".to_string(), expired_time);
        assert!(token.is_expired());
    }

    #[test]
    fn test_installation_token_not_expired() {
        let future_time = SystemTime::now() + Duration::from_secs(3600);
        let token = InstallationToken::new("token".to_string(), future_time);
        assert!(!token.is_expired());
    }

    #[test]
    fn test_installation_token_is_expiring_soon() {
        let soon_time = SystemTime::now() + Duration::from_secs(120); // 2 minutes
        let token = InstallationToken::new("token".to_string(), soon_time);
        assert!(token.is_expiring_soon());
    }

    #[test]
    fn test_installation_token_not_expiring_soon() {
        let future_time = SystemTime::now() + Duration::from_secs(3600); // 1 hour
        let token = InstallationToken::new("token".to_string(), future_time);
        assert!(!token.is_expiring_soon());
    }

    #[tokio::test]
    async fn test_installation_token_manager() {
        let auth = GithubAppAuth::new(12345, "private-key".to_string());
        let mut manager = InstallationTokenManager::new(auth);

        let token = manager.get_token(67890).await;
        assert!(token.is_ok());
        assert!(token.unwrap().contains("67890"));
    }

    #[tokio::test]
    async fn test_installation_token_manager_caching() {
        let auth = GithubAppAuth::new(12345, "private-key".to_string());
        let mut manager = InstallationTokenManager::new(auth);

        // First call should create token
        let token1 = manager.get_token(67890).await.unwrap();

        // Second call should return cached token
        let token2 = manager.get_token(67890).await.unwrap();

        assert_eq!(token1, token2);
    }

    #[tokio::test]
    async fn test_installation_token_manager_clear_cache() {
        let auth = GithubAppAuth::new(12345, "private-key".to_string());
        let mut manager = InstallationTokenManager::new(auth);

        // Get initial token
        let _token1 = manager.get_token(67890).await.unwrap();

        // Clear cache
        manager.clear_cache();

        // Should refresh token
        let token2 = manager.get_token(67890).await.unwrap();
        assert!(token2.contains("67890"));
    }
}
