use serde::{Deserialize, Serialize};

/// Quality level of a contribution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityLevel {
    Spam,
    Low,
    Acceptable,
    High,
}

/// Event type for credit scoring
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    PrOpened,
    Comment,
    PrMerged,
    ReviewSubmitted,
}

/// Scoring delta configuration for a specific event type and quality level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringDelta {
    pub spam: i32,
    pub low: i32,
    pub acceptable: i32,
    pub high: i32,
}

impl ScoringDelta {
    pub fn get(&self, quality: QualityLevel) -> i32 {
        match quality {
            QualityLevel::Spam => self.spam,
            QualityLevel::Low => self.low,
            QualityLevel::Acceptable => self.acceptable,
            QualityLevel::High => self.high,
        }
    }
}

/// Default scoring deltas per the spec
impl Default for ScoringDelta {
    fn default() -> Self {
        Self {
            spam: 0,
            low: 0,
            acceptable: 0,
            high: 0,
        }
    }
}

/// Repository configuration for credit scoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    /// Starting credit for new contributors
    pub starting_credit: i32,

    /// Minimum credit required to open PRs
    pub pr_threshold: i32,

    /// Credit level at which auto-blacklist triggers
    pub blacklist_threshold: i32,

    /// Scoring deltas for PR opened events
    pub pr_opened: ScoringDelta,

    /// Scoring deltas for comment events
    pub comment: ScoringDelta,

    /// Scoring deltas for PR merged events
    pub pr_merged: ScoringDelta,

    /// Scoring deltas for review submitted events
    pub review_submitted: ScoringDelta,
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            starting_credit: 100,
            pr_threshold: 50,
            blacklist_threshold: 0,
            pr_opened: ScoringDelta {
                spam: -25,
                low: -5,
                acceptable: 5,
                high: 15,
            },
            comment: ScoringDelta {
                spam: -10,
                low: -2,
                acceptable: 1,
                high: 3,
            },
            pr_merged: ScoringDelta {
                spam: 0,
                low: 0,
                acceptable: 20,
                high: 20,
            },
            review_submitted: ScoringDelta {
                spam: 0,
                low: 0,
                acceptable: 5,
                high: 5,
            },
        }
    }
}

impl RepoConfig {
    /// Get scoring delta configuration for a specific event type
    pub fn get_scoring_delta(&self, event_type: EventType) -> &ScoringDelta {
        match event_type {
            EventType::PrOpened => &self.pr_opened,
            EventType::Comment => &self.comment,
            EventType::PrMerged => &self.pr_merged,
            EventType::ReviewSubmitted => &self.review_submitted,
        }
    }
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_config_defaults() {
        let config = RepoConfig::default();
        assert_eq!(config.starting_credit, 100);
        assert_eq!(config.pr_threshold, 50);
        assert_eq!(config.blacklist_threshold, 0);
    }

    #[test]
    fn test_scoring_delta_get() {
        let delta = ScoringDelta {
            spam: -25,
            low: -5,
            acceptable: 5,
            high: 15,
        };

        assert_eq!(delta.get(QualityLevel::Spam), -25);
        assert_eq!(delta.get(QualityLevel::Low), -5);
        assert_eq!(delta.get(QualityLevel::Acceptable), 5);
        assert_eq!(delta.get(QualityLevel::High), 15);
    }

    #[test]
    fn test_get_scoring_delta() {
        let config = RepoConfig::default();

        let pr_delta = config.get_scoring_delta(EventType::PrOpened);
        assert_eq!(pr_delta.spam, -25);
        assert_eq!(pr_delta.high, 15);

        let comment_delta = config.get_scoring_delta(EventType::Comment);
        assert_eq!(comment_delta.spam, -10);
        assert_eq!(comment_delta.high, 3);

        let merged_delta = config.get_scoring_delta(EventType::PrMerged);
        assert_eq!(merged_delta.acceptable, 20);

        let review_delta = config.get_scoring_delta(EventType::ReviewSubmitted);
        assert_eq!(review_delta.acceptable, 5);
    }
}
