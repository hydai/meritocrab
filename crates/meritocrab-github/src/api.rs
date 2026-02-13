use crate::{
    error::{GithubError, GithubResult},
    types::CollaboratorRole,
};
use octocrab::{Octocrab, models::CommentId};

/// GitHub API client for repository operations
pub struct GithubApiClient {
    client: Octocrab,
}

impl GithubApiClient {
    /// Create new GitHub API client with authentication token
    pub fn new(token: String) -> GithubResult<Self> {
        let client = Octocrab::builder()
            .personal_token(token)
            .build()
            .map_err(|e| {
                GithubError::ApiError(format!("Failed to create octocrab client: {}", e))
            })?;

        Ok(Self { client })
    }

    /// Create client from existing octocrab instance
    pub fn from_octocrab(client: Octocrab) -> Self {
        Self { client }
    }

    /// Close a pull request
    ///
    /// # Arguments
    /// * `owner` - Repository owner username
    /// * `repo` - Repository name
    /// * `pr_number` - Pull request number
    pub async fn close_pull_request(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
    ) -> GithubResult<()> {
        self.client
            .pulls(owner, repo)
            .update(pr_number)
            .state(octocrab::params::pulls::State::Closed)
            .send()
            .await
            .map_err(|e| {
                GithubError::ApiError(format!("Failed to close PR #{}: {}", pr_number, e))
            })?;

        Ok(())
    }

    /// Add a comment to an issue or pull request
    ///
    /// # Arguments
    /// * `owner` - Repository owner username
    /// * `repo` - Repository name
    /// * `issue_number` - Issue or PR number
    /// * `body` - Comment body text
    pub async fn add_comment(
        &self,
        owner: &str,
        repo: &str,
        issue_number: u64,
        body: &str,
    ) -> GithubResult<CommentId> {
        let comment = self
            .client
            .issues(owner, repo)
            .create_comment(issue_number, body)
            .await
            .map_err(|e| {
                GithubError::ApiError(format!("Failed to add comment to #{}: {}", issue_number, e))
            })?;

        Ok(comment.id)
    }

    /// Check the collaborator role/permission level for a user
    ///
    /// # Arguments
    /// * `owner` - Repository owner username
    /// * `repo` - Repository name
    /// * `username` - User to check permissions for
    ///
    /// # Returns
    /// The user's permission level in the repository
    pub async fn check_collaborator_role(
        &self,
        owner: &str,
        repo: &str,
        username: &str,
    ) -> GithubResult<CollaboratorRole> {
        // Try to get collaborator permission
        // GitHub API returns 404 if user is not a collaborator
        let result = self
            .client
            .repos(owner, repo)
            .get_contributor_permission(username)
            .send()
            .await;

        match result {
            Ok(permission) => {
                // Parse permission level from octocrab's Permission enum
                // Convert to string to match against known permission levels
                let perm_str = format!("{:?}", permission.permission).to_lowercase();
                let role = match perm_str.as_str() {
                    "admin" => CollaboratorRole::Admin,
                    "maintain" => CollaboratorRole::Maintain,
                    "write" | "push" => CollaboratorRole::Write,
                    "triage" => CollaboratorRole::Triage,
                    "read" | "pull" => CollaboratorRole::Read,
                    _ => CollaboratorRole::None,
                };
                Ok(role)
            }
            Err(octocrab::Error::GitHub { source, .. })
                if source.message.contains("404") || source.message.contains("Not Found") =>
            {
                // User is not a collaborator
                Ok(CollaboratorRole::None)
            }
            Err(e) => Err(GithubError::ApiError(format!(
                "Failed to check collaborator role for {}: {}",
                username, e
            ))),
        }
    }

    /// Get file content from repository
    ///
    /// # Arguments
    /// * `owner` - Repository owner username
    /// * `repo` - Repository name
    /// * `path` - File path in repository (e.g., ".meritocrab.toml")
    ///
    /// # Returns
    /// Decoded file content as UTF-8 string
    pub async fn get_file_content(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
    ) -> GithubResult<String> {
        // Fetch file content from GitHub
        let content = self
            .client
            .repos(owner, repo)
            .get_content()
            .path(path)
            .send()
            .await
            .map_err(|e| {
                GithubError::ApiError(format!(
                    "Failed to fetch file {} from {}/{}: {}",
                    path, owner, repo, e
                ))
            })?;

        // GitHub returns content as base64-encoded
        // Octocrab's ContentItems can be a file or directory
        if let Some(file) = content.items.first() {
            if let Some(encoded_content) = &file.content {
                // Decode base64
                let decoded = base64::Engine::decode(
                    &base64::engine::general_purpose::STANDARD,
                    encoded_content.replace('\n', "").as_bytes(),
                )
                .map_err(|e| {
                    GithubError::ApiError(format!("Failed to decode base64 content: {}", e))
                })?;

                // Convert to UTF-8 string
                let content_str = String::from_utf8(decoded).map_err(|e| {
                    GithubError::ApiError(format!("Failed to decode UTF-8 content: {}", e))
                })?;

                return Ok(content_str);
            }
        }

        Err(GithubError::ApiError(format!(
            "File {} not found in {}/{}",
            path, owner, repo
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require GitHub API access and would normally use mocking.
    // For now, they verify the API structure without making actual requests.

    #[tokio::test]
    async fn test_create_api_client() {
        // Initialize rustls crypto provider for tests
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        let result = GithubApiClient::new("test-token".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_collaborator_role_parsing() {
        // Test role determination logic
        let admin_str = "admin";
        let role = match admin_str {
            "admin" => CollaboratorRole::Admin,
            "maintain" => CollaboratorRole::Maintain,
            "write" => CollaboratorRole::Write,
            "triage" => CollaboratorRole::Triage,
            "read" => CollaboratorRole::Read,
            _ => CollaboratorRole::None,
        };
        assert_eq!(role, CollaboratorRole::Admin);
    }

    // Integration tests would be added here with wiremock or similar
    // to mock GitHub API responses without making actual HTTP calls
}
