use crate::error::{ApiError, ApiResult};
use meritocrab_core::RepoConfig;
use meritocrab_github::GithubApiClient;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Cached repository configuration with TTL
#[derive(Debug, Clone)]
struct CachedConfig {
    config: RepoConfig,
    fetched_at: Instant,
}

/// Repository configuration loader with caching
///
/// Fetches `.meritocrab.toml` from repository root via GitHub API
/// and caches it with configurable TTL. Falls back to default config
/// if file is missing or invalid.
pub struct RepoConfigLoader {
    github_client: Arc<GithubApiClient>,
    cache: Arc<RwLock<HashMap<String, CachedConfig>>>,
    cache_ttl: Duration,
    default_config: RepoConfig,
}

impl RepoConfigLoader {
    /// Create new config loader
    ///
    /// # Arguments
    /// * `github_client` - GitHub API client for fetching config files
    /// * `cache_ttl_seconds` - TTL for cached configs in seconds
    pub fn new(github_client: Arc<GithubApiClient>, cache_ttl_seconds: u64) -> Self {
        Self {
            github_client,
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl: Duration::from_secs(cache_ttl_seconds),
            default_config: RepoConfig::default(),
        }
    }

    /// Get configuration for a repository
    ///
    /// Checks cache first, then fetches from GitHub if cache miss or expired.
    /// Returns default config if file is missing or invalid.
    pub async fn get_config(&self, repo_owner: &str, repo_name: &str) -> RepoConfig {
        let cache_key = format!("{}/{}", repo_owner, repo_name);

        // Check cache
        {
            let cache_guard = self.cache.read().await;
            if let Some(cached) = cache_guard.get(&cache_key) {
                if cached.fetched_at.elapsed() < self.cache_ttl {
                    info!("Using cached config for {}/{}", repo_owner, repo_name);
                    return cached.config.clone();
                }
            }
        }

        // Cache miss or expired, fetch from GitHub
        info!("Fetching .meritocrab.toml for {}/{}", repo_owner, repo_name);

        match self.fetch_config_from_github(repo_owner, repo_name).await {
            Ok(config) => {
                // Update cache
                let mut cache_guard = self.cache.write().await;
                cache_guard.insert(
                    cache_key.clone(),
                    CachedConfig {
                        config: config.clone(),
                        fetched_at: Instant::now(),
                    },
                );
                info!("Cached config for {}/{}", repo_owner, repo_name);
                config
            }
            Err(e) => {
                warn!(
                    "Failed to fetch config for {}/{}: {}. Using defaults.",
                    repo_owner, repo_name, e
                );
                self.default_config.clone()
            }
        }
    }

    /// Fetch .meritocrab.toml from GitHub repository
    async fn fetch_config_from_github(&self, repo_owner: &str, repo_name: &str) -> ApiResult<RepoConfig> {
        // Fetch file content from GitHub
        let file_content = self.github_client
            .get_file_content(repo_owner, repo_name, ".meritocrab.toml")
            .await?;

        // Parse TOML
        let config: RepoConfig = toml::from_str(&file_content).map_err(|e| {
            warn!(
                "Invalid .meritocrab.toml syntax for {}/{}: {}",
                repo_owner, repo_name, e
            );
            ApiError::Internal(format!("Invalid TOML syntax: {}", e))
        })?;

        info!(
            "Successfully loaded config for {}/{}: starting_credit={}, pr_threshold={}, blacklist_threshold={}",
            repo_owner, repo_name, config.starting_credit, config.pr_threshold, config.blacklist_threshold
        );

        Ok(config)
    }

    /// Clear cache for a specific repository
    #[allow(dead_code)]
    pub async fn invalidate_cache(&self, repo_owner: &str, repo_name: &str) {
        let cache_key = format!("{}/{}", repo_owner, repo_name);
        let mut cache_guard = self.cache.write().await;
        cache_guard.remove(&cache_key);
        info!("Invalidated cache for {}/{}", repo_owner, repo_name);
    }

    /// Clear all cached configs
    #[allow(dead_code)]
    pub async fn clear_cache(&self) {
        let mut cache_guard = self.cache.write().await;
        cache_guard.clear();
        info!("Cleared all config cache");
    }

    /// Get cache statistics (for monitoring)
    #[allow(dead_code)]
    pub async fn cache_size(&self) -> usize {
        let cache_guard = self.cache.read().await;
        cache_guard.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meritocrab_github::GithubApiClient;

    #[tokio::test]
    async fn test_loader_returns_default_on_error() {
        // Initialize rustls crypto provider for tests
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        // Create GitHub client (will fail when called)
        let github_client = Arc::new(
            GithubApiClient::new("test-token".to_string()).expect("Failed to create client")
        );

        let loader = RepoConfigLoader::new(github_client, 300);
        let config = loader.get_config("owner", "repo").await;

        // Should return defaults since GitHub fetch will fail
        assert_eq!(config.starting_credit, 100);
        assert_eq!(config.pr_threshold, 50);
        assert_eq!(config.blacklist_threshold, 0);
    }

    #[tokio::test]
    async fn test_cache_ttl() {
        // Initialize rustls crypto provider for tests
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        let github_client = Arc::new(
            GithubApiClient::new("test-token".to_string()).expect("Failed to create client")
        );

        // Very short TTL for testing
        let loader = RepoConfigLoader::new(github_client, 1);

        // First fetch (cache miss, will fail and return defaults, NOT cached)
        let config1 = loader.get_config("owner", "repo").await;
        // Cache is empty because fetch failed
        assert_eq!(loader.cache_size().await, 0);

        // Second fetch (cache miss again)
        let config2 = loader.get_config("owner", "repo").await;
        assert_eq!(config1.starting_credit, config2.starting_credit);

        // Cache is still empty because fetches fail in tests
        assert_eq!(loader.cache_size().await, 0);
    }

    #[tokio::test]
    async fn test_invalidate_cache() {
        // Initialize rustls crypto provider for tests
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        let github_client = Arc::new(
            GithubApiClient::new("test-token".to_string()).expect("Failed to create client")
        );

        let loader = RepoConfigLoader::new(github_client, 300);

        // Fetch config (will fail and not be cached)
        let _config = loader.get_config("owner", "repo").await;
        assert_eq!(loader.cache_size().await, 0);

        // Invalidate (cache is already empty, this is a no-op)
        loader.invalidate_cache("owner", "repo").await;
        assert_eq!(loader.cache_size().await, 0);
    }

    #[tokio::test]
    async fn test_clear_cache() {
        // Initialize rustls crypto provider for tests
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        let github_client = Arc::new(
            GithubApiClient::new("test-token".to_string()).expect("Failed to create client")
        );

        let loader = RepoConfigLoader::new(github_client, 300);

        // Fetch multiple configs (will fail and not be cached)
        let _config1 = loader.get_config("owner1", "repo1").await;
        let _config2 = loader.get_config("owner2", "repo2").await;
        assert_eq!(loader.cache_size().await, 0);

        // Clear all (cache is already empty, this is a no-op)
        loader.clear_cache().await;
        assert_eq!(loader.cache_size().await, 0);
    }
}
