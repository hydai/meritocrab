use meritocrab_core::config::QualityLevel;
use std::sync::Arc;

use crate::claude::ClaudeEvaluator;
use crate::config::LlmConfig;
use crate::mock::MockEvaluator;
use crate::openai::OpenAiEvaluator;
use crate::traits::{LlmError, LlmEvaluator};

/// Create an LLM evaluator from configuration
pub fn create_evaluator(config: &LlmConfig) -> Result<Arc<dyn LlmEvaluator>, LlmError> {
    match config {
        LlmConfig::Claude {
            api_key,
            model,
            base_url,
        } => {
            let evaluator = if let Some(url) = base_url {
                ClaudeEvaluator::with_base_url(api_key.clone(), model.clone(), url.clone())
            } else {
                ClaudeEvaluator::with_model(api_key.clone(), model.clone())
            };
            Ok(Arc::new(evaluator))
        }
        LlmConfig::OpenAi {
            api_key,
            model,
            base_url,
        } => {
            let evaluator = if let Some(url) = base_url {
                OpenAiEvaluator::with_base_url(api_key.clone(), model.clone(), url.clone())
            } else {
                OpenAiEvaluator::with_model(api_key.clone(), model.clone())
            };
            Ok(Arc::new(evaluator))
        }
        LlmConfig::Mock {
            default_classification,
        } => {
            let evaluator = if let Some(classification_str) = default_classification {
                let quality = parse_quality_level(classification_str)?;
                MockEvaluator::with_default(quality)
            } else {
                MockEvaluator::new()
            };
            Ok(Arc::new(evaluator))
        }
    }
}

/// Parse quality level from string
fn parse_quality_level(s: &str) -> Result<QualityLevel, LlmError> {
    match s.to_lowercase().as_str() {
        "spam" => Ok(QualityLevel::Spam),
        "low" | "low_quality" => Ok(QualityLevel::Low),
        "acceptable" => Ok(QualityLevel::Acceptable),
        "high" | "high_quality" => Ok(QualityLevel::High),
        _ => Err(LlmError::ConfigError(format!(
            "Invalid quality level: {}",
            s
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_evaluator_mock() {
        let config = LlmConfig::Mock {
            default_classification: None,
        };
        let evaluator = create_evaluator(&config);
        assert!(evaluator.is_ok());
    }

    #[test]
    fn test_create_evaluator_mock_with_default() {
        let config = LlmConfig::Mock {
            default_classification: Some("high".to_string()),
        };
        let evaluator = create_evaluator(&config);
        assert!(evaluator.is_ok());
    }

    #[test]
    fn test_create_evaluator_claude() {
        let config = LlmConfig::Claude {
            api_key: "test-key".to_string(),
            model: "claude-3-5-sonnet-20241022".to_string(),
            base_url: None,
        };
        let evaluator = create_evaluator(&config);
        assert!(evaluator.is_ok());
    }

    #[test]
    fn test_create_evaluator_openai() {
        let config = LlmConfig::OpenAi {
            api_key: "test-key".to_string(),
            model: "gpt-4o".to_string(),
            base_url: None,
        };
        let evaluator = create_evaluator(&config);
        assert!(evaluator.is_ok());
    }

    #[test]
    fn test_create_evaluator_with_base_url() {
        let config = LlmConfig::Claude {
            api_key: "test-key".to_string(),
            model: "claude-3-5-sonnet-20241022".to_string(),
            base_url: Some("https://custom.api.com".to_string()),
        };
        let evaluator = create_evaluator(&config);
        assert!(evaluator.is_ok());
    }

    #[test]
    fn test_parse_quality_level() {
        assert_eq!(parse_quality_level("spam").unwrap(), QualityLevel::Spam);
        assert_eq!(parse_quality_level("low").unwrap(), QualityLevel::Low);
        assert_eq!(
            parse_quality_level("acceptable").unwrap(),
            QualityLevel::Acceptable
        );
        assert_eq!(parse_quality_level("high").unwrap(), QualityLevel::High);
    }

    #[test]
    fn test_parse_quality_level_case_insensitive() {
        assert_eq!(parse_quality_level("SPAM").unwrap(), QualityLevel::Spam);
        assert_eq!(parse_quality_level("High").unwrap(), QualityLevel::High);
    }

    #[test]
    fn test_parse_quality_level_invalid() {
        assert!(parse_quality_level("invalid").is_err());
    }
}
