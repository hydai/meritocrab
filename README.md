# Meritocrab

[![crates.io](https://img.shields.io/crates/v/meritocrab.svg)](https://crates.io/crates/meritocrab)
[![CI](https://github.com/hydai/meritocrab/actions/workflows/ci.yml/badge.svg)](https://github.com/hydai/meritocrab/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A reputation/credit system for open source repositories that grades contributors based on contribution quality using LLM evaluation. The system gates PR submissions behind a credit threshold and provides tools for maintainers to manage contributor reputation.

## Features

- **Automated Credit Scoring**: LLM-powered evaluation of PRs and comments
- **PR Gating**: Contributors below credit threshold cannot open PRs
- **Shadow Blacklist**: Graceful handling of bad actors without alerting them
- **Maintainer Dashboard**: Web interface for reviewing evaluations and managing contributors
- **GitHub Integration**: Seamless integration via GitHub Apps and webhooks
- **Flexible Configuration**: Per-repository custom scoring via `.meritocrab.toml`
- **Role-Based Bypass**: Maintainers and collaborators exempt from checks
- **Audit Trail**: Complete history of all credit changes with maintainer overrides
- **GitHub Actions Mode**: Run Meritocrab without a server using GitHub Actions workflows

## Deployment Options

Meritocrab can be deployed in two modes:

### Server Mode (Full-Featured)

Deploy Meritocrab as a standalone server with a GitHub App. Best for production, high-traffic repositories, and teams that need the full feature set.

**Pros:**
- Complete feature set (maintainer dashboard, admin API, OAuth)
- Real-time webhook processing (~1s latency)
- Robust SQL-backed state storage (SQLite/PostgreSQL)
- Multi-repository support from a single server
- Advanced concurrency control for LLM evaluations

**Cons:**
- Requires server infrastructure (hosting, domain, SSL)
- More complex setup (GitHub App creation, webhook configuration)
- Ongoing maintenance and monitoring

**Setup**: See [Server Setup Guide](SETUP.md)

### GitHub Actions Mode (Zero Infrastructure)

Run Meritocrab using GitHub Actions workflows. Best for small-to-medium repositories that want quick adoption without managing infrastructure.

**Pros:**
- Zero infrastructure (no server, domain, or SSL needed)
- Simple setup (copy 3 workflow files + 1 repository secret)
- Free for public repositories (unlimited Actions minutes)
- Git-branch-based state storage (full audit trail via git history)
- Automatic scaling with GitHub's workflow runners

**Cons:**
- No maintainer dashboard or admin API
- Higher latency (~1-2 min workflow queue + run time)
- Limited to single-repository scope
- Eventual consistency for credit state (retry-on-conflict)
- Requires Rust ecosystem for CLI binary

**Setup**: See [GitHub Actions Setup Guide](docs/github-actions-setup.md)

### Comparison Table

| Feature | Server Mode | GitHub Actions Mode |
|---------|-------------|---------------------|
| Setup complexity | High | **Low** |
| Infrastructure cost | Server hosting required | **Free** (within GitHub limits) |
| Latency | **~1s** (webhook) | ~1-2 min (workflow) |
| Maintainer dashboard | **Yes** | No |
| Admin API | **Yes** | No |
| `/credit` commands | **Yes** | **Yes** |
| State durability | **Strong** (SQL ACID) | Moderate (git branch) |
| Multi-repo support | **Yes** (one server) | No (per-repo workflows) |
| Best for | Production, high-traffic repos | **Quick adoption, small-medium repos** |

**Migration path**: You can start with GitHub Actions mode and migrate to server mode later if your repository outgrows it. Credit data transfers seamlessly between modes.

## Installation

### As a library

```toml
# Use the facade crate for everything
[dependencies]
meritocrab = "0.1"

# Or depend on individual crates
[dependencies]
meritocrab-core = "0.1"
meritocrab-llm = "0.1"
```

### As a server

```bash
cargo install meritocrab-server
```

## Architecture

```
GitHub Webhooks (PR, Comment, Review)
        |
        v
+---------------------+
|  Axum HTTP Server    |  <- HMAC-SHA256 verification
|  /webhooks/github    |
+--------+------------+
         |
    +----+----+
    v         v
 GitHub    LLM Evaluator     <- trait-based (Claude, OpenAI, Mock)
  API       (async task)
(octocrab)    |
    |         v
    |    Credit Engine       <- pure functions, no I/O
    |         |
    +----+----+
         v
   Database (sqlx)           <- SQLite dev / PostgreSQL prod
         ^
         |
   Maintainer API            <- Admin endpoints + GitHub OAuth
```

## Quick Start

### Prerequisites

- Rust 1.85 or later
- Docker and Docker Compose (for containerized deployment)
- GitHub App with webhook and API access

### Local Development

1. **Clone the repository**:
   ```bash
   git clone https://github.com/hydai/meritocrab.git
   cd meritocrab
   ```

2. **Configure the application**:
   ```bash
   cp config.example.toml config.toml
   # Edit config.toml with your settings
   ```

3. **Set up GitHub App**:
   - Create a GitHub App at https://github.com/settings/apps
   - Set webhook URL to `https://yourdomain.com/webhooks/github`
   - Enable permissions: Repository contents (read), Pull requests (read/write), Issues (read/write)
   - Subscribe to events: Pull request, Issue comment, Pull request review
   - Download private key and save as `private-key.pem`

4. **Run migrations and start server**:
   ```bash
   cargo run --release
   ```

### Docker Deployment

1. **Configure environment**:
   ```bash
   cp .env.example .env
   # Edit .env with your GitHub App credentials
   ```

2. **Start services**:
   ```bash
   docker-compose up -d
   ```

3. **Verify health**:
   ```bash
   curl http://localhost:3000/health
   ```

## Configuration

### Server Configuration (`config.toml`)

```toml
[server]
host = "0.0.0.0"
port = 3000

[database]
url = "postgres://user:password@localhost/meritocrab"
max_connections = 10

[github]
app_id = 123456
installation_id = 7654321
webhook_secret = "your-webhook-secret"
private_key_path = "private-key.pem"
oauth_client_id = "your-oauth-client-id"
oauth_client_secret = "your-oauth-client-secret"
oauth_redirect_url = "http://localhost:3000/auth/callback"

[llm]
provider = "claude"  # claude, openai, or mock
api_key = "your-api-key"
model = "claude-3-5-sonnet-20241022"

[credit]
starting = 100
pr_threshold = 50
blacklist_threshold = 0

max_concurrent_llm_evals = 5
```

### Per-Repository Configuration (`.meritocrab.toml`)

Place this file in the root of your repository to customize scoring:

```toml
# Starting credit for new contributors
starting_credit = 100

# Minimum credit required to open PRs
pr_threshold = 50

# Credit level that triggers auto-blacklist
blacklist_threshold = 0

# PR opened scoring deltas
[pr_opened]
spam = -25
low = -5
acceptable = 5
high = 15

# Comment scoring deltas
[comment]
spam = -10
low = -2
acceptable = 1
high = 3

# PR merged bonus (no LLM evaluation)
[pr_merged]
bonus = 20

# Review submitted bonus (no LLM evaluation)
[review_submitted]
bonus = 5
```

## API Endpoints

### Public Endpoints

- `GET /health` - Health check with server status
- `POST /webhooks/github` - GitHub webhook receiver (HMAC verified)

### Authentication Endpoints

- `GET /auth/github` - Initiate GitHub OAuth flow
- `GET /auth/callback` - OAuth callback handler
- `POST /auth/logout` - Logout current session

### Admin API (Requires Maintainer Role)

- `GET /api/repos/:owner/:repo/evaluations?status=pending` - List pending evaluations
- `POST /api/repos/:owner/:repo/evaluations/:id/approve` - Approve evaluation
- `POST /api/repos/:owner/:repo/evaluations/:id/override` - Override evaluation with custom delta
- `GET /api/repos/:owner/:repo/contributors` - List all contributors
- `POST /api/repos/:owner/:repo/contributors/:user_id/adjust` - Manually adjust credit
- `POST /api/repos/:owner/:repo/contributors/:user_id/blacklist` - Toggle blacklist status
- `GET /api/repos/:owner/:repo/events` - View credit event history

## Maintainer Commands

Maintainers can use special comments in GitHub issues/PRs:

### Check Credit

```
/credit check @12345
```

Returns credit score, role, blacklist status, and last 5 credit events.

### Override Credit

```
/credit override @12345 +20 "Excellent contribution with thorough tests"
```

Adjusts credit by specified delta with reason. Auto-blacklists if credit drops to/below threshold.

### Manual Blacklist

```
/credit blacklist @12345
```

Immediately blacklists contributor. Future PRs will be shadow-closed.

**Note**: Commands currently require numeric GitHub user ID instead of username.

## Credit Scoring

| Event | Spam | Low Quality | Acceptable | High Quality |
|-------|------|-------------|------------|--------------|
| PR opened | -25 | -5 | +5 | +15 |
| Comment | -10 | -2 | +1 | +3 |
| PR merged | — | — | — | +20 |
| Review submitted | — | — | — | +5 |

### Workflow

1. **PR Opened**: Check credit >= threshold -> If insufficient, close PR with message
2. **LLM Evaluation**: Async evaluation of content quality
3. **Credit Adjustment**: Apply delta if confidence >= 0.85, else queue for maintainer review
4. **Auto-Blacklist**: If credit <= blacklist_threshold, auto-blacklist contributor
5. **Shadow Enforcement**: Blacklisted PRs closed after randomized delay (30-120s)

## Development

### Running Tests

```bash
# All tests
cargo test --all

# Specific crate
cargo test -p meritocrab-api

# With output
cargo test -- --nocapture
```

### Project Structure

```
meritocrab/
├── Cargo.toml                 # Workspace root + meritocrab facade crate
├── src/lib.rs                 # Facade: re-exports all sub-crates
├── LICENSE                    # MIT
├── Dockerfile                 # Multi-stage production build
├── docker-compose.yml         # Server + PostgreSQL
├── config.example.toml        # Server configuration template
├── .meritocrab.toml.example   # Per-repo config example
└── crates/
    ├── meritocrab-core/       # Credit scoring (pure functions, no I/O)
    ├── meritocrab-github/     # GitHub API + webhook verification
    ├── meritocrab-llm/        # LLM evaluator trait + implementations
    ├── meritocrab-db/         # Database layer + migrations
    ├── meritocrab-api/        # HTTP handlers + middleware
    └── meritocrab-server/     # Entry point, HTTP server setup
```

### Crate Dependency Graph

```
meritocrab-core       meritocrab-github     (leaf crates, no internal deps)
      |                     |
      +-------+-------+     |
              |       |     |
       meritocrab-db  meritocrab-llm
              |       |     |
              +---+---+-----+
                  |
           meritocrab-api
                  |
           meritocrab-server (binary)
```

### Adding a New LLM Provider

1. Implement `meritocrab_llm::LlmEvaluator` trait
2. Add configuration in `meritocrab_llm::create_evaluator()`
3. Update config example with new provider option

## Production Deployment

### Docker

The provided `Dockerfile` uses multi-stage builds for optimal image size:

```bash
# Build
docker build -t meritocrab:latest .

# Run
docker run -p 3000:3000 \
  -e DATABASE_URL=postgres://... \
  -e GITHUB_APP_ID=123456 \
  -v /path/to/private-key.pem:/app/private-key.pem:ro \
  meritocrab:latest
```

### Health Checks

The `/health` endpoint returns comprehensive status:

```json
{
  "status": "healthy",
  "version": "0.1.3",
  "uptime_seconds": 3600,
  "database": {
    "connected": true,
    "driver": "postgres"
  },
  "llm_provider": {
    "provider": "claude",
    "available": true
  }
}
```

### Graceful Shutdown

The server handles SIGTERM gracefully:

1. Stop accepting new requests
2. Complete in-flight webhook processing
3. Flush pending LLM evaluations to database
4. Close database connections

## Monitoring

### Logs

Structured logging with `tracing`:

- Request IDs for correlation
- Webhook events with type and contributor
- LLM evaluations with timing and classification
- Errors with full context

Configure log level via `RUST_LOG` environment variable:

```bash
RUST_LOG=info cargo run
RUST_LOG=debug,sqlx=warn cargo run  # Debug app, warn for sqlx
```

## Troubleshooting

### "No drivers installed" error

Ensure `sqlx::any::install_default_drivers()` is called before creating database pools.

### Webhook signature verification fails

Verify:
1. Webhook secret in GitHub App matches `GITHUB_WEBHOOK_SECRET`
2. Webhook URL is correctly configured
3. Content-Type is `application/json`

### LLM evaluation timeouts

Increase semaphore limit in config:

```toml
max_concurrent_llm_evals = 10  # Default: 5
```

## Security Considerations

- **Webhook Verification**: All webhooks verified with HMAC-SHA256
- **Authentication**: Admin endpoints protected by GitHub OAuth
- **Secrets**: Never commit `.env`, `config.toml`, or private keys to git
- **Database**: Use strong passwords and restrict network access
- **Shadow Blacklist**: Randomized delays prevent detection of blacklist status

## Releases

Releases are automated via [Knope](https://knope.tech/) and GitHub Actions:

1. Push conventional commits to `master` (e.g., `feat: add rate limiting`, `fix: handle timeout`)
2. The **prepare-release** workflow creates a release PR with version bumps and changelog
3. Merge the release PR to publish all 7 crates to crates.io and create a GitHub release

All crate versions stay in sync. Publishing uses [Trusted Publishing](https://doc.rust-lang.org/cargo/reference/registry-authentication.html#trusted-publishing) (OIDC) — no API tokens required.

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes with tests
4. Run `cargo fmt` and `cargo clippy`
5. Use [Conventional Commits](https://www.conventionalcommits.org/) for commit messages
6. Submit a pull request — CI will run fmt, clippy, and tests automatically

## License

MIT License - See [LICENSE](LICENSE) file for details.

## Support

- GitHub Issues: https://github.com/hydai/meritocrab/issues

## Acknowledgments

Built with:
- [Axum](https://github.com/tokio-rs/axum) - Web framework
- [SQLx](https://github.com/launchbadge/sqlx) - Async SQL toolkit
- [Octocrab](https://github.com/XAMPPRocky/octocrab) - GitHub API client
- [Anthropic Claude](https://www.anthropic.com/) / [OpenAI](https://openai.com/) - LLM providers
