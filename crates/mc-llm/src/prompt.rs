use crate::traits::{ContentType, EvalContext};

/// System prompt for LLM evaluation
pub fn system_prompt() -> &'static str {
    r#"You are an expert code reviewer evaluating open source contributions for quality and spam detection.

Your task is to classify contributions into one of four quality levels:
- spam: Obvious spam, promotional content, or malicious contributions
- low: Low-effort contributions with minimal value (trivial changes, poor quality, unclear intent)
- acceptable: Valid contributions that meet basic standards
- high: High-quality contributions (well-structured, clear intent, meaningful improvements)

Return your evaluation as JSON in this exact format:
{
  "classification": "spam" | "low" | "acceptable" | "high",
  "confidence": 0.0-1.0,
  "reasoning": "Brief explanation of your classification"
}

Be objective and focus on:
1. Intent and quality of the contribution
2. Clarity of communication
3. Technical merit
4. Effort and thoughtfulness
5. Potential value to the project"#
}

/// Build user prompt for evaluating content
pub fn build_user_prompt(content: &str, context: &EvalContext) -> String {
    match context.content_type {
        ContentType::PullRequest => build_pr_prompt(content, context),
        ContentType::Comment => build_comment_prompt(content, context),
        ContentType::Review => build_review_prompt(content, context),
    }
}

/// Build prompt for pull request evaluation
fn build_pr_prompt(content: &str, context: &EvalContext) -> String {
    let mut prompt = String::from("Evaluate this pull request:\n\n");

    if let Some(title) = &context.title {
        prompt.push_str(&format!("Title: {}\n\n", title));
    }

    prompt.push_str(&format!("Description:\n{}\n\n", context.body));

    if let Some(diff) = &context.diff_summary {
        prompt.push_str(&format!("Diff Summary: {}\n\n", diff));
    }

    prompt.push_str(&format!("Full Content:\n{}\n\n", content));
    prompt.push_str("Provide your evaluation as JSON.");

    prompt
}

/// Build prompt for comment evaluation
fn build_comment_prompt(content: &str, context: &EvalContext) -> String {
    let mut prompt = String::from("Evaluate this comment:\n\n");

    if let Some(thread) = &context.thread_context {
        prompt.push_str(&format!("Thread Context:\n{}\n\n", thread));
    }

    prompt.push_str(&format!("Comment:\n{}\n\n", content));
    prompt.push_str("Provide your evaluation as JSON.");

    prompt
}

/// Build prompt for review evaluation
fn build_review_prompt(content: &str, context: &EvalContext) -> String {
    let mut prompt = String::from("Evaluate this pull request review:\n\n");

    if let Some(thread) = &context.thread_context {
        prompt.push_str(&format!("PR Context:\n{}\n\n", thread));
    }

    prompt.push_str(&format!("Review:\n{}\n\n", content));
    prompt.push_str("Provide your evaluation as JSON.");

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_prompt() {
        let prompt = system_prompt();
        assert!(prompt.contains("spam"));
        assert!(prompt.contains("low"));
        assert!(prompt.contains("acceptable"));
        assert!(prompt.contains("high"));
        assert!(prompt.contains("JSON"));
    }

    #[test]
    fn test_build_pr_prompt() {
        let context = EvalContext {
            content_type: ContentType::PullRequest,
            title: Some("Fix parser bug".to_string()),
            body: "This fixes issue #123".to_string(),
            diff_summary: Some("+10 -5 lines".to_string()),
            thread_context: None,
        };

        let prompt = build_user_prompt("PR content here", &context);
        assert!(prompt.contains("pull request"));
        assert!(prompt.contains("Fix parser bug"));
        assert!(prompt.contains("This fixes issue #123"));
        assert!(prompt.contains("+10 -5 lines"));
        assert!(prompt.contains("PR content here"));
    }

    #[test]
    fn test_build_comment_prompt() {
        let context = EvalContext {
            content_type: ContentType::Comment,
            title: None,
            body: "Great work!".to_string(),
            diff_summary: None,
            thread_context: Some("Discussion about implementation".to_string()),
        };

        let prompt = build_user_prompt("Great work!", &context);
        assert!(prompt.contains("comment"));
        assert!(prompt.contains("Discussion about implementation"));
        assert!(prompt.contains("Great work!"));
    }

    #[test]
    fn test_build_review_prompt() {
        let context = EvalContext {
            content_type: ContentType::Review,
            title: None,
            body: "Looks good to me".to_string(),
            diff_summary: None,
            thread_context: Some("PR about feature X".to_string()),
        };

        let prompt = build_user_prompt("Looks good to me", &context);
        assert!(prompt.contains("review"));
        assert!(prompt.contains("PR about feature X"));
        assert!(prompt.contains("Looks good to me"));
    }
}
