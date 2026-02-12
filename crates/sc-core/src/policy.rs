use serde::{Deserialize, Serialize};

/// Result of a PR gate check
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GateResult {
    /// PR is allowed
    Allow,
    /// PR is denied
    Deny,
}

/// Check if a PR should be allowed based on credit score and threshold
///
/// Returns `GateResult::Allow` if credit_score >= threshold, otherwise `GateResult::Deny`.
///
/// # Examples
///
/// ```
/// use sc_core::policy::{check_pr_gate, GateResult};
///
/// assert_eq!(check_pr_gate(100, 50), GateResult::Allow);
/// assert_eq!(check_pr_gate(50, 50), GateResult::Allow);  // Equal is allowed
/// assert_eq!(check_pr_gate(49, 50), GateResult::Deny);
/// assert_eq!(check_pr_gate(0, 50), GateResult::Deny);
/// ```
pub fn check_pr_gate(credit_score: i32, threshold: i32) -> GateResult {
    if credit_score >= threshold {
        GateResult::Allow
    } else {
        GateResult::Deny
    }
}

/// Check if a user should be blacklisted based on credit score
///
/// Returns `true` if credit_score <= blacklist_threshold, otherwise `false`.
///
/// # Examples
///
/// ```
/// use sc_core::policy::check_blacklist;
///
/// assert_eq!(check_blacklist(0, 0), true);   // At threshold
/// assert_eq!(check_blacklist(-5, 0), true);  // Below threshold
/// assert_eq!(check_blacklist(1, 0), false);  // Above threshold
/// assert_eq!(check_blacklist(50, 0), false);
/// ```
pub fn check_blacklist(credit_score: i32, blacklist_threshold: i32) -> bool {
    credit_score <= blacklist_threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test check_pr_gate
    #[test]
    fn test_check_pr_gate_allow() {
        assert_eq!(check_pr_gate(100, 50), GateResult::Allow);
        assert_eq!(check_pr_gate(75, 50), GateResult::Allow);
        assert_eq!(check_pr_gate(51, 50), GateResult::Allow);
    }

    #[test]
    fn test_check_pr_gate_allow_at_threshold() {
        assert_eq!(check_pr_gate(50, 50), GateResult::Allow);
        assert_eq!(check_pr_gate(100, 100), GateResult::Allow);
        assert_eq!(check_pr_gate(0, 0), GateResult::Allow);
    }

    #[test]
    fn test_check_pr_gate_deny() {
        assert_eq!(check_pr_gate(49, 50), GateResult::Deny);
        assert_eq!(check_pr_gate(25, 50), GateResult::Deny);
        assert_eq!(check_pr_gate(0, 50), GateResult::Deny);
    }

    #[test]
    fn test_check_pr_gate_boundary_cases() {
        // Zero threshold
        assert_eq!(check_pr_gate(0, 0), GateResult::Allow);
        assert_eq!(check_pr_gate(1, 0), GateResult::Allow);
        assert_eq!(check_pr_gate(-1, 0), GateResult::Deny);

        // High threshold
        assert_eq!(check_pr_gate(200, 100), GateResult::Allow);
        assert_eq!(check_pr_gate(99, 100), GateResult::Deny);
    }

    // Test check_blacklist
    #[test]
    fn test_check_blacklist_at_threshold() {
        assert!(check_blacklist(0, 0));
        assert!(check_blacklist(10, 10));
        assert!(check_blacklist(-5, -5));
    }

    #[test]
    fn test_check_blacklist_below_threshold() {
        assert!(check_blacklist(-1, 0));
        assert!(check_blacklist(-10, 0));
        assert!(check_blacklist(-100, 0));
        assert!(check_blacklist(5, 10));
    }

    #[test]
    fn test_check_blacklist_above_threshold() {
        assert!(!check_blacklist(1, 0));
        assert!(!check_blacklist(50, 0));
        assert!(!check_blacklist(100, 0));
        assert!(!check_blacklist(11, 10));
    }

    #[test]
    fn test_check_blacklist_default_threshold() {
        // Default threshold is 0
        assert!(check_blacklist(0, 0));
        assert!(check_blacklist(-5, 0));
        assert!(!check_blacklist(1, 0));
        assert!(!check_blacklist(50, 0));
    }

    #[test]
    fn test_check_blacklist_boundary_cases() {
        // Negative threshold
        assert!(check_blacklist(-10, -5));
        assert!(check_blacklist(-5, -5));
        assert!(!check_blacklist(-4, -5));

        // Positive threshold
        assert!(check_blacklist(5, 10));
        assert!(check_blacklist(10, 10));
        assert!(!check_blacklist(11, 10));
    }
}
