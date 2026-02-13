# GitHub Actions Mode: Setup Guide

This guide walks you through setting up Meritocrab using GitHub Actions workflows instead of deploying a server. The GitHub Actions mode is ideal for small-to-medium repositories that want to adopt Meritocrab without managing infrastructure.

## Table of Contents

1. [Quick Start](#quick-start)
2. [Detailed Setup](#detailed-setup)
3. [Configuration](#configuration)
4. [Testing the Setup](#testing-the-setup)
5. [Customization](#customization)
6. [Troubleshooting](#troubleshooting)
7. [Migration to Server Mode](#migration-to-server-mode)

## Quick Start

Get Meritocrab running in under 5 minutes:

1. **Copy workflow files to your repository**:
   ```bash
   mkdir -p .github/workflows
   curl -O https://raw.githubusercontent.com/hydai/meritocrab/master/.github/workflows/pr-evaluate.yml
   curl -O https://raw.githubusercontent.com/hydai/meritocrab/master/.github/workflows/pr-gate.yml
   curl -O https://raw.githubusercontent.com/hydai/meritocrab/master/.github/workflows/pr-commands.yml
   mv *.yml .github/workflows/
   ```

2. **Add your LLM API key as a repository secret**:
   - Go to your repository on GitHub
   - Navigate to **Settings** → **Secrets and variables** → **Actions**
   - Click **New repository secret**
   - Name: `MERITOCRAB_LLM_API_KEY`
   - Value: Your Claude or OpenAI API key

3. **Initialize the credit state branch**:
   ```bash
   # Install meritocrab-cli
   cargo install meritocrab-cli

   # Initialize the state branch
   meritocrab-cli state init --repo .

   # Push the new branch
   git push origin meritocrab-data
   ```

4. **Commit and push the workflow files**:
   ```bash
   git add .github/workflows/
   git commit -m "chore: add meritocrab workflows"
   git push
   ```

That's it! Meritocrab will now evaluate all new PRs automatically.

## Detailed Setup

### Prerequisites

- A GitHub repository where you have admin access
- An LLM API key (Claude or OpenAI)
- Rust 1.85+ installed locally (for state initialization)

### Step 1: Copy Workflow Files

Meritocrab uses three GitHub Actions workflows:

1. **pr-evaluate.yml** — Safely extracts PR metadata from fork PRs (unprivileged)
2. **pr-gate.yml** — Evaluates PRs with LLM and gates based on credit (privileged)
3. **pr-commands.yml** — Handles `/credit` commands in PR comments

Copy these files to your repository's `.github/workflows/` directory:

```bash
mkdir -p .github/workflows
cd .github/workflows

# Download workflow files from the Meritocrab repository
curl -O https://raw.githubusercontent.com/hydai/meritocrab/master/.github/workflows/pr-evaluate.yml
curl -O https://raw.githubusercontent.com/hydai/meritocrab/master/.github/workflows/pr-gate.yml
curl -O https://raw.githubusercontent.com/hydai/meritocrab/master/.github/workflows/pr-commands.yml
```

**Alternative**: Copy the files directly from the [Meritocrab repository](https://github.com/hydai/meritocrab/tree/master/.github/workflows).

### Step 2: Configure LLM API Key

The workflows need access to an LLM API for evaluating contribution quality.

#### Option A: Claude (Recommended)

1. Get an API key from [Anthropic Console](https://console.anthropic.com/)
2. The key format is `sk-ant-...`

#### Option B: OpenAI

1. Get an API key from [OpenAI Platform](https://platform.openai.com/api-keys)
2. The key format is `sk-...`

#### Add the Secret to Your Repository

1. Go to your repository on GitHub
2. Navigate to **Settings** → **Secrets and variables** → **Actions**
3. Click **New repository secret**
4. Name: `MERITOCRAB_LLM_API_KEY`
5. Value: Paste your API key (Claude or OpenAI)
6. Click **Add secret**

**Security note**: Repository secrets are encrypted and only accessible to workflows running on the base branch. Fork PRs cannot access secrets.

### Step 3: Initialize the Credit State Branch

Meritocrab stores contributor credit scores and event history on a dedicated git branch called `meritocrab-data`. This branch is independent of your main codebase.

#### Option A: Using meritocrab-cli (Recommended)

```bash
# Install the CLI tool
cargo install meritocrab-cli

# Initialize the state branch in your repository
meritocrab-cli state init --repo .

# Push the new branch to GitHub
git push origin meritocrab-data
```

This creates an orphan branch with the following structure:

```
meritocrab-data/
  credit-data/
    contributors.json    # Contributor credit scores
    events.json          # Credit event history
    config.json          # Repository configuration
```

#### Option B: Manual Initialization

If you prefer not to install the CLI locally, the workflows will automatically initialize the state branch on the first PR. However, pre-initializing ensures the branch exists before any PRs arrive.

Manual steps:

```bash
# Create orphan branch
git checkout --orphan meritocrab-data

# Remove all files from staging
git rm -rf .

# Create directory structure
mkdir -p credit-data

# Create empty credit data files
echo '{}' > credit-data/contributors.json
echo '[]' > credit-data/events.json
echo '{"starting_credit":100,"pr_threshold":50,"blacklist_threshold":0}' > credit-data/config.json

# Commit and push
git add credit-data/
git commit -m "chore: initialize meritocrab state branch"
git push origin meritocrab-data

# Return to your main branch
git checkout master  # or main
```

### Step 4: Configure Default Workflow Settings

The workflows use default scoring thresholds defined in `pr-gate.yml`. Review and adjust these if needed:

**Location**: `.github/workflows/pr-gate.yml` — Look for the `credit-check` step:

```yaml
# Default PR threshold (line ~143)
const prThreshold = 50;
```

You can customize this value directly in the workflow file, or override it repository-wide using `.meritocrab.toml` (see [Configuration](#configuration) below).

**LLM Provider**: The workflow defaults to Claude. To use OpenAI, modify the `LLM_CONFIG` environment variable in the `Evaluate PR quality` step:

```yaml
# Change this (line ~183):
LLM_CONFIG: '{"provider":"claude","api_key":"${{ secrets.MERITOCRAB_LLM_API_KEY }}"}'

# To this for OpenAI:
LLM_CONFIG: '{"provider":"openai","api_key":"${{ secrets.MERITOCRAB_LLM_API_KEY }}"}'
```

### Step 5: Commit and Enable Workflows

```bash
# Add workflow files
git add .github/workflows/pr-evaluate.yml
git add .github/workflows/pr-gate.yml
git add .github/workflows/pr-commands.yml

# Commit
git commit -m "chore: add meritocrab GitHub Actions workflows"

# Push to enable workflows
git push origin master  # or main
```

Once pushed, the workflows will trigger automatically on:
- **PR opened/updated**: `pr-evaluate.yml` → `pr-gate.yml`
- **Issue comment created**: `pr-commands.yml` (if comment starts with `/credit`)

## Configuration

### Repository-Wide Configuration

Create a `.meritocrab.toml` file in your repository root to customize scoring thresholds:

```bash
# Copy the example configuration
curl -O https://raw.githubusercontent.com/hydai/meritocrab/master/.meritocrab.toml.example
mv .meritocrab.toml.example .meritocrab.toml
```

Edit `.meritocrab.toml`:

```toml
# Starting credit for new contributors
starting_credit = 100

# Minimum credit required to open PRs
pr_threshold = 50

# Credit level that triggers auto-blacklist
blacklist_threshold = 0

# Credit deltas for PR quality levels
[pr_opened]
spam = -25
low = -5
acceptable = 5
high = 15

# Credit deltas for comments
[comment]
spam = -10
low = -2
acceptable = 1
high = 3

# Merged PR bonus (no LLM evaluation)
[pr_merged]
acceptable = 20
high = 20

# Review submitted bonus
[review_submitted]
acceptable = 5
high = 5
```

Commit the configuration file:

```bash
git add .meritocrab.toml
git commit -m "chore: configure meritocrab scoring thresholds"
git push
```

**Note**: The workflows read `.meritocrab.toml` from the repository root on each run. Changes take effect immediately.

### Workflow Permissions

The workflows require specific GitHub Actions permissions. These are defined in the workflow files:

**pr-evaluate.yml** (unprivileged):
- `pull-requests: read`
- `contents: read`

**pr-gate.yml** and **pr-commands.yml** (privileged):
- `pull-requests: write`
- `issues: write`
- `contents: write` (for state branch updates)

If your repository has restrictive default permissions, you may need to adjust them:

1. Go to **Settings** → **Actions** → **General**
2. Under **Workflow permissions**, select **Read and write permissions**
3. Save changes

## Testing the Setup

### Test 1: Open a Test PR

1. Create a new branch with a trivial change:
   ```bash
   git checkout -b test-meritocrab
   echo "# Test" >> README.md
   git add README.md
   git commit -m "test: verify meritocrab integration"
   git push origin test-meritocrab
   ```

2. Open a PR from this branch

3. Watch the **Actions** tab in your repository:
   - `pr-evaluate.yml` should run first (~30s)
   - `pr-gate.yml` should run after evaluation completes (~1-2 min)

4. Check the PR for a comment from `github-actions` bot with the evaluation result

### Test 2: Check Credit Score

In any PR or issue, post a comment:

```
/credit check @your-username
```

The `pr-commands.yml` workflow will run and reply with your current credit score.

### Test 3: Verify State Branch

```bash
# Fetch the state branch
git fetch origin meritocrab-data

# Check it out to inspect
git checkout meritocrab-data

# View contributor data
cat credit-data/contributors.json

# View event history
cat credit-data/events.json

# Return to main branch
git checkout master  # or main
```

You should see entries for the test PR in both files.

## Customization

### Adjusting Scoring Thresholds

To make the system more or less strict:

**More lenient** (encourage new contributors):
```toml
starting_credit = 150
pr_threshold = 30

[pr_opened]
spam = -15
low = -3
acceptable = 5
high = 20
```

**More strict** (higher quality bar):
```toml
starting_credit = 80
pr_threshold = 70

[pr_opened]
spam = -50
low = -10
acceptable = 3
high = 10
```

### Custom LLM Models

The workflows default to `claude-sonnet-4-5` (via the CLI). To use a different model, modify the `Evaluate PR quality` step in `pr-gate.yml`:

```yaml
# For Claude Haiku (faster, cheaper):
LLM_CONFIG: '{"provider":"claude","api_key":"${{ secrets.MERITOCRAB_LLM_API_KEY }}","model":"claude-3-5-haiku-20241022"}'

# For GPT-4:
LLM_CONFIG: '{"provider":"openai","api_key":"${{ secrets.MERITOCRAB_LLM_API_KEY }}","model":"gpt-4o"}'
```

### Disabling Features

**Disable automatic PR gating** (evaluation only):

Comment out the "Close PR" step in `pr-gate.yml` (lines ~148-173). The workflow will still evaluate and post comments, but won't close PRs.

**Disable `/credit` commands**:

Delete or disable the `pr-commands.yml` workflow file.

## Troubleshooting

### Workflow Not Triggering

**Symptom**: No workflow runs appear in the Actions tab after opening a PR.

**Causes and Solutions**:

1. **Workflows not enabled for the repository**:
   - Go to **Settings** → **Actions** → **General**
   - Ensure "Allow all actions and reusable workflows" is selected
   - Save changes

2. **Workflow files not on the default branch**:
   - Workflows must be committed to the repository's default branch (usually `master` or `main`)
   - Check which branch is default: **Settings** → **Branches**

3. **Syntax errors in workflow YAML**:
   - GitHub will silently skip workflows with YAML errors
   - Validate workflow syntax using [actionlint](https://github.com/rhysd/actionlint):
     ```bash
     actionlint .github/workflows/*.yml
     ```

4. **Concurrency conflict**:
   - If you pushed multiple commits rapidly, the concurrency group may have canceled earlier runs
   - Check the Actions tab for canceled runs
   - This is expected behavior — only the latest run matters

### Artifact Not Found

**Symptom**: `pr-gate.yml` fails with "Artifact not found" error.

**Causes and Solutions**:

1. **pr-evaluate.yml failed**:
   - Check the Actions tab for the `pr-evaluate.yml` run status
   - If it failed, the artifact was never created
   - Common causes: API rate limit, network timeout, or malformed PR

2. **Artifact expired**:
   - Artifacts are retained for 7 days (configurable in `pr-evaluate.yml`)
   - If you re-run `pr-gate.yml` after 7 days, the artifact will be gone
   - Solution: Re-run `pr-evaluate.yml` first to regenerate the artifact

3. **workflow_run timing issue**:
   - Rarely, `pr-gate.yml` may start before the artifact upload completes
   - Solution: Re-run the `pr-gate.yml` workflow manually from the Actions tab

### LLM API Key Not Set or Invalid

**Symptom**: `pr-gate.yml` fails at the "Evaluate PR quality" step with authentication errors.

**Causes and Solutions**:

1. **Secret not configured**:
   - Verify the secret exists: **Settings** → **Secrets and variables** → **Actions**
   - Secret name must be exactly `MERITOCRAB_LLM_API_KEY` (case-sensitive)

2. **Invalid or expired API key**:
   - Test your API key outside GitHub:
     ```bash
     # Claude
     curl https://api.anthropic.com/v1/messages \
       -H "x-api-key: $MERITOCRAB_LLM_API_KEY" \
       -H "anthropic-version: 2023-06-01" \
       -H "content-type: application/json" \
       -d '{"model":"claude-3-5-sonnet-20241022","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}'

     # OpenAI
     curl https://api.openai.com/v1/chat/completions \
       -H "Authorization: Bearer $MERITOCRAB_LLM_API_KEY" \
       -H "Content-Type: application/json" \
       -d '{"model":"gpt-4o","messages":[{"role":"user","content":"Hi"}],"max_tokens":10}'
     ```
   - If these fail, regenerate your API key

3. **Wrong provider in workflow**:
   - If using OpenAI, ensure `LLM_CONFIG` specifies `"provider":"openai"`
   - If using Claude, ensure it specifies `"provider":"claude"`

4. **Rate limit exceeded**:
   - Check your LLM provider's dashboard for rate limit status
   - Reduce concurrent evaluations if hitting limits
   - Consider upgrading your API plan

### State Branch Conflicts

**Symptom**: `pr-gate.yml` or `pr-commands.yml` fails with git push errors like "rejected - non-fast-forward".

**Cause**: Two PRs opened simultaneously both tried to update the `meritocrab-data` branch, and one lost the race.

**Solution**: This is expected and handled automatically by the CLI's retry logic. The workflow will:
1. Pull the latest state
2. Re-apply the credit delta
3. Attempt to push again (up to 3 retries)

If you see this error, simply **re-run the failed workflow** from the Actions tab. It will succeed on the retry.

**Preventing conflicts** (optional):
- Use GitHub's branch protection rules to limit concurrent workflow runs
- Set stricter concurrency groups in the workflow files (may delay PR evaluation)

### Workflow Runs Slow or Times Out

**Symptom**: Workflows take longer than expected or hit the 6-hour timeout.

**Causes and Solutions**:

1. **Rust toolchain installation is slow**:
   - The workflows cache Rust dependencies using `Swatinem/rust-cache`
   - First runs are slow (~5-10 min to install Rust + build CLI)
   - Subsequent runs use cache and complete in ~1-2 min

2. **LLM API latency**:
   - Claude/OpenAI API calls typically take 5-15 seconds
   - Network issues or API outages can cause delays
   - The workflow has `continue-on-error: true` for evaluation — PRs won't be blocked if LLM fails

3. **Large diff size**:
   - PRs with massive diffs (>10KB after truncation) may slow down evaluation
   - The workflow truncates diffs to 10KB to prevent this
   - No action needed — this is expected for large PRs

### Evaluation Comment Not Posted

**Symptom**: Workflow succeeds but no comment appears on the PR.

**Causes and Solutions**:

1. **Insufficient permissions**:
   - Ensure the workflow has `pull-requests: write` and `issues: write` permissions
   - Check **Settings** → **Actions** → **General** → **Workflow permissions**

2. **Comment already exists**:
   - The workflow posts one comment per evaluation
   - If you re-run the workflow, it creates a new comment (doesn't update the existing one)
   - Check older comments on the PR

3. **Evaluation step failed**:
   - Check the "Evaluate PR quality" step logs in the workflow run
   - If this step failed, no comment will be posted
   - Re-run the workflow after fixing the issue

## Migration to Server Mode

If your repository outgrows GitHub Actions mode (e.g., high PR volume, need for maintainer dashboard), you can migrate to server mode.

### Why Migrate?

Consider server mode if you need:
- **Maintainer dashboard**: Web UI for reviewing pending evaluations
- **Admin API**: Programmatic access to credit data
- **Lower latency**: ~1s webhook response vs. ~1-2 min workflow delay
- **Higher rate limits**: No GitHub Actions minutes limit
- **Multi-repo support**: One server handles many repositories

### Migration Process

1. **Export credit data** from the `meritocrab-data` branch:
   ```bash
   git checkout meritocrab-data
   cp -r credit-data/ /tmp/meritocrab-export/
   git checkout master
   ```

2. **Set up Meritocrab server** following the [server setup guide](../SETUP.md)

3. **Import credit data** into the server database:
   ```bash
   # Use the meritocrab-cli import command (if available)
   meritocrab-cli import --from /tmp/meritocrab-export/ --database-url postgres://...

   # Or manually insert JSON data into the database
   ```

4. **Disable GitHub Actions workflows**:
   - Move workflow files out of `.github/workflows/`
   - Or add `if: false` to each workflow's `jobs` section

5. **Configure GitHub App** webhook to point to your server

The core scoring logic is identical between modes, so credit scores and event history transfer seamlessly.

### Hybrid Mode (Advanced)

You can run both modes simultaneously for a transition period:
- Keep Actions workflows enabled for automated PR gating
- Deploy the server for the maintainer dashboard and admin API
- Point both to the same state (database or shared git branch)

However, this requires careful coordination to avoid race conditions. Recommended only for advanced users.

## Rate Limits and Costs

### GitHub Actions Minutes

- **Public repositories**: Unlimited minutes (free)
- **Private repositories**: 2,000 minutes/month (free tier), then paid

Each PR evaluation consumes approximately:
- First run: ~10 minutes (Rust toolchain + CLI build)
- Subsequent runs: ~2 minutes (cached)

For a repository with 10 PRs/day, expect:
- ~600 minutes/month (well within free tier for private repos)
- No cost for public repos

### LLM API Costs

**Claude Sonnet 4.5** (recommended):
- Input: $3 / million tokens
- Output: $15 / million tokens
- Per PR (~1,500 input + 300 output tokens): ~$0.009 (< 1 cent)

**OpenAI GPT-4**:
- Input: $5 / million tokens
- Output: $15 / million tokens
- Per PR (~1,500 input + 300 output tokens): ~$0.012 (1.2 cents)

For a repository with 100 PRs/month:
- Claude: ~$0.90/month
- OpenAI: ~$1.20/month

**Cost optimization**:
- Use Claude Haiku for faster, cheaper evaluations (~70% cost reduction)
- Set a lower `starting_credit` to reduce evaluations for established contributors
- Monitor LLM provider dashboards for spend alerts

## Additional Resources

- [Design Document](../DESIGN-github-actions.md) — Technical details and architecture
- [Main README](../README.md) — Server mode setup and API reference
- [Server Setup Guide](../SETUP.md) — End-to-end server deployment
- [GitHub Actions Security Best Practices](https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions)

## Support

If you encounter issues not covered in this guide:

1. Check the [GitHub Issues](https://github.com/hydai/meritocrab/issues) for known problems
2. Review workflow run logs in your repository's **Actions** tab
3. Open a new issue with:
   - Workflow run URL
   - Error message from logs
   - Repository visibility (public/private)
   - LLM provider (Claude/OpenAI)
