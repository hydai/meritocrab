use serde::{Deserialize, Serialize};

/// LLM provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "lowercase")]
pub enum LlmConfig {
    Claude {
        api_key: String,
        #[serde(default = "default_claude_model")]
        model: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        base_url: Option<String>,
    },
    OpenAi {
        api_key: String,
        #[serde(default = "default_openai_model")]
        model: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        base_url: Option<String>,
    },
    Mock {
        #[serde(skip_serializing_if = "Option::is_none")]
        default_classification: Option<String>,
    },
}

fn default_claude_model() -> String {
    "claude-3-5-sonnet-20241022".to_string()
}

fn default_openai_model() -> String {
    "gpt-4o".to_string()
}

impl Default for LlmConfig {
    fn default() -> Self {
        LlmConfig::Mock {
            default_classification: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_config_default() {
        let config = LlmConfig::default();
        match config {
            LlmConfig::Mock { .. } => {}
            _ => panic!("Default should be Mock"),
        }
    }

    #[test]
    fn test_llm_config_claude_serialization() {
        let config = LlmConfig::Claude {
            api_key: "test-key".to_string(),
            model: "claude-3-opus-20240229".to_string(),
            base_url: None,
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("claude"));
        assert!(json.contains("test-key"));
        assert!(json.contains("claude-3-opus-20240229"));
    }

    #[test]
    fn test_llm_config_openai_serialization() {
        let config = LlmConfig::OpenAi {
            api_key: "test-key".to_string(),
            model: "gpt-4-turbo".to_string(),
            base_url: None,
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("openai"));
        assert!(json.contains("test-key"));
        assert!(json.contains("gpt-4-turbo"));
    }

    #[test]
    fn test_llm_config_mock_serialization() {
        let config = LlmConfig::Mock {
            default_classification: Some("high".to_string()),
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("mock"));
        assert!(json.contains("high"));
    }

    #[test]
    fn test_llm_config_deserialization() {
        let json = r#"{"provider":"claude","api_key":"test","model":"claude-3-5-sonnet-20241022"}"#;
        let config: LlmConfig = serde_json::from_str(json).unwrap();

        match config {
            LlmConfig::Claude { api_key, model, .. } => {
                assert_eq!(api_key, "test");
                assert_eq!(model, "claude-3-5-sonnet-20241022");
            }
            _ => panic!("Expected Claude config"),
        }
    }

    #[test]
    fn test_llm_config_with_base_url() {
        let config = LlmConfig::Claude {
            api_key: "test".to_string(),
            model: "claude-3-5-sonnet-20241022".to_string(),
            base_url: Some("https://custom.api.com".to_string()),
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("https://custom.api.com"));
    }
}
