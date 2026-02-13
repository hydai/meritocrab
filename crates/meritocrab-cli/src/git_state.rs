use anyhow::{Context, Result, bail};
use meritocrab_core::{RepoConfig, apply_credit, check_blacklist};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

const DATA_BRANCH: &str = "meritocrab-data";
const CONTRIBUTORS_FILE: &str = "credit-data/contributors.json";
const EVENTS_FILE: &str = "credit-data/events.json";

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

/// Initialize git state backend by creating meritocrab-data orphan branch
pub fn init_git_state(repo_path: &Path) -> Result<()> {
    // Check if branch already exists
    if branch_exists(repo_path)? {
        eprintln!("State already initialized: meritocrab-data branch exists");
        return Ok(());
    }

    // Create temporary directory for initialization
    let temp_dir = tempfile::TempDir::new().context("Failed to create temp directory")?;
    let temp_path = temp_dir.path();

    // Initialize a new git repo in temp directory
    run_git(temp_path, &["init"])?;

    // Create orphan branch
    run_git(temp_path, &["checkout", "--orphan", DATA_BRANCH])?;

    // Create credit-data directory and files
    let credit_data_dir = temp_path.join("credit-data");
    std::fs::create_dir_all(&credit_data_dir).context("Failed to create credit-data directory")?;

    // Write empty JSON files
    let empty_contributors =
        serde_json::to_string_pretty(&HashMap::<String, ContributorState>::new())?;
    let empty_events = serde_json::to_string_pretty(&Vec::<CreditEvent>::new())?;

    std::fs::write(
        credit_data_dir.join("contributors.json"),
        empty_contributors,
    )
    .context("Failed to write contributors.json")?;
    std::fs::write(credit_data_dir.join("events.json"), empty_events)
        .context("Failed to write events.json")?;

    // Add and commit
    run_git(temp_path, &["add", "credit-data/"])?;
    run_git(
        temp_path,
        &["commit", "-m", "meritocrab: initialize state branch"],
    )?;

    // Add remote pointing to the original repo and push
    let repo_path_str = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf())
        .to_string_lossy()
        .to_string();
    run_git(temp_path, &["remote", "add", "origin", &repo_path_str])?;
    run_git(temp_path, &["push", "origin", DATA_BRANCH])?;

    Ok(())
}

/// Check if meritocrab-data branch exists
fn branch_exists(repo_path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "rev-parse",
            "--verify",
            DATA_BRANCH,
        ])
        .output()
        .context("Failed to run git rev-parse")?;

    Ok(output.status.success())
}

/// Read contributors.json from git branch
pub fn read_contributors(repo_path: &Path) -> Result<String> {
    read_file_from_branch(repo_path, CONTRIBUTORS_FILE)
}

/// Read events.json from git branch
#[allow(dead_code)]
pub fn read_events(repo_path: &Path) -> Result<String> {
    read_file_from_branch(repo_path, EVENTS_FILE)
}

/// Read a file from the meritocrab-data branch
fn read_file_from_branch(repo_path: &Path, file_path: &str) -> Result<String> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "show",
            &format!("{}:{}", DATA_BRANCH, file_path),
        ])
        .output()
        .with_context(|| format!("Failed to read {} from git", file_path))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Git show failed: {}", stderr);
    }

    String::from_utf8(output.stdout).context("Invalid UTF-8 in git output")
}

/// Update git state with new credit delta
#[allow(clippy::too_many_arguments)]
pub fn update_git_state(
    repo_path: &Path,
    contributor_id: u64,
    username: &str,
    delta: i32,
    event_type: &str,
    pr_number: Option<u64>,
    evaluation_summary: Option<&str>,
    commit_msg: &str,
    config: &RepoConfig,
) -> Result<()> {
    // Create temporary directory for the operation
    let temp_dir = tempfile::TempDir::new().context("Failed to create temp directory")?;
    let temp_path = temp_dir.path();

    // Clone just the data branch
    let repo_path_str = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf())
        .to_string_lossy()
        .to_string();

    run_git(
        temp_path.parent().unwrap(),
        &[
            "clone",
            "--single-branch",
            "-b",
            DATA_BRANCH,
            &repo_path_str,
            &temp_path.to_string_lossy(),
        ],
    )?;

    // Read current state
    let contributors_path = temp_path.join(CONTRIBUTORS_FILE);
    let events_path = temp_path.join(EVENTS_FILE);

    let mut contributors: HashMap<String, ContributorState> = {
        let json = std::fs::read_to_string(&contributors_path)
            .context("Failed to read contributors.json")?;
        serde_json::from_str(&json).context("Failed to parse contributors.json")?
    };

    let mut events: Vec<CreditEvent> = {
        let json = std::fs::read_to_string(&events_path).context("Failed to read events.json")?;
        serde_json::from_str(&json).context("Failed to parse events.json")?
    };

    // Get current credit or default
    let contributor_id_str = contributor_id.to_string();
    let credit_before = contributors
        .get(&contributor_id_str)
        .map(|s| s.credit)
        .unwrap_or(config.starting_credit);

    // Apply credit delta with clamping to 0
    let credit_after = apply_credit(credit_before, delta);

    // Check blacklist status
    let is_blacklisted = check_blacklist(credit_after, config.blacklist_threshold);

    // Update contributor state
    contributors.insert(
        contributor_id_str.clone(),
        ContributorState {
            username: username.to_string(),
            credit: credit_after,
            is_blacklisted,
        },
    );

    // Create credit event
    let event = CreditEvent {
        contributor_id,
        event_type: event_type.to_string(),
        delta,
        credit_before,
        credit_after,
        pr_number,
        evaluation_summary: evaluation_summary.map(|s| s.to_string()),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    events.push(event);

    // Write updated files
    let contributors_json = serde_json::to_string_pretty(&contributors)?;
    let events_json = serde_json::to_string_pretty(&events)?;

    std::fs::write(&contributors_path, contributors_json)
        .context("Failed to write contributors.json")?;
    std::fs::write(&events_path, events_json).context("Failed to write events.json")?;

    // Commit and push
    run_git(temp_path, &["add", "credit-data/"])?;
    run_git(temp_path, &["commit", "-m", commit_msg])?;
    run_git(temp_path, &["push", "origin", DATA_BRANCH])?;

    eprintln!(
        "Updated contributor {}: {} -> {} credit (delta: {})",
        contributor_id, credit_before, credit_after, delta
    );

    Ok(())
}

/// Run a git command
fn run_git(cwd: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run git command: git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Git command failed: git {}\n{}", args.join(" "), stderr);
    }

    Ok(())
}
