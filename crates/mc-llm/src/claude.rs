use async_trait::async_trait;
use reqwest::Client;
use mc_core::config::QualityLevel;
use serde::{Deserialize, Serialize};

use crate::prompt::{build_user_prompt, system_prompt};
use crate::traits::{EvalContext, Evaluation, LlmError, LlmEvaluator};

/// Claude API evaluator
#[derive(Debug, Clone)]
pub struct ClaudeEvaluator {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl ClaudeEvaluator {
    /// Create a new Claude evaluator
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model: "claude-3-5-sonnet-20241022".to_string(),
            base_url: "https://api.anthropic.com/v1/messages".to_string(),
        }
    }

    /// Create a Claude evaluator with custom model
    pub fn with_model(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            base_url: "https://api.anthropic.com/v1/messages".to_string(),
        }
    }

    /// Create a Claude evaluator with custom base URL (for testing)
    pub fn with_base_url(api_key: String, model: String, base_url: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            base_url,
        }
    }

    /// Parse classification string to QualityLevel
    fn parse_classification(s: &str) -> Result<QualityLevel, LlmError> {
        match s.to_lowercase().as_str() {
            "spam" => Ok(QualityLevel::Spam),
            "low" | "low_quality" => Ok(QualityLevel::Low),
            "acceptable" => Ok(QualityLevel::Acceptable),
            "high" | "high_quality" => Ok(QualityLevel::High),
            _ => Err(LlmError::InvalidClassification(s.to_string())),
        }
    }
}

#[derive(Debug, Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<ClaudeMessage>,
}

#[derive(Debug, Serialize)]
struct ClaudeMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeContent>,
}

#[derive(Debug, Deserialize)]
struct ClaudeContent {
    text: String,
}

#[derive(Debug, Deserialize)]
struct LlmResponse {
    classification: String,
    confidence: f64,
    reasoning: String,
}

#[async_trait]
impl LlmEvaluator for ClaudeEvaluator {
    async fn evaluate(&self, content: &str, context: &EvalContext) -> Result<Evaluation, LlmError> {
        let user_prompt = build_user_prompt(content, context);

        let request = ClaudeRequest {
            model: self.model.clone(),
            max_tokens: 1024,
            system: system_prompt().to_string(),
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: user_prompt,
            }],
        };

        let response = self
            .client
            .post(&self.base_url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            return Err(match status.as_u16() {
                401 => LlmError::AuthError,
                429 => LlmError::RateLimitError,
                _ => LlmError::ApiError(format!("HTTP {}: {}", status, error_text)),
            });
        }

        let claude_response: ClaudeResponse = response
            .json()
            .await
            .map_err(|e| LlmError::ParseError(format!("Failed to parse Claude response: {}", e)))?;

        let text = claude_response
            .content
            .first()
            .ok_or_else(|| LlmError::ParseError("Empty response from Claude".to_string()))?
            .text
            .clone();

        // Try to extract JSON from the response
        let json_start = text.find('{').unwrap_or(0);
        let json_end = text.rfind('}').map(|i| i + 1).unwrap_or(text.len());
        let json_text = &text[json_start..json_end];

        let llm_response: LlmResponse = serde_json::from_str(json_text)
            .map_err(|e| LlmError::ParseError(format!("Failed to parse LLM JSON: {}", e)))?;

        let classification = Self::parse_classification(&llm_response.classification)?;

        // Validate confidence is in valid range
        let confidence = llm_response.confidence.clamp(0.0, 1.0);

        Ok(Evaluation::new(
            classification,
            confidence,
            llm_response.reasoning,
        ))
    }

    fn provider_name(&self) -> String {
        "claude".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ContentType;

    #[test]
    fn test_parse_classification() {
        assert_eq!(
            ClaudeEvaluator::parse_classification("spam").unwrap(),
            QualityLevel::Spam
        );
        assert_eq!(
            ClaudeEvaluator::parse_classification("low").unwrap(),
            QualityLevel::Low
        );
        assert_eq!(
            ClaudeEvaluator::parse_classification("acceptable").unwrap(),
            QualityLevel::Acceptable
        );
        assert_eq!(
            ClaudeEvaluator::parse_classification("high").unwrap(),
            QualityLevel::High
        );
    }

    #[test]
    fn test_parse_classification_case_insensitive() {
        assert_eq!(
            ClaudeEvaluator::parse_classification("SPAM").unwrap(),
            QualityLevel::Spam
        );
        assert_eq!(
            ClaudeEvaluator::parse_classification("High_Quality").unwrap(),
            QualityLevel::High
        );
    }

    #[test]
    fn test_parse_classification_invalid() {
        assert!(ClaudeEvaluator::parse_classification("invalid").is_err());
    }

    #[test]
    fn test_claude_evaluator_new() {
        let evaluator = ClaudeEvaluator::new("test-key".to_string());
        assert_eq!(evaluator.api_key, "test-key");
        assert_eq!(evaluator.model, "claude-3-5-sonnet-20241022");
        assert_eq!(evaluator.base_url, "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn test_claude_evaluator_with_model() {
        let evaluator = ClaudeEvaluator::with_model(
            "test-key".to_string(),
            "claude-3-opus-20240229".to_string(),
        );
        assert_eq!(evaluator.model, "claude-3-opus-20240229");
    }

    #[test]
    fn test_claude_request_serialization() {
        let request = ClaudeRequest {
            model: "test-model".to_string(),
            max_tokens: 1024,
            system: "system prompt".to_string(),
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: "test content".to_string(),
            }],
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("test-model"));
        assert!(json.contains("system prompt"));
        assert!(json.contains("test content"));
    }

    #[tokio::test]
    async fn test_claude_evaluator_invalid_api_key() {
        let evaluator = ClaudeEvaluator::new("invalid-key".to_string());
        let context = EvalContext {
            content_type: ContentType::Comment,
            title: None,
            body: "test".to_string(),
            diff_summary: None,
            thread_context: None,
        };

        // This will fail because we're using an invalid API key
        // We can't test real API calls without a valid key, but we can verify the error handling
        let result = evaluator.evaluate("test content", &context).await;
        // Should get an auth error or network error
        assert!(result.is_err());
    }
}
