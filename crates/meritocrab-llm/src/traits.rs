use async_trait::async_trait;
use meritocrab_core::config::QualityLevel;
use serde::{Deserialize, Serialize};

/// Context for LLM evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalContext {
    /// Type of content being evaluated
    pub content_type: ContentType,
    /// Title (for PRs)
    pub title: Option<String>,
    /// Body text
    pub body: String,
    /// Diff summary (for PRs)
    pub diff_summary: Option<String>,
    /// Thread context (for comments)
    pub thread_context: Option<String>,
}

/// Type of content being evaluated
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    PullRequest,
    Comment,
    Review,
}

/// Result of LLM evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evaluation {
    /// Quality classification
    pub classification: QualityLevel,
    /// Confidence score (0.0-1.0)
    pub confidence: f64,
    /// Reasoning for the classification
    pub reasoning: String,
}

impl Evaluation {
    /// Create a new evaluation
    pub fn new(classification: QualityLevel, confidence: f64, reasoning: String) -> Self {
        Self {
            classification,
            confidence,
            reasoning,
        }
    }
}

/// LLM evaluator trait for assessing contribution quality
#[async_trait]
pub trait LlmEvaluator: Send + Sync {
    /// Evaluate content and return quality classification with confidence
    async fn evaluate(&self, content: &str, context: &EvalContext) -> Result<Evaluation, LlmError>;

    /// Get the provider name (e.g., "claude", "openai", "mock")
    fn provider_name(&self) -> String;
}

/// Errors that can occur during LLM evaluation
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("API request failed: {0}")]
    ApiError(String),

    #[error("Failed to parse API response: {0}")]
    ParseError(String),

    #[error("Invalid API key or authentication failed")]
    AuthError,

    #[error("Rate limit exceeded")]
    RateLimitError,

    #[error("LLM returned invalid classification: {0}")]
    InvalidClassification(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluation_new() {
        let eval = Evaluation::new(
            QualityLevel::High,
            0.95,
            "Well-structured PR with clear intent".to_string(),
        );

        assert_eq!(eval.classification, QualityLevel::High);
        assert_eq!(eval.confidence, 0.95);
        assert_eq!(eval.reasoning, "Well-structured PR with clear intent");
    }

    #[test]
    fn test_eval_context_pr() {
        let context = EvalContext {
            content_type: ContentType::PullRequest,
            title: Some("Fix bug in parser".to_string()),
            body: "This fixes the parser bug".to_string(),
            diff_summary: Some("+10 -5".to_string()),
            thread_context: None,
        };

        assert_eq!(context.content_type, ContentType::PullRequest);
        assert!(context.title.is_some());
        assert!(context.diff_summary.is_some());
    }

    #[test]
    fn test_eval_context_comment() {
        let context = EvalContext {
            content_type: ContentType::Comment,
            title: None,
            body: "This looks good to me".to_string(),
            diff_summary: None,
            thread_context: Some("Previous discussion about implementation".to_string()),
        };

        assert_eq!(context.content_type, ContentType::Comment);
        assert!(context.thread_context.is_some());
    }
}
