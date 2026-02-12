use crate::config::{EventType, QualityLevel, RepoConfig};

/// Calculate the credit delta for a specific event type and quality level
///
/// This is a pure function that takes event type and quality level and returns
/// the credit delta according to the default scoring table.
///
/// # Examples
///
/// ```
/// use meritocrab_core::credit::calculate_delta;
/// use meritocrab_core::config::{EventType, QualityLevel};
///
/// assert_eq!(calculate_delta(EventType::PrOpened, QualityLevel::Spam), -25);
/// assert_eq!(calculate_delta(EventType::PrOpened, QualityLevel::High), 15);
/// assert_eq!(calculate_delta(EventType::Comment, QualityLevel::Spam), -10);
/// ```
pub fn calculate_delta(event_type: EventType, quality: QualityLevel) -> i32 {
    let config = RepoConfig::default();
    calculate_delta_with_config(&config, event_type, quality)
}

/// Calculate the credit delta using a custom configuration
///
/// This function allows using custom scoring configurations instead of defaults.
///
/// # Examples
///
/// ```
/// use meritocrab_core::credit::calculate_delta_with_config;
/// use meritocrab_core::config::{EventType, QualityLevel, RepoConfig};
///
/// let config = RepoConfig::default();
/// assert_eq!(
///     calculate_delta_with_config(&config, EventType::PrOpened, QualityLevel::High),
///     15
/// );
/// ```
pub fn calculate_delta_with_config(
    config: &RepoConfig,
    event_type: EventType,
    quality: QualityLevel,
) -> i32 {
    let delta = config.get_scoring_delta(event_type);
    delta.get(quality)
}

/// Apply a credit delta to the current score, clamping to minimum 0
///
/// This is a pure function that applies a delta to a credit score and ensures
/// the result never goes below 0.
///
/// # Examples
///
/// ```
/// use meritocrab_core::credit::apply_credit;
///
/// assert_eq!(apply_credit(100, -25), 75);
/// assert_eq!(apply_credit(100, 15), 115);
/// assert_eq!(apply_credit(10, -50), 0);  // Clamped to 0
/// assert_eq!(apply_credit(0, -10), 0);   // Already at minimum
/// ```
pub fn apply_credit(current_score: i32, delta: i32) -> i32 {
    (current_score + delta).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test calculate_delta with default config
    #[test]
    fn test_calculate_delta_pr_opened() {
        assert_eq!(calculate_delta(EventType::PrOpened, QualityLevel::Spam), -25);
        assert_eq!(calculate_delta(EventType::PrOpened, QualityLevel::Low), -5);
        assert_eq!(calculate_delta(EventType::PrOpened, QualityLevel::Acceptable), 5);
        assert_eq!(calculate_delta(EventType::PrOpened, QualityLevel::High), 15);
    }

    #[test]
    fn test_calculate_delta_comment() {
        assert_eq!(calculate_delta(EventType::Comment, QualityLevel::Spam), -10);
        assert_eq!(calculate_delta(EventType::Comment, QualityLevel::Low), -2);
        assert_eq!(calculate_delta(EventType::Comment, QualityLevel::Acceptable), 1);
        assert_eq!(calculate_delta(EventType::Comment, QualityLevel::High), 3);
    }

    #[test]
    fn test_calculate_delta_pr_merged() {
        assert_eq!(calculate_delta(EventType::PrMerged, QualityLevel::Spam), 0);
        assert_eq!(calculate_delta(EventType::PrMerged, QualityLevel::Low), 0);
        assert_eq!(calculate_delta(EventType::PrMerged, QualityLevel::Acceptable), 20);
        assert_eq!(calculate_delta(EventType::PrMerged, QualityLevel::High), 20);
    }

    #[test]
    fn test_calculate_delta_review_submitted() {
        assert_eq!(calculate_delta(EventType::ReviewSubmitted, QualityLevel::Spam), 0);
        assert_eq!(calculate_delta(EventType::ReviewSubmitted, QualityLevel::Low), 0);
        assert_eq!(calculate_delta(EventType::ReviewSubmitted, QualityLevel::Acceptable), 5);
        assert_eq!(calculate_delta(EventType::ReviewSubmitted, QualityLevel::High), 5);
    }

    // Test apply_credit
    #[test]
    fn test_apply_credit_positive_delta() {
        assert_eq!(apply_credit(100, 15), 115);
        assert_eq!(apply_credit(50, 20), 70);
        assert_eq!(apply_credit(0, 5), 5);
    }

    #[test]
    fn test_apply_credit_negative_delta() {
        assert_eq!(apply_credit(100, -25), 75);
        assert_eq!(apply_credit(50, -10), 40);
        assert_eq!(apply_credit(30, -30), 0);
    }

    #[test]
    fn test_apply_credit_clamps_to_zero() {
        assert_eq!(apply_credit(10, -50), 0);
        assert_eq!(apply_credit(5, -100), 0);
        assert_eq!(apply_credit(0, -10), 0);
    }

    #[test]
    fn test_apply_credit_boundary_cases() {
        // Exactly zero
        assert_eq!(apply_credit(100, -100), 0);

        // Large positive
        assert_eq!(apply_credit(1000, 500), 1500);

        // Large negative but still positive result
        assert_eq!(apply_credit(1000, -500), 500);
    }

    // Test custom config
    #[test]
    fn test_calculate_delta_with_custom_config() {
        use crate::config::ScoringDelta;

        let mut config = RepoConfig::default();
        config.pr_opened = ScoringDelta {
            spam: -50,
            low: -10,
            acceptable: 10,
            high: 30,
        };

        assert_eq!(
            calculate_delta_with_config(&config, EventType::PrOpened, QualityLevel::Spam),
            -50
        );
        assert_eq!(
            calculate_delta_with_config(&config, EventType::PrOpened, QualityLevel::High),
            30
        );
    }
}
