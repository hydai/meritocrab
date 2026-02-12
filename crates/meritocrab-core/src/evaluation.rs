use serde::{Deserialize, Serialize};

/// Status of a pending evaluation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationStatus {
    /// Pending maintainer review
    Pending,
    /// Approved by maintainer
    Approved,
    /// Overridden by maintainer with different delta
    Overridden,
    /// Automatically applied (high confidence)
    AutoApplied,
}

/// State of an LLM evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationState {
    /// Unique identifier for the evaluation
    pub id: String,

    /// GitHub user being evaluated
    pub github_user_id: i64,

    /// LLM classification result
    pub llm_classification: String,

    /// Confidence score (0.0 to 1.0)
    pub confidence: f64,

    /// Proposed credit delta
    pub proposed_delta: i32,

    /// Current status
    pub status: EvaluationStatus,

    /// Optional maintainer note for overrides
    pub maintainer_note: Option<String>,

    /// Final delta applied (may differ from proposed if overridden)
    pub final_delta: Option<i32>,
}

impl EvaluationState {
    /// Create a new pending evaluation
    pub fn new(
        id: String,
        github_user_id: i64,
        llm_classification: String,
        confidence: f64,
        proposed_delta: i32,
    ) -> Self {
        Self {
            id,
            github_user_id,
            llm_classification,
            confidence,
            proposed_delta,
            status: EvaluationStatus::Pending,
            maintainer_note: None,
            final_delta: None,
        }
    }

    /// Check if evaluation can be auto-applied based on confidence threshold
    pub fn can_auto_apply(&self, confidence_threshold: f64) -> bool {
        self.confidence >= confidence_threshold
    }

    /// Auto-apply the evaluation (marks as auto-applied and sets final delta)
    pub fn auto_apply(mut self) -> Self {
        self.status = EvaluationStatus::AutoApplied;
        self.final_delta = Some(self.proposed_delta);
        self
    }

    /// Approve the evaluation with the proposed delta
    pub fn approve(mut self, maintainer_note: Option<String>) -> Self {
        self.status = EvaluationStatus::Approved;
        self.final_delta = Some(self.proposed_delta);
        self.maintainer_note = maintainer_note;
        self
    }

    /// Override the evaluation with a different delta
    pub fn override_delta(mut self, new_delta: i32, maintainer_note: String) -> Self {
        self.status = EvaluationStatus::Overridden;
        self.final_delta = Some(new_delta);
        self.maintainer_note = Some(maintainer_note);
        self
    }

    /// Get the final delta to apply (if evaluation is completed)
    pub fn get_final_delta(&self) -> Option<i32> {
        self.final_delta
    }

    /// Check if evaluation is completed (not pending)
    pub fn is_completed(&self) -> bool {
        self.status != EvaluationStatus::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_evaluation_state() {
        let eval = EvaluationState::new(
            "eval-123".to_string(),
            12345,
            "high_quality".to_string(),
            0.9,
            15,
        );

        assert_eq!(eval.id, "eval-123");
        assert_eq!(eval.github_user_id, 12345);
        assert_eq!(eval.llm_classification, "high_quality");
        assert_eq!(eval.confidence, 0.9);
        assert_eq!(eval.proposed_delta, 15);
        assert_eq!(eval.status, EvaluationStatus::Pending);
        assert_eq!(eval.maintainer_note, None);
        assert_eq!(eval.final_delta, None);
    }

    #[test]
    fn test_can_auto_apply() {
        let eval = EvaluationState::new(
            "eval-123".to_string(),
            12345,
            "high_quality".to_string(),
            0.9,
            15,
        );

        assert!(eval.can_auto_apply(0.85));
        assert!(eval.can_auto_apply(0.9));
        assert!(!eval.can_auto_apply(0.95));
    }

    #[test]
    fn test_auto_apply() {
        let eval = EvaluationState::new(
            "eval-123".to_string(),
            12345,
            "high_quality".to_string(),
            0.9,
            15,
        );

        let applied = eval.auto_apply();
        assert_eq!(applied.status, EvaluationStatus::AutoApplied);
        assert_eq!(applied.final_delta, Some(15));
        assert!(applied.is_completed());
    }

    #[test]
    fn test_approve() {
        let eval = EvaluationState::new(
            "eval-123".to_string(),
            12345,
            "high_quality".to_string(),
            0.8,
            15,
        );

        let approved = eval.approve(Some("Looks good".to_string()));
        assert_eq!(approved.status, EvaluationStatus::Approved);
        assert_eq!(approved.final_delta, Some(15));
        assert_eq!(approved.maintainer_note, Some("Looks good".to_string()));
        assert!(approved.is_completed());
    }

    #[test]
    fn test_override_delta() {
        let eval = EvaluationState::new(
            "eval-123".to_string(),
            12345,
            "acceptable".to_string(),
            0.7,
            5,
        );

        let overridden = eval.override_delta(10, "Bumping to high quality".to_string());
        assert_eq!(overridden.status, EvaluationStatus::Overridden);
        assert_eq!(overridden.final_delta, Some(10));
        assert_eq!(
            overridden.maintainer_note,
            Some("Bumping to high quality".to_string())
        );
        assert!(overridden.is_completed());
    }

    #[test]
    fn test_get_final_delta() {
        let eval = EvaluationState::new(
            "eval-123".to_string(),
            12345,
            "high_quality".to_string(),
            0.9,
            15,
        );

        assert_eq!(eval.get_final_delta(), None);

        let applied = eval.auto_apply();
        assert_eq!(applied.get_final_delta(), Some(15));
    }

    #[test]
    fn test_is_completed() {
        let eval = EvaluationState::new(
            "eval-123".to_string(),
            12345,
            "high_quality".to_string(),
            0.9,
            15,
        );

        assert!(!eval.is_completed());

        let applied = eval.auto_apply();
        assert!(applied.is_completed());
    }
}
