# Meritocrab Setup Guide

End-to-end walkthrough for connecting Meritocrab to an existing GitHub repository. By the end of this guide you will have a running instance that evaluates PRs and manages contributor credit.

## Table of Contents

1. [Prerequisites](#1-prerequisites)
2. [Create a GitHub App](#2-create-a-github-app)
3. [Install the GitHub App on Your Repo](#3-install-the-github-app-on-your-repo)
4. [Create OAuth Credentials](#4-create-oauth-credentials-for-maintainer-dashboard)
5. [Configure Meritocrab](#5-configure-meritocrab)
6. [Local Development with Webhook Forwarding](#6-local-development-with-webhook-forwarding)
7. [Build and Run](#7-build-and-run)
8. [Per-Repository Configuration](#8-per-repository-configuration)
9. [Verify End-to-End](#9-verify-end-to-end)
10. [Production Deployment](#10-production-deployment)
11. [Troubleshooting](#11-troubleshooting)

---

## 1. Prerequisites

**Required:**

- **Rust 1.85+** — install via [rustup](https://rustup.rs/)
- **A GitHub account** with admin access to the repo you want to monitor
- **A target repository** — the repo where Meritocrab will evaluate PRs

**Optional (for local development):**

- **ngrok** or **smee.io** — forwards GitHub webhooks to your local machine
- **Docker & Docker Compose** — for containerized deployment

Verify Rust is installed:

```bash
rustc --version   # Should show 1.85.0 or later
cargo --version
```

---

## 2. Create a GitHub App

The GitHub App is how Meritocrab authenticates with the GitHub API, receives webhooks, and interacts with PRs/issues.

1. Go to **https://github.com/settings/apps/new**

2. Fill in the basics:
   - **GitHub App name**: e.g., `meritocrab-myorg` (must be globally unique)
   - **Homepage URL**: your project URL or `https://github.com/hydai/meritocrab`
   - **Webhook URL**: leave as a placeholder for now (e.g., `https://example.com/webhooks/github`). You will update this in [Step 6](#6-local-development-with-webhook-forwarding).
   - **Webhook secret**: generate a secure random string and save it — you will need this for `config.toml`:
     ```bash
     openssl rand -hex 32
     ```

3. Set **permissions**:

   | Permission | Access |
   |---|---|
   | Repository contents | Read |
   | Pull requests | Read & Write |
   | Issues | Read & Write |

4. Subscribe to **events**:
   - Pull request
   - Issue comment
   - Pull request review

5. Under "Where can this GitHub App be installed?", select **Only on this account**.

6. Click **Create GitHub App**.

7. On the app settings page, note the **App ID** (displayed near the top).

8. Scroll down to **Private keys** and click **Generate a private key**. A `.pem` file will download — save it to your Meritocrab project directory (e.g., as `private-key.pem`).

> **Important:** Never commit the `.pem` file to git. It is already in `.gitignore`.

---

## 3. Install the GitHub App on Your Repo

1. From your GitHub App settings page, click **Install App** in the left sidebar.

2. Click **Install** next to your account/organization.

3. Choose **Only select repositories** and pick the target repo. Click **Install**.

4. After installation, look at the browser URL — it will look like:
   ```
   https://github.com/settings/installations/12345678
   ```
   The number at the end (`12345678`) is your **Installation ID**. Save this for `config.toml`.

   Alternatively, you can find it via the GitHub CLI:
   ```bash
   gh api /user/installations --jq '.installations[] | {id, app_slug, target_type}'
   ```

---

## 4. Create OAuth Credentials (for Maintainer Dashboard)

The maintainer dashboard uses GitHub OAuth for authentication. This is a **separate credential** from the GitHub App created above.

1. Go to **https://github.com/settings/developers** and click **OAuth Apps** > **New OAuth App**.

2. Fill in:
   - **Application name**: e.g., `Meritocrab Dashboard`
   - **Homepage URL**: `http://localhost:8080` (or your production URL)
   - **Authorization callback URL**: `http://localhost:8080/auth/callback`

3. Click **Register application**.

4. Note the **Client ID** shown on the next page.

5. Click **Generate a new client secret** and copy it immediately — it is only shown once.

> **Why two credentials?** The GitHub App handles API access and webhooks (using a private key for JWT auth). The OAuth App handles user login for the maintainer dashboard (standard OAuth flow). They serve different purposes.

---

## 5. Configure Meritocrab

Copy the example config and fill in the values collected in the previous steps:

```bash
cp config.example.toml config.toml
```

Edit `config.toml` with the values from Steps 2-4:

```toml
[server]
host = "127.0.0.1"
port = 8080

[database]
# SQLite for local development (no setup needed)
url = "sqlite://meritocrab.db"
max_connections = 10

[github]
app_id = 123456                    # From Step 2.7
installation_id = 789012           # From Step 3.4
private_key_path = "./private-key.pem"  # From Step 2.8
webhook_secret = "abc123..."       # From Step 2.2 (openssl rand output)
oauth_client_id = "Iv1.xxxx"      # From Step 4.4
oauth_client_secret = "xxxx..."   # From Step 4.5
oauth_redirect_url = "http://localhost:8080/auth/callback"

[llm]
# Start with "mock" for initial testing — no API key needed
provider = "mock"

# Maximum concurrent LLM evaluations (controls rate limiting)
max_concurrent_llm_evals = 10

[credit]
starting_credit = 100
pr_threshold = 50
blacklist_threshold = 0
```

> **Tip:** Start with `provider = "mock"` to verify the full pipeline works before switching to a real LLM. The mock evaluator classifies everything as "high" quality by default.

---

## 6. Local Development with Webhook Forwarding

GitHub needs to reach your local machine to deliver webhooks. Use ngrok or smee.io to create a public tunnel.

### Option A: ngrok

```bash
# Install ngrok (macOS)
brew install ngrok

# Start a tunnel to your local server
ngrok http 8080
```

ngrok will display a forwarding URL like:
```
Forwarding  https://a1b2c3d4.ngrok-free.app -> http://localhost:8080
```

Copy the HTTPS URL.

### Option B: smee.io

```bash
# Install smee client
npm install -g smee-client

# Create a channel at https://smee.io — copy the URL
# Then forward events to your local server
smee -u https://smee.io/YOUR_CHANNEL -t http://localhost:8080/webhooks/github
```

### Update the GitHub App Webhook URL

1. Go to **https://github.com/settings/apps** and click your app.
2. In the **Webhook URL** field, set it to:
   ```
   https://a1b2c3d4.ngrok-free.app/webhooks/github
   ```
   (Replace with your actual ngrok or smee URL.)
3. Click **Save changes**.

> **Note:** ngrok URLs change every time you restart ngrok (on the free plan). You will need to update the GitHub App webhook URL each time. smee.io URLs are stable.

---

## 7. Build and Run

```bash
# Build in release mode
cargo build --release

# Run the server
cargo run --release
```

You should see log output indicating the server started:
```
INFO meritocrab_server: Starting server on 127.0.0.1:8080
INFO meritocrab_server: Database connected (sqlite)
INFO meritocrab_server: LLM provider: mock
```

Verify the server is running:

```bash
curl http://localhost:8080/health
```

Expected response:
```json
{
  "status": "healthy",
  "version": "0.1.4",
  "database": {
    "connected": true,
    "driver": "sqlite"
  },
  "llm_provider": {
    "provider": "mock",
    "available": true
  }
}
```

---

## 8. Per-Repository Configuration

Meritocrab supports per-repo scoring overrides via a `.meritocrab.toml` file in the **target repository** (the repo being monitored, not the Meritocrab server repo).

1. Copy the example into your target repo:
   ```bash
   # In your target repository (not the meritocrab server directory)
   cp /path/to/meritocrab/.meritocrab.toml.example .meritocrab.toml
   ```

2. Customize the scoring thresholds as needed:
   ```toml
   # Lower the bar for new contributors
   starting_credit = 150
   pr_threshold = 30

   # Stricter spam penalties
   [pr_opened]
   spam = -50
   low = -10
   acceptable = 5
   high = 20
   ```

3. Commit and push to the target repo:
   ```bash
   git add .meritocrab.toml
   git commit -m "chore: add meritocrab scoring configuration"
   git push
   ```

If no `.meritocrab.toml` exists in the target repo, the defaults from `config.toml`'s `[credit]` section are used.

---

## 9. Verify End-to-End

With the server running and webhook forwarding active, test the full pipeline:

### 9.1 Test Webhook Delivery

1. Open a test PR on the monitored repo (can be a trivial change).
2. Watch the server logs for webhook receipt:
   ```
   INFO meritocrab_api: Received webhook: pull_request (action=opened)
   INFO meritocrab_api: LLM evaluation started for PR #1
   INFO meritocrab_api: Credit adjusted: user_id=12345 delta=+15 new_balance=115
   ```
3. Check GitHub App → Advanced → Recent Deliveries to verify webhooks are being sent and receiving `200` responses.

### 9.2 Check Credit Assignment

Use a maintainer command in any issue or PR comment on the monitored repo:

```
/credit check @<github-user-id>
```

> **Note:** Commands currently require the numeric GitHub user ID, not the username. Find it via:
> ```bash
> gh api /users/USERNAME --jq '.id'
> ```

### 9.3 Test PR Gating

To verify that low-credit contributors are blocked:

1. Use the admin API to lower a test user's credit below the threshold:
   ```bash
   curl -X POST http://localhost:8080/api/repos/OWNER/REPO/contributors/USER_ID/adjust \
     -H "Content-Type: application/json" \
     -d '{"delta": -100, "reason": "testing PR gating"}'
   ```
   (This endpoint requires maintainer authentication via OAuth.)

2. Have that user open a PR — it should be automatically closed with a message.

### 9.4 Test Maintainer Commands

Comment on a PR in the monitored repo:

```
/credit override @12345 +50 "Rewarding excellent documentation"
```

Check the server logs to confirm the override was applied.

---

## 10. Production Deployment

When you are ready to go live, make these changes:

### 10.1 Switch to PostgreSQL

Update `config.toml`:
```toml
[database]
url = "postgres://meritocrab:strong_password@localhost:5432/meritocrab"
max_connections = 20
```

### 10.2 Switch to a Real LLM Provider

```toml
[llm]
provider = "claude"
api_key = "sk-ant-..."
model = "claude-sonnet-4-5-20250929"

# Or OpenAI:
# provider = "openai"
# api_key = "sk-..."
# model = "gpt-4o"
```

### 10.3 Deploy with Docker

```bash
# Set environment variables for secrets
export GITHUB_APP_ID=123456
export GITHUB_INSTALLATION_ID=789012
export GITHUB_WEBHOOK_SECRET="your-secret"
export GITHUB_OAUTH_CLIENT_ID="Iv1.xxxx"
export GITHUB_OAUTH_CLIENT_SECRET="xxxx"
export GITHUB_PRIVATE_KEY_FILE="./private-key.pem"
export LLM_PROVIDER="claude"
export LLM_API_KEY="sk-ant-..."
export LLM_MODEL="claude-sonnet-4-5-20250929"

# Start services (PostgreSQL + Meritocrab server)
docker-compose up -d
```

The Docker setup runs on port **3000** by default (see `docker-compose.yml`).

### 10.4 Update Webhook URL

Replace the ngrok/smee URL with your production URL in the GitHub App settings:

```
https://meritocrab.yourdomain.com/webhooks/github
```

### 10.5 Verify

```bash
curl https://meritocrab.yourdomain.com/health
```

---

## 11. Troubleshooting

### Webhook signature verification fails

- Verify the `webhook_secret` in `config.toml` matches exactly what you set in the GitHub App webhook settings.
- Ensure the GitHub App webhook Content-Type is set to `application/json` (not `application/x-www-form-urlencoded`).
- Check that there are no extra whitespace or newline characters in the secret.

### "No drivers installed" error

Ensure `sqlx::any::install_default_drivers()` is called before creating database pools. This is handled automatically by the server binary.

### Webhooks are not arriving

- Check GitHub App → Advanced → Recent Deliveries for delivery status.
- If using ngrok, verify the tunnel is still running (free plan URLs expire after ~2 hours of inactivity).
- Confirm the webhook URL ends with `/webhooks/github`.

### Wrong installation_id

Symptoms: `404 Not Found` or `403 Forbidden` errors when the server tries to interact with GitHub.

Fix: verify the installation ID matches what GitHub assigned:
```bash
gh api /user/installations --jq '.installations[] | {id, app_slug}'
```

### LLM evaluation timeouts

If evaluations are slow or timing out, increase the concurrency limit:
```toml
max_concurrent_llm_evals = 15
```

Or check that your LLM API key is valid and has sufficient quota.

### OAuth callback fails

- Verify `oauth_redirect_url` in `config.toml` matches the callback URL in your GitHub OAuth App settings exactly (including protocol and port).
- For local dev: `http://localhost:8080/auth/callback`
- For Docker: `http://localhost:3000/auth/callback` (Docker uses port 3000)
- For production: `https://meritocrab.yourdomain.com/auth/callback`

### Database connection issues (PostgreSQL)

- Ensure PostgreSQL is running and accessible.
- Verify the connection URL format: `postgres://user:password@host:port/database`
- If using Docker Compose, the server waits for PostgreSQL to be healthy before starting.

---

## Quick Reference: Where Each Value Comes From

| Config Field | Where to Find It |
|---|---|
| `github.app_id` | GitHub App settings page (top of page) |
| `github.installation_id` | URL after installing the app, or `gh api /user/installations` |
| `github.private_key_path` | Downloaded `.pem` file from GitHub App settings |
| `github.webhook_secret` | You generate this (`openssl rand -hex 32`) and set it in both the GitHub App and `config.toml` |
| `github.oauth_client_id` | GitHub OAuth App settings page |
| `github.oauth_client_secret` | Generated once in GitHub OAuth App settings |
| `llm.api_key` | Your LLM provider's API dashboard (Anthropic or OpenAI) |
