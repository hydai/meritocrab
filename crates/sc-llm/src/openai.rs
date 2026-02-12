use async_trait::async_trait;
use reqwest::Client;
use sc_core::config::QualityLevel;
use serde::{Deserialize, Serialize};

use crate::prompt::{build_user_prompt, system_prompt};
use crate::traits::{EvalContext, Evaluation, LlmError, LlmEvaluator};

/// OpenAI API evaluator
#[derive(Debug, Clone)]
pub struct OpenAiEvaluator {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAiEvaluator {
    /// Create a new OpenAI evaluator
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model: "gpt-4o".to_string(),
            base_url: "https://api.openai.com/v1/chat/completions".to_string(),
        }
    }

    /// Create an OpenAI evaluator with custom model
    pub fn with_model(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            base_url: "https://api.openai.com/v1/chat/completions".to_string(),
        }
    }

    /// Create an OpenAI evaluator with custom base URL (for testing)
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
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponseMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct LlmResponse {
    classification: String,
    confidence: f64,
    reasoning: String,
}

#[async_trait]
impl LlmEvaluator for OpenAiEvaluator {
    async fn evaluate(&self, content: &str, context: &EvalContext) -> Result<Evaluation, LlmError> {
        let user_prompt = build_user_prompt(content, context);

        let request = OpenAiRequest {
            model: self.model.clone(),
            messages: vec![
                OpenAiMessage {
                    role: "system".to_string(),
                    content: system_prompt().to_string(),
                },
                OpenAiMessage {
                    role: "user".to_string(),
                    content: user_prompt,
                },
            ],
            temperature: 0.3,
            max_tokens: 1024,
        };

        let response = self
            .client
            .post(&self.base_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
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

        let openai_response: OpenAiResponse = response
            .json()
            .await
            .map_err(|e| LlmError::ParseError(format!("Failed to parse OpenAI response: {}", e)))?;

        let text = openai_response
            .choices
            .first()
            .ok_or_else(|| LlmError::ParseError("Empty response from OpenAI".to_string()))?
            .message
            .content
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ContentType;

    #[test]
    fn test_parse_classification() {
        assert_eq!(
            OpenAiEvaluator::parse_classification("spam").unwrap(),
            QualityLevel::Spam
        );
        assert_eq!(
            OpenAiEvaluator::parse_classification("low").unwrap(),
            QualityLevel::Low
        );
        assert_eq!(
            OpenAiEvaluator::parse_classification("acceptable").unwrap(),
            QualityLevel::Acceptable
        );
        assert_eq!(
            OpenAiEvaluator::parse_classification("high").unwrap(),
            QualityLevel::High
        );
    }

    #[test]
    fn test_parse_classification_case_insensitive() {
        assert_eq!(
            OpenAiEvaluator::parse_classification("SPAM").unwrap(),
            QualityLevel::Spam
        );
        assert_eq!(
            OpenAiEvaluator::parse_classification("High_Quality").unwrap(),
            QualityLevel::High
        );
    }

    #[test]
    fn test_parse_classification_invalid() {
        assert!(OpenAiEvaluator::parse_classification("invalid").is_err());
    }

    #[test]
    fn test_openai_evaluator_new() {
        let evaluator = OpenAiEvaluator::new("test-key".to_string());
        assert_eq!(evaluator.api_key, "test-key");
        assert_eq!(evaluator.model, "gpt-4o");
        assert_eq!(
            evaluator.base_url,
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_openai_evaluator_with_model() {
        let evaluator = OpenAiEvaluator::with_model(
            "test-key".to_string(),
            "gpt-4-turbo".to_string(),
        );
        assert_eq!(evaluator.model, "gpt-4-turbo");
    }

    #[test]
    fn test_openai_request_serialization() {
        let request = OpenAiRequest {
            model: "test-model".to_string(),
            messages: vec![
                OpenAiMessage {
                    role: "system".to_string(),
                    content: "system prompt".to_string(),
                },
                OpenAiMessage {
                    role: "user".to_string(),
                    content: "test content".to_string(),
                },
            ],
            temperature: 0.3,
            max_tokens: 1024,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("test-model"));
        assert!(json.contains("system prompt"));
        assert!(json.contains("test content"));
        assert!(json.contains("0.3"));
    }

    #[tokio::test]
    async fn test_openai_evaluator_invalid_api_key() {
        let evaluator = OpenAiEvaluator::new("invalid-key".to_string());
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
