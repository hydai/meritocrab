# Open Source Meritocrab System

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
   git clone https://github.com/yourusername/meritocrab.git
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

1. **PR Opened**: Check credit >= threshold → If insufficient, close PR with message
2. **LLM Evaluation**: Async evaluation of content quality
3. **Credit Adjustment**: Apply delta if confidence >= 0.85, else queue for maintainer review
4. **Auto-Blacklist**: If credit <= blacklist_threshold, auto-blacklist contributor
5. **Shadow Enforcement**: Blacklisted PRs closed after randomized delay (30-120s)

## Database Schema

### Tables

- **contributors**: Per-user per-repo credit tracking
- **credit_events**: Immutable audit log of all credit changes
- **pending_evaluations**: Maintainer review queue for low-confidence evaluations
- **repo_configs**: Cached per-repository configuration

### Migrations

Migrations are automatically applied on server startup. Manual migration:

```bash
sqlx migrate run --database-url "your-database-url"
```

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
├── Cargo.toml                 # Workspace root
├── Dockerfile                 # Multi-stage production build
├── docker-compose.yml         # Server + PostgreSQL
├── config.toml                # Server configuration
├── .meritocrab.toml.example # Per-repo config example
└── crates/
    ├── meritocrab-server/     # Entry point, HTTP server setup
    ├── meritocrab-core/       # Credit scoring (pure functions)
    ├── meritocrab-github/     # GitHub API + webhook verification
    ├── meritocrab-llm/        # LLM evaluator trait + implementations
    ├── meritocrab-db/         # Database layer + migrations
    └── meritocrab-api/        # HTTP handlers + middleware
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
  "version": "0.1.0",
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

### Rate Limiting

For production deployments, implement rate limiting at the reverse proxy level (nginx, HAProxy) or API gateway level (AWS API Gateway, Kong). The webhook endpoint receives rate limiting naturally from GitHub's webhook delivery mechanism.

Admin endpoints are protected by GitHub OAuth authentication which provides basic DoS protection.

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

### Metrics

For production monitoring, consider integrating:

- Prometheus for metrics collection
- Grafana for visualization
- Application-level metrics: request latency, LLM evaluation time, credit score distribution

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

### Database connection pool exhausted

Increase max connections in config:

```toml
[database]
max_connections = 20  # Default: 10
```

## Security Considerations

- **Webhook Verification**: All webhooks verified with HMAC-SHA256
- **Authentication**: Admin endpoints protected by GitHub OAuth
- **Secrets**: Never commit `.env`, `config.toml`, or private keys to git
- **Database**: Use strong passwords and restrict network access
- **Shadow Blacklist**: Randomized delays prevent detection of blacklist status

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes with tests
4. Run `cargo fmt` and `cargo clippy`
5. Submit a pull request

## License

MIT License - See LICENSE file for details

## Support

For issues and questions:
- GitHub Issues: https://github.com/yourusername/meritocrab/issues
- Documentation: https://github.com/yourusername/meritocrab/wiki

## Acknowledgments

Built with:
- [Axum](https://github.com/tokio-rs/axum) - Web framework
- [SQLx](https://github.com/launchbadge/sqlx) - Async SQL toolkit
- [Octocrab](https://github.com/XAMPPRocky/octocrab) - GitHub API client
- [Anthropic Claude](https://www.anthropic.com/) / [OpenAI](https://openai.com/) - LLM providers
