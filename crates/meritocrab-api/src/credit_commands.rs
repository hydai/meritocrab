use lazy_static::lazy_static;
use regex::Regex;

/// Parsed /credit command
#[derive(Debug, Clone, PartialEq)]
pub enum CreditCommand {
    /// `/credit check @username`
    Check { username: String },
    /// `/credit override @username +10 "reason"`
    Override {
        username: String,
        delta: i32,
        reason: String,
    },
    /// `/credit blacklist @username`
    Blacklist { username: String },
}

lazy_static! {
    // Match: /credit check @username
    static ref CHECK_REGEX: Regex = Regex::new(r#"(?m)^/credit\s+check\s+@(\w+)\s*$"#).unwrap();

    // Match: /credit override @username +10 "reason" or /credit override @username -20 "reason"
    static ref OVERRIDE_REGEX: Regex = Regex::new(r#"(?m)^/credit\s+override\s+@(\w+)\s+([+-]\d+)\s+"([^"]+)"\s*$"#).unwrap();

    // Match: /credit blacklist @username
    static ref BLACKLIST_REGEX: Regex = Regex::new(r#"(?m)^/credit\s+blacklist\s+@(\w+)\s*$"#).unwrap();
}

/// Parse /credit command from comment body
///
/// Returns Some(CreditCommand) if a valid /credit command is found, None otherwise.
/// If multiple commands are present, only the first one is parsed.
pub fn parse_credit_command(comment_body: &str) -> Option<CreditCommand> {
    // Try to match /credit check @username
    if let Some(captures) = CHECK_REGEX.captures(comment_body) {
        let username = captures.get(1).unwrap().as_str().to_string();
        return Some(CreditCommand::Check { username });
    }

    // Try to match /credit override @username +10 "reason"
    if let Some(captures) = OVERRIDE_REGEX.captures(comment_body) {
        let username = captures.get(1).unwrap().as_str().to_string();
        let delta_str = captures.get(2).unwrap().as_str();
        let reason = captures.get(3).unwrap().as_str().to_string();

        // Parse delta (includes sign)
        if let Ok(delta) = delta_str.parse::<i32>() {
            return Some(CreditCommand::Override {
                username,
                delta,
                reason,
            });
        }
    }

    // Try to match /credit blacklist @username
    if let Some(captures) = BLACKLIST_REGEX.captures(comment_body) {
        let username = captures.get(1).unwrap().as_str().to_string();
        return Some(CreditCommand::Blacklist { username });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_check_command() {
        let comment = "/credit check @user123";
        let cmd = parse_credit_command(comment);
        assert_eq!(
            cmd,
            Some(CreditCommand::Check {
                username: "user123".to_string()
            })
        );
    }

    #[test]
    fn test_parse_check_command_with_whitespace() {
        let comment = "/credit check @user123  ";
        let cmd = parse_credit_command(comment);
        assert_eq!(
            cmd,
            Some(CreditCommand::Check {
                username: "user123".to_string()
            })
        );
    }

    #[test]
    fn test_parse_override_positive() {
        let comment = r#"/credit override @user123 +10 "good first contribution""#;
        let cmd = parse_credit_command(comment);
        assert_eq!(
            cmd,
            Some(CreditCommand::Override {
                username: "user123".to_string(),
                delta: 10,
                reason: "good first contribution".to_string()
            })
        );
    }

    #[test]
    fn test_parse_override_negative() {
        let comment = r#"/credit override @spammer -20 "spam PR""#;
        let cmd = parse_credit_command(comment);
        assert_eq!(
            cmd,
            Some(CreditCommand::Override {
                username: "spammer".to_string(),
                delta: -20,
                reason: "spam PR".to_string()
            })
        );
    }

    #[test]
    fn test_parse_blacklist_command() {
        let comment = "/credit blacklist @badactor";
        let cmd = parse_credit_command(comment);
        assert_eq!(
            cmd,
            Some(CreditCommand::Blacklist {
                username: "badactor".to_string()
            })
        );
    }

    #[test]
    fn test_parse_no_command() {
        let comment = "This is just a regular comment";
        let cmd = parse_credit_command(comment);
        assert_eq!(cmd, None);
    }

    #[test]
    fn test_parse_invalid_command() {
        let comment = "/credit unknown @user";
        let cmd = parse_credit_command(comment);
        assert_eq!(cmd, None);
    }

    #[test]
    fn test_parse_command_in_multi_line_comment() {
        let comment = r#"Some context before

/credit check @user123

Some context after"#;
        let cmd = parse_credit_command(comment);
        assert_eq!(
            cmd,
            Some(CreditCommand::Check {
                username: "user123".to_string()
            })
        );
    }

    #[test]
    fn test_parse_override_with_multiword_reason() {
        let comment = r#"/credit override @user +5 "excellent bug fix with detailed explanation""#;
        let cmd = parse_credit_command(comment);
        assert_eq!(
            cmd,
            Some(CreditCommand::Override {
                username: "user".to_string(),
                delta: 5,
                reason: "excellent bug fix with detailed explanation".to_string()
            })
        );
    }
}
