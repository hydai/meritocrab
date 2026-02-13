use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use meritocrab_core::{
    EventType, QualityLevel, RepoConfig, apply_credit, calculate_delta_with_config, check_blacklist,
};
use meritocrab_llm::{ContentType, EvalContext, LlmConfig, create_evaluator};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

mod git_state;

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
    /// Initialize state backend
    State {
        #[command(subcommand)]
        state_command: StateCommands,
    },
    /// Credit state management commands
    Credit {
        #[command(subcommand)]
        credit_command: CreditCommands,
    },
}

#[derive(Subcommand)]
enum StateCommands {
    /// Initialize git-branch state backend
    Init(StateInitArgs),
}

#[derive(Args)]
struct StateInitArgs {
    /// Git repository path
    #[arg(long, default_value = ".")]
    repo: PathBuf,
}

#[derive(Subcommand)]
enum CreditCommands {
    /// Initialize credit state directory
    Init(InitArgs),
    /// Check contributor credit state
    Check(CheckArgs),
    /// Update contributor credit state
    Update(UpdateArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum StateBackend {
    /// File-based state (default)
    File,
    /// Git branch state
    Git,
}

#[derive(Args)]
struct InitArgs {
    /// State directory path
    #[arg(long, default_value = "./credit-data")]
    state_dir: PathBuf,
}

#[derive(Args)]
struct CheckArgs {
    /// State backend
    #[arg(long, value_enum, default_value = "file")]
    state_backend: StateBackend,

    /// State directory path (used with file backend)
    #[arg(long, default_value = "./credit-data")]
    state_dir: PathBuf,

    /// Git repository path (used with git backend)
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Contributor GitHub user ID
    #[arg(long)]
    contributor_id: u64,

    /// Path to .meritocrab.toml config file
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Args)]
struct UpdateArgs {
    /// State backend
    #[arg(long, value_enum, default_value = "file")]
    state_backend: StateBackend,

    /// State directory path (used with file backend)
    #[arg(long, default_value = "./credit-data")]
    state_dir: PathBuf,

    /// Git repository path (used with git backend)
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Contributor GitHub user ID
    #[arg(long)]
    contributor_id: u64,

    /// Contributor username
    #[arg(long)]
    username: String,

    /// Credit delta to apply (or absolute value when --override is set)
    #[arg(long)]
    delta: i32,

    /// Event type
    #[arg(long)]
    event_type: String,

    /// PR number
    #[arg(long)]
    pr_number: Option<u64>,

    /// Evaluation summary
    #[arg(long)]
    evaluation_summary: Option<String>,

    /// Path to .meritocrab.toml config file
    #[arg(long)]
    config: Option<PathBuf>,

    /// Treat delta as an absolute credit value instead of a delta
    #[arg(long, default_value = "false")]
    r#override: bool,

    /// Explicitly set blacklist status (true/false)
    #[arg(long)]
    set_blacklisted: Option<bool>,
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

/// Contributor state in contributors.json
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContributorState {
    username: String,
    credit: i32,
    is_blacklisted: bool,
}

/// Credit event in events.json
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CreditEvent {
    contributor_id: u64,
    event_type: String,
    delta: i32,
    credit_before: i32,
    credit_after: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pr_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    evaluation_summary: Option<String>,
    timestamp: String,
}

/// Output format for credit check command
#[derive(Debug, Serialize, Deserialize)]
struct CreditCheckOutput {
    contributor_id: u64,
    username: Option<String>,
    credit: i32,
    is_blacklisted: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Evaluate { input, llm_config } => {
            evaluate_command(input, llm_config).await?;
        }
        Commands::State { state_command } => match state_command {
            StateCommands::Init(args) => {
                state_init_command(args)?;
            }
        },
        Commands::Credit { credit_command } => match credit_command {
            CreditCommands::Init(args) => {
                credit_init_command(args)?;
            }
            CreditCommands::Check(args) => {
                credit_check_command(args)?;
            }
            CreditCommands::Update(args) => {
                credit_update_command(args)?;
            }
        },
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

/// Initialize git-branch state backend
fn state_init_command(args: StateInitArgs) -> Result<()> {
    git_state::init_git_state(&args.repo)?;
    eprintln!(
        "Initialized git state backend on meritocrab-data branch in {:?}",
        args.repo.canonicalize().unwrap_or(args.repo)
    );
    Ok(())
}

/// Initialize credit state directory with empty JSON files
fn credit_init_command(args: InitArgs) -> Result<()> {
    // Create state directory if it doesn't exist
    std::fs::create_dir_all(&args.state_dir)
        .with_context(|| format!("Failed to create state directory: {:?}", args.state_dir))?;

    let contributors_path = args.state_dir.join("contributors.json");
    let events_path = args.state_dir.join("events.json");

    // Write empty contributors.json ({})
    write_json_atomic(
        &contributors_path,
        &HashMap::<String, ContributorState>::new(),
    )
    .context("Failed to write contributors.json")?;

    // Write empty events.json ([])
    write_json_atomic(&events_path, &Vec::<CreditEvent>::new())
        .context("Failed to write events.json")?;

    eprintln!(
        "Initialized credit state in {:?}",
        args.state_dir.canonicalize().unwrap_or(args.state_dir)
    );

    Ok(())
}

/// Check contributor credit state
fn credit_check_command(args: CheckArgs) -> Result<()> {
    let config = load_repo_config(args.config.as_deref())?;

    // Read contributors.json based on backend
    let contributors: HashMap<String, ContributorState> = match args.state_backend {
        StateBackend::File => {
            let contributors_path = args.state_dir.join("contributors.json");
            if contributors_path.exists() {
                let json = std::fs::read_to_string(&contributors_path)
                    .with_context(|| format!("Failed to read {:?}", contributors_path))?;
                serde_json::from_str(&json).context("Failed to parse contributors.json")?
            } else {
                HashMap::new()
            }
        }
        StateBackend::Git => {
            let json = git_state::read_contributors(&args.repo)?;
            serde_json::from_str(&json).context("Failed to parse contributors.json from git")?
        }
    };

    // Look up contributor
    let contributor_id_str = args.contributor_id.to_string();
    let output = if let Some(state) = contributors.get(&contributor_id_str) {
        CreditCheckOutput {
            contributor_id: args.contributor_id,
            username: Some(state.username.clone()),
            credit: state.credit,
            is_blacklisted: state.is_blacklisted,
        }
    } else {
        // Return default starting credit for new contributor
        CreditCheckOutput {
            contributor_id: args.contributor_id,
            username: None,
            credit: config.starting_credit,
            is_blacklisted: false,
        }
    };

    // Output JSON to stdout
    let output_json =
        serde_json::to_string_pretty(&output).context("Failed to serialize output")?;
    println!("{}", output_json);

    Ok(())
}

/// Update contributor credit state
fn credit_update_command(args: UpdateArgs) -> Result<()> {
    let config = load_repo_config(args.config.as_deref())?;

    match args.state_backend {
        StateBackend::File => {
            credit_update_file_backend(&args, &config)?;
        }
        StateBackend::Git => {
            credit_update_git_backend(&args, &config)?;
        }
    }

    Ok(())
}

/// Update credit state using file backend
fn credit_update_file_backend(args: &UpdateArgs, config: &RepoConfig) -> Result<()> {
    let contributors_path = args.state_dir.join("contributors.json");
    let events_path = args.state_dir.join("events.json");

    // Create state directory if it doesn't exist
    std::fs::create_dir_all(&args.state_dir)
        .with_context(|| format!("Failed to create state directory: {:?}", args.state_dir))?;

    // Read contributors.json
    let mut contributors: HashMap<String, ContributorState> = if contributors_path.exists() {
        let json = std::fs::read_to_string(&contributors_path)
            .with_context(|| format!("Failed to read {:?}", contributors_path))?;
        serde_json::from_str(&json).context("Failed to parse contributors.json")?
    } else {
        HashMap::new()
    };

    // Read events.json
    let mut events: Vec<CreditEvent> = if events_path.exists() {
        let json = std::fs::read_to_string(&events_path)
            .with_context(|| format!("Failed to read {:?}", events_path))?;
        serde_json::from_str(&json).context("Failed to parse events.json")?
    } else {
        Vec::new()
    };

    // Get current credit or default
    let contributor_id_str = args.contributor_id.to_string();
    let credit_before = contributors
        .get(&contributor_id_str)
        .map(|s| s.credit)
        .unwrap_or(config.starting_credit);

    // Apply credit: either absolute override or delta
    let credit_after = if args.r#override {
        // Override mode: delta value is the absolute credit to set
        std::cmp::max(0, args.delta)
    } else {
        apply_credit(credit_before, args.delta)
    };

    // Determine blacklist status: explicit flag takes priority, else check threshold
    let is_blacklisted = if let Some(bl) = args.set_blacklisted {
        bl
    } else {
        check_blacklist(credit_after, config.blacklist_threshold)
    };

    // Update contributor state
    contributors.insert(
        contributor_id_str.clone(),
        ContributorState {
            username: args.username.clone(),
            credit: credit_after,
            is_blacklisted,
        },
    );

    // Create credit event
    let event = CreditEvent {
        contributor_id: args.contributor_id,
        event_type: args.event_type.clone(),
        delta: args.delta,
        credit_before,
        credit_after,
        pr_number: args.pr_number,
        evaluation_summary: args.evaluation_summary.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    events.push(event);

    // Write updated files atomically
    write_json_atomic(&contributors_path, &contributors)
        .context("Failed to write contributors.json")?;
    write_json_atomic(&events_path, &events).context("Failed to write events.json")?;

    eprintln!(
        "Updated contributor {}: {} -> {} credit (delta: {})",
        args.contributor_id, credit_before, credit_after, args.delta
    );

    Ok(())
}

/// Update credit state using git backend with retry logic
fn credit_update_git_backend(args: &UpdateArgs, config: &RepoConfig) -> Result<()> {
    let max_retries = 3;
    let mut backoff_ms = 1000; // Start with 1 second

    for attempt in 1..=max_retries {
        match try_update_git_state(args, config) {
            Ok(_) => return Ok(()),
            Err(e) if attempt < max_retries && is_conflict_error(&e) => {
                eprintln!(
                    "Conflict detected on attempt {}/{}, retrying after {}ms...",
                    attempt, max_retries, backoff_ms
                );
                std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
                backoff_ms *= 2; // Exponential backoff
            }
            Err(e) => {
                if attempt == max_retries && is_conflict_error(&e) {
                    bail!(
                        "Failed to update git state after {} retries due to concurrent conflicts. \
                        Please try again later.",
                        max_retries
                    );
                } else {
                    return Err(e);
                }
            }
        }
    }

    unreachable!()
}

/// Check if error is a git conflict error
fn is_conflict_error(err: &anyhow::Error) -> bool {
    let err_str = err.to_string().to_lowercase();
    err_str.contains("conflict")
        || err_str.contains("rejected")
        || err_str.contains("non-fast-forward")
}

/// Try to update git state once (may fail due to concurrent updates)
fn try_update_git_state(args: &UpdateArgs, config: &RepoConfig) -> Result<()> {
    // Create commit message with PR number and event type
    let commit_msg = if let Some(pr_number) = args.pr_number {
        format!(
            "meritocrab: update credit for {} ({} #{})",
            args.username, args.event_type, pr_number
        )
    } else {
        format!(
            "meritocrab: update credit for {} ({})",
            args.username, args.event_type
        )
    };

    git_state::update_git_state(
        &args.repo,
        args.contributor_id,
        &args.username,
        args.delta,
        &args.event_type,
        args.pr_number,
        args.evaluation_summary.as_deref(),
        &commit_msg,
        config,
        args.r#override,
        args.set_blacklisted,
    )?;

    Ok(())
}

/// Load RepoConfig from .meritocrab.toml or use defaults
fn load_repo_config(config_path: Option<&std::path::Path>) -> Result<RepoConfig> {
    if let Some(path) = config_path {
        let toml_str = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;
        let config: RepoConfig = toml::from_str(&toml_str)
            .with_context(|| format!("Failed to parse TOML config: {:?}", path))?;
        Ok(config)
    } else {
        Ok(RepoConfig::default())
    }
}

/// Write JSON to file atomically using temp file + rename
fn write_json_atomic<T: Serialize>(path: &std::path::Path, data: &T) -> Result<()> {
    use std::io::Write;

    let parent = path.parent().context("Failed to get parent directory")?;

    // Create a temporary file in the same directory
    let temp_path = parent.join(format!(
        ".{}.tmp.{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id()
    ));

    // Write JSON to temp file
    let json = serde_json::to_string_pretty(data).context("Failed to serialize JSON")?;
    let mut temp_file = std::fs::File::create(&temp_path)
        .with_context(|| format!("Failed to create temp file: {:?}", temp_path))?;
    temp_file
        .write_all(json.as_bytes())
        .context("Failed to write to temp file")?;
    temp_file.sync_all().context("Failed to sync temp file")?;

    // Atomically rename temp file to final path
    std::fs::rename(&temp_path, path)
        .with_context(|| format!("Failed to rename {:?} to {:?}", temp_path, path))?;

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

    // Credit state management tests
    #[test]
    fn test_credit_init_creates_empty_files() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let args = InitArgs {
            state_dir: temp_dir.path().to_path_buf(),
        };

        credit_init_command(args).unwrap();

        // Verify contributors.json exists and is empty object
        let contributors_path = temp_dir.path().join("contributors.json");
        assert!(contributors_path.exists());
        let contributors_json = fs::read_to_string(&contributors_path).unwrap();
        let contributors: HashMap<String, ContributorState> =
            serde_json::from_str(&contributors_json).unwrap();
        assert!(contributors.is_empty());

        // Verify events.json exists and is empty array
        let events_path = temp_dir.path().join("events.json");
        assert!(events_path.exists());
        let events_json = fs::read_to_string(&events_path).unwrap();
        let events: Vec<CreditEvent> = serde_json::from_str(&events_json).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_credit_check_nonexistent_contributor() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();

        // Initialize state
        credit_init_command(InitArgs {
            state_dir: temp_dir.path().to_path_buf(),
        })
        .unwrap();

        // Check non-existent contributor - should return default credit
        let args = CheckArgs {
            state_backend: StateBackend::File,
            state_dir: temp_dir.path().to_path_buf(),
            repo: PathBuf::from("."),
            contributor_id: 12345678,
            config: None,
        };

        // Just verify it doesn't panic - the command writes to stdout
        let result = credit_check_command(args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_credit_update_applies_delta() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();

        // Initialize state
        credit_init_command(InitArgs {
            state_dir: temp_dir.path().to_path_buf(),
        })
        .unwrap();

        // Update credit
        let update_args = UpdateArgs {
            state_backend: StateBackend::File,
            state_dir: temp_dir.path().to_path_buf(),
            repo: PathBuf::from("."),
            contributor_id: 12345678,
            username: "alice".to_string(),
            delta: 15,
            event_type: "pr_opened".to_string(),
            pr_number: Some(42),
            evaluation_summary: Some("High quality PR".to_string()),
            config: None,
            r#override: false,
            set_blacklisted: None,
        };

        credit_update_command(update_args).unwrap();

        // Read and verify contributors.json
        let contributors_json =
            fs::read_to_string(temp_dir.path().join("contributors.json")).unwrap();
        let contributors: HashMap<String, ContributorState> =
            serde_json::from_str(&contributors_json).unwrap();

        let state = contributors.get("12345678").unwrap();
        assert_eq!(state.username, "alice");
        assert_eq!(state.credit, 115); // Default 100 + 15
        assert!(!state.is_blacklisted);

        // Read and verify events.json
        let events_json = fs::read_to_string(temp_dir.path().join("events.json")).unwrap();
        let events: Vec<CreditEvent> = serde_json::from_str(&events_json).unwrap();

        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.contributor_id, 12345678);
        assert_eq!(event.event_type, "pr_opened");
        assert_eq!(event.delta, 15);
        assert_eq!(event.credit_before, 100);
        assert_eq!(event.credit_after, 115);
        assert_eq!(event.pr_number, Some(42));
        assert_eq!(
            event.evaluation_summary,
            Some("High quality PR".to_string())
        );
    }

    #[test]
    fn test_credit_update_clamps_to_zero() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();

        // Initialize state
        credit_init_command(InitArgs {
            state_dir: temp_dir.path().to_path_buf(),
        })
        .unwrap();

        // Update with large negative delta
        let update_args = UpdateArgs {
            state_backend: StateBackend::File,
            state_dir: temp_dir.path().to_path_buf(),
            repo: PathBuf::from("."),
            contributor_id: 99999999,
            username: "bob".to_string(),
            delta: -150,
            event_type: "pr_opened".to_string(),
            pr_number: None,
            evaluation_summary: None,
            config: None,
            r#override: false,
            set_blacklisted: None,
        };

        credit_update_command(update_args).unwrap();

        // Verify credit clamped to 0
        let contributors_json =
            fs::read_to_string(temp_dir.path().join("contributors.json")).unwrap();
        let contributors: HashMap<String, ContributorState> =
            serde_json::from_str(&contributors_json).unwrap();

        let state = contributors.get("99999999").unwrap();
        assert_eq!(state.credit, 0); // Clamped to 0, not -50
        assert!(state.is_blacklisted); // At threshold
    }

    #[test]
    fn test_credit_update_sets_blacklist_flag() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();

        // Initialize state
        credit_init_command(InitArgs {
            state_dir: temp_dir.path().to_path_buf(),
        })
        .unwrap();

        // First update - credit still above threshold
        credit_update_command(UpdateArgs {
            state_backend: StateBackend::File,
            state_dir: temp_dir.path().to_path_buf(),
            repo: PathBuf::from("."),
            contributor_id: 55555555,
            username: "charlie".to_string(),
            delta: -50,
            event_type: "pr_opened".to_string(),
            pr_number: None,
            evaluation_summary: None,
            config: None,
            r#override: false,
            set_blacklisted: None,
        })
        .unwrap();

        let contributors_json =
            fs::read_to_string(temp_dir.path().join("contributors.json")).unwrap();
        let contributors: HashMap<String, ContributorState> =
            serde_json::from_str(&contributors_json).unwrap();
        let state = contributors.get("55555555").unwrap();
        assert_eq!(state.credit, 50);
        assert!(!state.is_blacklisted); // Above threshold

        // Second update - drops to threshold
        credit_update_command(UpdateArgs {
            state_backend: StateBackend::File,
            state_dir: temp_dir.path().to_path_buf(),
            repo: PathBuf::from("."),
            contributor_id: 55555555,
            username: "charlie".to_string(),
            delta: -50,
            event_type: "pr_opened".to_string(),
            pr_number: None,
            evaluation_summary: None,
            config: None,
            r#override: false,
            set_blacklisted: None,
        })
        .unwrap();

        let contributors_json =
            fs::read_to_string(temp_dir.path().join("contributors.json")).unwrap();
        let contributors: HashMap<String, ContributorState> =
            serde_json::from_str(&contributors_json).unwrap();
        let state = contributors.get("55555555").unwrap();
        assert_eq!(state.credit, 0);
        assert!(state.is_blacklisted); // At threshold
    }

    #[test]
    fn test_credit_with_custom_config() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("custom.toml");

        // Write custom config
        let config_toml = r#"
starting_credit = 200
pr_threshold = 75
blacklist_threshold = 10

[pr_opened]
spam = -25
low = -5
acceptable = 5
high = 15

[comment]
spam = -10
low = -2
acceptable = 1
high = 3

[pr_merged]
spam = 0
low = 0
acceptable = 20
high = 20

[review_submitted]
spam = 0
low = 0
acceptable = 5
high = 5
"#;
        fs::write(&config_path, config_toml).unwrap();

        // Initialize state
        credit_init_command(InitArgs {
            state_dir: temp_dir.path().to_path_buf(),
        })
        .unwrap();

        // Update with custom config - should use starting_credit = 200
        credit_update_command(UpdateArgs {
            state_backend: StateBackend::File,
            state_dir: temp_dir.path().to_path_buf(),
            repo: PathBuf::from("."),
            contributor_id: 77777777,
            username: "dave".to_string(),
            delta: 10,
            event_type: "pr_opened".to_string(),
            pr_number: None,
            evaluation_summary: None,
            config: Some(config_path.clone()),
            r#override: false,
            set_blacklisted: None,
        })
        .unwrap();

        let contributors_json =
            fs::read_to_string(temp_dir.path().join("contributors.json")).unwrap();
        let contributors: HashMap<String, ContributorState> =
            serde_json::from_str(&contributors_json).unwrap();
        let state = contributors.get("77777777").unwrap();
        assert_eq!(state.credit, 210); // 200 (custom starting) + 10

        // Drop to just above custom blacklist threshold
        credit_update_command(UpdateArgs {
            state_backend: StateBackend::File,
            state_dir: temp_dir.path().to_path_buf(),
            repo: PathBuf::from("."),
            contributor_id: 77777777,
            username: "dave".to_string(),
            delta: -199,
            event_type: "pr_opened".to_string(),
            pr_number: None,
            evaluation_summary: None,
            config: Some(config_path.clone()),
            r#override: false,
            set_blacklisted: None,
        })
        .unwrap();

        let contributors_json =
            fs::read_to_string(temp_dir.path().join("contributors.json")).unwrap();
        let contributors: HashMap<String, ContributorState> =
            serde_json::from_str(&contributors_json).unwrap();
        let state = contributors.get("77777777").unwrap();
        assert_eq!(state.credit, 11);
        assert!(!state.is_blacklisted); // 11 > 10 (custom threshold)

        // Drop to threshold
        credit_update_command(UpdateArgs {
            state_backend: StateBackend::File,
            state_dir: temp_dir.path().to_path_buf(),
            repo: PathBuf::from("."),
            contributor_id: 77777777,
            username: "dave".to_string(),
            delta: -1,
            event_type: "pr_opened".to_string(),
            pr_number: None,
            evaluation_summary: None,
            config: Some(config_path),
            r#override: false,
            set_blacklisted: None,
        })
        .unwrap();

        let contributors_json =
            fs::read_to_string(temp_dir.path().join("contributors.json")).unwrap();
        let contributors: HashMap<String, ContributorState> =
            serde_json::from_str(&contributors_json).unwrap();
        let state = contributors.get("77777777").unwrap();
        assert_eq!(state.credit, 10);
        assert!(state.is_blacklisted); // 10 <= 10 (custom threshold)
    }

    #[test]
    fn test_atomic_write_creates_valid_json() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join("test.json");

        let data: HashMap<String, i32> = vec![("a".to_string(), 1), ("b".to_string(), 2)]
            .into_iter()
            .collect();

        write_json_atomic(&test_path, &data).unwrap();

        // Verify file exists and contains valid JSON
        assert!(test_path.exists());
        let json = std::fs::read_to_string(&test_path).unwrap();
        let parsed: HashMap<String, i32> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.get("a"), Some(&1));
        assert_eq!(parsed.get("b"), Some(&2));
    }
}
