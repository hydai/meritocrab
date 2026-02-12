use config::{Config, ConfigError, Environment, File};
use mc_core::{RepoConfig, ServerConfig};
use mc_llm::LlmConfig;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Complete application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub github: GithubConfig,
    pub credit: RepoConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default = "default_max_concurrent_llm_evals")]
    pub max_concurrent_llm_evals: usize,
}

fn default_max_concurrent_llm_evals() -> usize {
    10
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

/// GitHub configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubConfig {
    pub app_id: u64,
    pub installation_id: u64,
    pub private_key_path: String,
    pub webhook_secret: String,
    pub api_url: Option<String>,
    /// OAuth client ID for maintainer dashboard
    #[serde(default = "default_oauth_client_id")]
    pub oauth_client_id: String,
    /// OAuth client secret for maintainer dashboard
    #[serde(default = "default_oauth_client_secret")]
    pub oauth_client_secret: String,
    /// OAuth redirect URL
    #[serde(default = "default_oauth_redirect_url")]
    pub oauth_redirect_url: String,
}

fn default_oauth_client_id() -> String {
    "".to_string()
}

fn default_oauth_client_secret() -> String {
    "".to_string()
}

fn default_oauth_redirect_url() -> String {
    "http://localhost:8080/auth/callback".to_string()
}

impl AppConfig {
    /// Load configuration from file and environment variables
    ///
    /// Configuration is loaded in the following order (later sources override earlier ones):
    /// 1. Default values
    /// 2. config.toml file (if present)
    /// 3. Environment variables (prefixed with MERITOCRAB_)
    ///
    /// Environment variables use double underscore for nesting:
    /// - MERITOCRAB_SERVER__HOST=127.0.0.1
    /// - MERITOCRAB_DATABASE__URL=sqlite://db.sqlite
    pub fn load() -> Result<Self, ConfigError> {
        let builder = Config::builder()
            // Start with default values
            .set_default("server.host", "127.0.0.1")?
            .set_default("server.port", 8080)?
            .set_default("database.url", "sqlite://meritocrab.db")?
            .set_default("database.max_connections", 10)?
            .set_default("credit.starting_credit", 100)?
            .set_default("credit.pr_threshold", 50)?
            .set_default("credit.blacklist_threshold", 0)?
            .set_default("credit.pr_opened.spam", -25)?
            .set_default("credit.pr_opened.low", -5)?
            .set_default("credit.pr_opened.acceptable", 5)?
            .set_default("credit.pr_opened.high", 15)?
            .set_default("credit.comment.spam", -10)?
            .set_default("credit.comment.low", -2)?
            .set_default("credit.comment.acceptable", 1)?
            .set_default("credit.comment.high", 3)?
            .set_default("credit.pr_merged.spam", 0)?
            .set_default("credit.pr_merged.low", 0)?
            .set_default("credit.pr_merged.acceptable", 20)?
            .set_default("credit.pr_merged.high", 20)?
            .set_default("credit.review_submitted.spam", 0)?
            .set_default("credit.review_submitted.low", 0)?
            .set_default("credit.review_submitted.acceptable", 5)?
            .set_default("credit.review_submitted.high", 5)?;

        // Try to load config.toml if it exists
        let builder = if Path::new("config.toml").exists() {
            builder.add_source(File::with_name("config"))
        } else {
            builder
        };

        // Override with environment variables
        let builder = builder.add_source(
            Environment::with_prefix("MERITOCRAB")
                .separator("__")
                .try_parsing(true),
        );

        builder.build()?.try_deserialize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_types() {
        // Test that config types can be constructed
        let server_config = ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 8080,
        };
        assert_eq!(server_config.host, "127.0.0.1");
        assert_eq!(server_config.port, 8080);

        let db_config = DatabaseConfig {
            url: "sqlite://test.db".to_string(),
            max_connections: 10,
        };
        assert_eq!(db_config.url, "sqlite://test.db");
        assert_eq!(db_config.max_connections, 10);
    }
}
