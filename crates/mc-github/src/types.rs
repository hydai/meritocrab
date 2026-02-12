use serde::{Deserialize, Serialize};

/// GitHub user information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub login: String,
    #[serde(rename = "type")]
    pub user_type: Option<String>,
}

/// GitHub repository information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub id: i64,
    pub name: String,
    pub full_name: String,
    pub owner: User,
}

/// Pull request information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub user: User,
    pub state: String,
    pub merged: Option<bool>,
    pub html_url: String,
}

/// Issue comment information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: i64,
    pub body: String,
    pub user: User,
    pub html_url: String,
}

/// Pull request review information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub id: i64,
    pub body: Option<String>,
    pub user: User,
    pub state: String,
    pub html_url: String,
}

/// Pull request issue information (for comments)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub number: i64,
    pub title: String,
    pub user: User,
    pub pull_request: Option<PullRequestReference>,
}

/// Reference to a pull request from an issue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestReference {
    pub url: String,
}

/// Pull request webhook event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestEvent {
    pub action: String,
    pub number: i64,
    pub pull_request: PullRequest,
    pub repository: Repository,
    pub sender: User,
}

/// Issue comment webhook event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueCommentEvent {
    pub action: String,
    pub issue: Issue,
    pub comment: Comment,
    pub repository: Repository,
    pub sender: User,
}

/// Pull request review webhook event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestReviewEvent {
    pub action: String,
    pub review: Review,
    pub pull_request: PullRequest,
    pub repository: Repository,
    pub sender: User,
}

/// GitHub collaborator permission level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CollaboratorRole {
    Admin,
    Maintain,
    Write,
    Triage,
    Read,
    None,
}

impl CollaboratorRole {
    /// Check if role has write access or higher
    pub fn has_write_access(&self) -> bool {
        matches!(self, Self::Admin | Self::Maintain | Self::Write)
    }

    /// Check if role is maintainer (admin or maintain)
    pub fn is_maintainer(&self) -> bool {
        matches!(self, Self::Admin | Self::Maintain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pull_request_event() {
        let json = r#"{
            "action": "opened",
            "number": 123,
            "pull_request": {
                "number": 123,
                "title": "Test PR",
                "body": "Test body",
                "user": {
                    "id": 12345,
                    "login": "testuser"
                },
                "state": "open",
                "merged": false,
                "html_url": "https://github.com/owner/repo/pull/123"
            },
            "repository": {
                "id": 1,
                "name": "repo",
                "full_name": "owner/repo",
                "owner": {
                    "id": 1,
                    "login": "owner"
                }
            },
            "sender": {
                "id": 12345,
                "login": "testuser"
            }
        }"#;

        let event: PullRequestEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.action, "opened");
        assert_eq!(event.number, 123);
        assert_eq!(event.pull_request.title, "Test PR");
        assert_eq!(event.pull_request.body, Some("Test body".to_string()));
        assert_eq!(event.pull_request.user.login, "testuser");
        assert_eq!(event.pull_request.user.id, 12345);
        assert_eq!(event.repository.owner.login, "owner");
        assert_eq!(event.repository.name, "repo");
    }

    #[test]
    fn test_parse_issue_comment_event() {
        let json = r#"{
            "action": "created",
            "issue": {
                "number": 123,
                "title": "Test Issue",
                "user": {
                    "id": 1,
                    "login": "owner"
                },
                "pull_request": {
                    "url": "https://api.github.com/repos/owner/repo/pulls/123"
                }
            },
            "comment": {
                "id": 456,
                "body": "Test comment",
                "user": {
                    "id": 12345,
                    "login": "testuser"
                },
                "html_url": "https://github.com/owner/repo/issues/123#issuecomment-456"
            },
            "repository": {
                "id": 1,
                "name": "repo",
                "full_name": "owner/repo",
                "owner": {
                    "id": 1,
                    "login": "owner"
                }
            },
            "sender": {
                "id": 12345,
                "login": "testuser"
            }
        }"#;

        let event: IssueCommentEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.action, "created");
        assert_eq!(event.issue.number, 123);
        assert_eq!(event.comment.body, "Test comment");
        assert_eq!(event.comment.user.login, "testuser");
        assert!(event.issue.pull_request.is_some());
    }

    #[test]
    fn test_parse_pull_request_review_event() {
        let json = r#"{
            "action": "submitted",
            "review": {
                "id": 789,
                "body": "LGTM",
                "user": {
                    "id": 12345,
                    "login": "testuser"
                },
                "state": "approved",
                "html_url": "https://github.com/owner/repo/pull/123#pullrequestreview-789"
            },
            "pull_request": {
                "number": 123,
                "title": "Test PR",
                "body": null,
                "user": {
                    "id": 1,
                    "login": "owner"
                },
                "state": "open",
                "merged": false,
                "html_url": "https://github.com/owner/repo/pull/123"
            },
            "repository": {
                "id": 1,
                "name": "repo",
                "full_name": "owner/repo",
                "owner": {
                    "id": 1,
                    "login": "owner"
                }
            },
            "sender": {
                "id": 12345,
                "login": "testuser"
            }
        }"#;

        let event: PullRequestReviewEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.action, "submitted");
        assert_eq!(event.review.state, "approved");
        assert_eq!(event.review.body, Some("LGTM".to_string()));
        assert_eq!(event.review.user.login, "testuser");
        assert_eq!(event.pull_request.number, 123);
    }

    #[test]
    fn test_collaborator_role_has_write_access() {
        assert!(CollaboratorRole::Admin.has_write_access());
        assert!(CollaboratorRole::Maintain.has_write_access());
        assert!(CollaboratorRole::Write.has_write_access());
        assert!(!CollaboratorRole::Triage.has_write_access());
        assert!(!CollaboratorRole::Read.has_write_access());
        assert!(!CollaboratorRole::None.has_write_access());
    }

    #[test]
    fn test_collaborator_role_is_maintainer() {
        assert!(CollaboratorRole::Admin.is_maintainer());
        assert!(CollaboratorRole::Maintain.is_maintainer());
        assert!(!CollaboratorRole::Write.is_maintainer());
        assert!(!CollaboratorRole::Triage.is_maintainer());
        assert!(!CollaboratorRole::Read.is_maintainer());
        assert!(!CollaboratorRole::None.is_maintainer());
    }
}
