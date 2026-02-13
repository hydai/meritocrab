use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use meritocrab_core::{EventType, QualityLevel, RepoConfig, calculate_delta_with_config};
use meritocrab_llm::{ContentType, EvalContext, LlmConfig, create_evaluator};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "meritocrab-cli")]
#[command(about = "CLI for GitHub Actions mode of the Meritocrab reputation system")]
#[command(version = VERSION)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Evaluate a PR from a JSON artifact file
    Evaluate {
        /// Path to the PR evaluation artifact JSON file
        #[arg(short, long)]
        input: PathBuf,

        /// LLM configuration as JSON string
        #[arg(short, long)]
        llm_config: String,
    },
}

/// PR evaluation artifact schema (from DESIGN-github-actions.md Section 3)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrArtifact {
    schema_version: u32,
    pr_number: u64,
    pr_author: String,
    pr_author_id: u64,
    pr_title: String,
    pr_body: String,
    base_repo: String,
    head_repo: String,
    diff_stats: DiffStats,
    file_list: Vec<String>,
    diff_content: String,
    event_timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiffStats {
    additions: u64,
    deletions: u64,
    changed_files: u64,
}

/// Evaluation output
#[derive(Debug, Serialize, Deserialize)]
struct EvaluationOutput {
    classification: QualityLevel,
    confidence: f64,
    reasoning: String,
    credit_delta: i32,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Evaluate { input, llm_config } => {
            evaluate_command(input, llm_config).await?;
        }
    }

    Ok(())
}

async fn evaluate_command(input_path: PathBuf, llm_config_str: String) -> Result<()> {
    // Read and parse artifact JSON
    let artifact_json = std::fs::read_to_string(&input_path)
        .with_context(|| format!("Failed to read artifact file: {:?}", input_path))?;

    let artifact: PrArtifact =
        serde_json::from_str(&artifact_json).context("Failed to parse artifact JSON")?;

    // Validate artifact schema
    validate_artifact(&artifact)?;

    // Parse LLM config
    let llm_config: LlmConfig =
        serde_json::from_str(&llm_config_str).context("Failed to parse LLM config JSON")?;

    // Truncate diff_content to 10KB if needed
    let max_diff_size = 10 * 1024; // 10KB
    let diff_content = if artifact.diff_content.len() > max_diff_size {
        &artifact.diff_content[..max_diff_size]
    } else {
        &artifact.diff_content
    };

    // Create LLM evaluator
    let evaluator = create_evaluator(&llm_config).context("Failed to create LLM evaluator")?;

    // Build evaluation context
    let eval_context = EvalContext {
        content_type: ContentType::PullRequest,
        title: Some(artifact.pr_title.clone()),
        body: artifact.pr_body.clone(),
        diff_summary: Some(format!(
            "+{} -{} files:{}",
            artifact.diff_stats.additions,
            artifact.diff_stats.deletions,
            artifact.diff_stats.changed_files
        )),
        thread_context: None,
    };

    // Call LLM evaluator
    let evaluation = evaluator
        .evaluate(diff_content, &eval_context)
        .await
        .context("LLM evaluation failed")?;

    // Calculate credit delta using default RepoConfig
    let repo_config = RepoConfig::default();
    let credit_delta =
        calculate_delta_with_config(&repo_config, EventType::PrOpened, evaluation.classification);

    // Create output
    let output = EvaluationOutput {
        classification: evaluation.classification,
        confidence: evaluation.confidence,
        reasoning: evaluation.reasoning,
        credit_delta,
    };

    // Output JSON to stdout
    let output_json =
        serde_json::to_string_pretty(&output).context("Failed to serialize output")?;
    println!("{}", output_json);

    Ok(())
}

fn validate_artifact(artifact: &PrArtifact) -> Result<()> {
    // Check schema version
    if artifact.schema_version != 1 {
        bail!(
            "Invalid schema_version: expected 1, got {}",
            artifact.schema_version
        );
    }

    // Validate required fields are non-empty
    if artifact.pr_author.is_empty() {
        bail!("Missing required field: pr_author");
    }
    if artifact.pr_title.is_empty() {
        bail!("Missing required field: pr_title");
    }
    if artifact.base_repo.is_empty() {
        bail!("Missing required field: base_repo");
    }
    if artifact.head_repo.is_empty() {
        bail!("Missing required field: head_repo");
    }
    if artifact.event_timestamp.is_empty() {
        bail!("Missing required field: event_timestamp");
    }

    // Validate numeric fields are reasonable
    if artifact.pr_number == 0 {
        bail!("Invalid pr_number: must be > 0");
    }
    if artifact.pr_author_id == 0 {
        bail!("Invalid pr_author_id: must be > 0");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_artifact_valid() {
        let artifact = PrArtifact {
            schema_version: 1,
            pr_number: 42,
            pr_author: "contributor".to_string(),
            pr_author_id: 12345678,
            pr_title: "Add feature".to_string(),
            pr_body: "This adds a feature".to_string(),
            base_repo: "owner/repo".to_string(),
            head_repo: "fork/repo".to_string(),
            diff_stats: DiffStats {
                additions: 50,
                deletions: 10,
                changed_files: 3,
            },
            file_list: vec!["src/main.rs".to_string()],
            diff_content: "diff content".to_string(),
            event_timestamp: "2026-02-13T12:00:00Z".to_string(),
        };

        assert!(validate_artifact(&artifact).is_ok());
    }

    #[test]
    fn test_validate_artifact_invalid_schema_version() {
        let artifact = PrArtifact {
            schema_version: 2,
            pr_number: 42,
            pr_author: "contributor".to_string(),
            pr_author_id: 12345678,
            pr_title: "Add feature".to_string(),
            pr_body: "This adds a feature".to_string(),
            base_repo: "owner/repo".to_string(),
            head_repo: "fork/repo".to_string(),
            diff_stats: DiffStats {
                additions: 50,
                deletions: 10,
                changed_files: 3,
            },
            file_list: vec!["src/main.rs".to_string()],
            diff_content: "diff content".to_string(),
            event_timestamp: "2026-02-13T12:00:00Z".to_string(),
        };

        assert!(validate_artifact(&artifact).is_err());
    }

    #[test]
    fn test_validate_artifact_missing_pr_author() {
        let artifact = PrArtifact {
            schema_version: 1,
            pr_number: 42,
            pr_author: "".to_string(),
            pr_author_id: 12345678,
            pr_title: "Add feature".to_string(),
            pr_body: "This adds a feature".to_string(),
            base_repo: "owner/repo".to_string(),
            head_repo: "fork/repo".to_string(),
            diff_stats: DiffStats {
                additions: 50,
                deletions: 10,
                changed_files: 3,
            },
            file_list: vec!["src/main.rs".to_string()],
            diff_content: "diff content".to_string(),
            event_timestamp: "2026-02-13T12:00:00Z".to_string(),
        };

        let result = validate_artifact(&artifact);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("pr_author"));
    }

    #[test]
    fn test_validate_artifact_missing_pr_title() {
        let artifact = PrArtifact {
            schema_version: 1,
            pr_number: 42,
            pr_author: "contributor".to_string(),
            pr_author_id: 12345678,
            pr_title: "".to_string(),
            pr_body: "This adds a feature".to_string(),
            base_repo: "owner/repo".to_string(),
            head_repo: "fork/repo".to_string(),
            diff_stats: DiffStats {
                additions: 50,
                deletions: 10,
                changed_files: 3,
            },
            file_list: vec!["src/main.rs".to_string()],
            diff_content: "diff content".to_string(),
            event_timestamp: "2026-02-13T12:00:00Z".to_string(),
        };

        let result = validate_artifact(&artifact);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("pr_title"));
    }

    #[test]
    fn test_validate_artifact_zero_pr_number() {
        let artifact = PrArtifact {
            schema_version: 1,
            pr_number: 0,
            pr_author: "contributor".to_string(),
            pr_author_id: 12345678,
            pr_title: "Add feature".to_string(),
            pr_body: "This adds a feature".to_string(),
            base_repo: "owner/repo".to_string(),
            head_repo: "fork/repo".to_string(),
            diff_stats: DiffStats {
                additions: 50,
                deletions: 10,
                changed_files: 3,
            },
            file_list: vec!["src/main.rs".to_string()],
            diff_content: "diff content".to_string(),
            event_timestamp: "2026-02-13T12:00:00Z".to_string(),
        };

        let result = validate_artifact(&artifact);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("pr_number"));
    }

    #[test]
    fn test_artifact_deserialization_valid() {
        let json = r#"{
            "schema_version": 1,
            "pr_number": 42,
            "pr_author": "contributor-username",
            "pr_author_id": 12345678,
            "pr_title": "Add feature X",
            "pr_body": "This PR adds...",
            "base_repo": "owner/repo",
            "head_repo": "fork-owner/repo",
            "diff_stats": { "additions": 50, "deletions": 10, "changed_files": 3 },
            "file_list": ["src/main.rs", "tests/test_main.rs", "README.md"],
            "diff_content": "truncated diff (first 10KB)",
            "event_timestamp": "2026-02-13T12:00:00Z"
        }"#;

        let artifact: Result<PrArtifact, _> = serde_json::from_str(json);
        assert!(artifact.is_ok());
        let artifact = artifact.unwrap();
        assert_eq!(artifact.schema_version, 1);
        assert_eq!(artifact.pr_number, 42);
        assert_eq!(artifact.pr_author, "contributor-username");
    }

    #[test]
    fn test_artifact_deserialization_missing_field() {
        let json = r#"{
            "schema_version": 1,
            "pr_number": 42,
            "pr_author": "contributor-username"
        }"#;

        let artifact: Result<PrArtifact, _> = serde_json::from_str(json);
        assert!(artifact.is_err());
    }

    #[test]
    fn test_artifact_deserialization_rejects_extra_fields() {
        let json = r#"{
            "schema_version": 1,
            "pr_number": 42,
            "pr_author": "contributor-username",
            "pr_author_id": 12345678,
            "pr_title": "Add feature X",
            "pr_body": "This PR adds...",
            "base_repo": "owner/repo",
            "head_repo": "fork-owner/repo",
            "diff_stats": { "additions": 50, "deletions": 10, "changed_files": 3 },
            "file_list": ["src/main.rs"],
            "diff_content": "diff",
            "event_timestamp": "2026-02-13T12:00:00Z",
            "unexpected_field": "value"
        }"#;

        let artifact: Result<PrArtifact, _> = serde_json::from_str(json);
        assert!(artifact.is_err());
        let err = artifact.unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn test_evaluation_output_serialization() {
        let output = EvaluationOutput {
            classification: QualityLevel::High,
            confidence: 0.95,
            reasoning: "Well-structured PR".to_string(),
            credit_delta: 15,
        };

        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("high"));
        assert!(json.contains("0.95"));
        assert!(json.contains("15"));
    }
}
