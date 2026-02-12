use async_trait::async_trait;
use mc_core::config::QualityLevel;

use crate::traits::{EvalContext, Evaluation, LlmError, LlmEvaluator};

/// Mock LLM evaluator for testing that uses keyword matching
#[derive(Debug, Clone)]
pub struct MockEvaluator {
    /// Optional default classification to return
    default_classification: Option<QualityLevel>,
}

impl MockEvaluator {
    /// Create a new mock evaluator with keyword-based classification
    pub fn new() -> Self {
        Self {
            default_classification: None,
        }
    }

    /// Create a mock evaluator that always returns the specified classification
    pub fn with_default(classification: QualityLevel) -> Self {
        Self {
            default_classification: Some(classification),
        }
    }

    /// Classify content based on keywords
    fn classify_by_keywords(&self, content: &str) -> (QualityLevel, f64, String) {
        let lower = content.to_lowercase();

        // Check for spam indicators
        if lower.contains("spam")
            || lower.contains("buy now")
            || lower.contains("click here")
            || lower.contains("free money")
            || lower.contains("viagra")
        {
            return (
                QualityLevel::Spam,
                0.95,
                "Content contains spam indicators".to_string(),
            );
        }

        // Check for low quality indicators
        if lower.contains("low quality")
            || lower.contains("trivial")
            || lower.contains("wip")
            || lower.contains("test commit")
            || lower.len() < 10
        {
            return (
                QualityLevel::Low,
                0.85,
                "Content appears to be low quality or incomplete".to_string(),
            );
        }

        // Check for high quality indicators
        if lower.contains("high quality")
            || lower.contains("well-structured")
            || lower.contains("comprehensive")
            || lower.contains("implements")
            || lower.contains("fixes #")
            || (lower.contains("test") && lower.contains("documentation"))
        {
            return (
                QualityLevel::High,
                0.90,
                "Content demonstrates high quality and thoroughness".to_string(),
            );
        }

        // Default to acceptable
        (
            QualityLevel::Acceptable,
            0.80,
            "Content meets basic quality standards".to_string(),
        )
    }
}

impl Default for MockEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmEvaluator for MockEvaluator {
    async fn evaluate(&self, content: &str, _context: &EvalContext) -> Result<Evaluation, LlmError> {
        // If a default classification is set, use it
        if let Some(classification) = self.default_classification {
            return Ok(Evaluation::new(
                classification,
                0.95,
                format!("Mock evaluation: {:?}", classification),
            ));
        }

        // Otherwise, use keyword-based classification
        let (classification, confidence, reasoning) = self.classify_by_keywords(content);
        Ok(Evaluation::new(classification, confidence, reasoning))
    }

    fn provider_name(&self) -> String {
        "mock".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ContentType;

    #[tokio::test]
    async fn test_mock_evaluator_spam() {
        let evaluator = MockEvaluator::new();
        let context = EvalContext {
            content_type: ContentType::Comment,
            title: None,
            body: "Click here for free money!".to_string(),
            diff_summary: None,
            thread_context: None,
        };

        let result = evaluator.evaluate("Click here for free money!", &context).await;
        assert!(result.is_ok());

        let eval = result.unwrap();
        assert_eq!(eval.classification, QualityLevel::Spam);
        assert!(eval.confidence >= 0.9);
        assert!(eval.reasoning.contains("spam"));
    }

    #[tokio::test]
    async fn test_mock_evaluator_low_quality() {
        let evaluator = MockEvaluator::new();
        let context = EvalContext {
            content_type: ContentType::PullRequest,
            title: Some("WIP".to_string()),
            body: "work in progress".to_string(),
            diff_summary: None,
            thread_context: None,
        };

        let result = evaluator.evaluate("wip - not ready", &context).await;
        assert!(result.is_ok());

        let eval = result.unwrap();
        assert_eq!(eval.classification, QualityLevel::Low);
        assert!(eval.confidence >= 0.8);
    }

    #[tokio::test]
    async fn test_mock_evaluator_acceptable() {
        let evaluator = MockEvaluator::new();
        let context = EvalContext {
            content_type: ContentType::Comment,
            title: None,
            body: "This looks reasonable to me".to_string(),
            diff_summary: None,
            thread_context: None,
        };

        let result = evaluator
            .evaluate("This looks reasonable to me", &context)
            .await;
        assert!(result.is_ok());

        let eval = result.unwrap();
        assert_eq!(eval.classification, QualityLevel::Acceptable);
        assert!(eval.confidence >= 0.7);
    }

    #[tokio::test]
    async fn test_mock_evaluator_high_quality() {
        let evaluator = MockEvaluator::new();
        let context = EvalContext {
            content_type: ContentType::PullRequest,
            title: Some("Implements feature X".to_string()),
            body: "This is a comprehensive implementation with tests and documentation".to_string(),
            diff_summary: Some("+100 -20".to_string()),
            thread_context: None,
        };

        let result = evaluator
            .evaluate(
                "This is a comprehensive implementation with tests and documentation",
                &context,
            )
            .await;
        assert!(result.is_ok());

        let eval = result.unwrap();
        assert_eq!(eval.classification, QualityLevel::High);
        assert!(eval.confidence >= 0.85);
    }

    #[tokio::test]
    async fn test_mock_evaluator_with_default() {
        let evaluator = MockEvaluator::with_default(QualityLevel::High);
        let context = EvalContext {
            content_type: ContentType::Comment,
            title: None,
            body: "Any content".to_string(),
            diff_summary: None,
            thread_context: None,
        };

        let result = evaluator.evaluate("spam content here", &context).await;
        assert!(result.is_ok());

        let eval = result.unwrap();
        // Should return High despite spam content, because default is set
        assert_eq!(eval.classification, QualityLevel::High);
    }

    #[tokio::test]
    async fn test_mock_evaluator_short_content() {
        let evaluator = MockEvaluator::new();
        let context = EvalContext {
            content_type: ContentType::Comment,
            title: None,
            body: "ok".to_string(),
            diff_summary: None,
            thread_context: None,
        };

        let result = evaluator.evaluate("ok", &context).await;
        assert!(result.is_ok());

        let eval = result.unwrap();
        // Short content should be classified as low quality
        assert_eq!(eval.classification, QualityLevel::Low);
    }
}
