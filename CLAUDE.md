# Meritocrab - Project Instructions

## Overview

Meritocrab is a reputation/credit system for open source repositories. It evaluates contributor quality via LLM and gates PR access behind credit thresholds. Published on crates.io as `meritocrab` (facade) plus 6 sub-crates.

## Build & Test Commands

```bash
cargo build --release          # Always use release builds
cargo test --all               # Run all workspace tests (176 tests)
cargo test -p meritocrab-api   # Run tests for a specific crate
cargo clippy --all             # Lint
cargo fmt --all -- --check     # Format check
```

Tests use in-memory SQLite (`sqlite::memory:`) and require no external services.

## Pre-commit Checklist

Run all three checks before every commit. CI uses `RUSTFLAGS="-Dwarnings"` and `clippy -D warnings`, so any warning is a build failure.

```bash
cargo fmt --all                        # Auto-fix formatting (edition 2024 import sorting)
cargo clippy --all -- -D warnings      # Lint with warnings-as-errors
cargo test --all                       # Run all 176 tests
```

## Project Structure

This is a Cargo workspace with 7 crates (6 sub-crates + 1 facade):

```
Cargo.toml          # Workspace root AND meritocrab facade crate [package]
src/lib.rs          # Facade: re-exports sub-crates as meritocrab::{core,github,db,llm,api}
crates/
  meritocrab-core/    # Pure functions: credit scoring, policy checks, config types. No I/O.
  meritocrab-github/  # GitHub API client (octocrab), webhook HMAC verification, event types.
  meritocrab-db/      # SQLx database layer. Migrations in migrations/001_initial.sql.
  meritocrab-llm/     # LlmEvaluator trait + Claude/OpenAI/Mock implementations.
  meritocrab-api/     # Axum HTTP handlers, webhook processing, admin API, OAuth middleware.
  meritocrab-server/  # Binary entry point. Config loading, server startup, graceful shutdown.
```

### Dependency Graph (bottom-up)

- **Leaf crates** (no internal deps): `meritocrab-core`, `meritocrab-github`
- **Tier 2** (depend on core): `meritocrab-db`, `meritocrab-llm`
- **Tier 3** (depends on all above): `meritocrab-api`
- **Binary** (depends on all): `meritocrab-server`

## Architecture Patterns

- **meritocrab-core** contains pure functions only. No async, no I/O. All scoring logic lives here.
- **meritocrab-llm** uses the `LlmEvaluator` trait (`async-trait`). Add new providers by implementing this trait and registering in `factory.rs`.
- **meritocrab-api** handlers receive `AppState` (via Axum state extraction) which holds the DB pool, GitHub client, LLM evaluator, and config.
- **Tests** in `meritocrab-api/tests/` use `include_str!("../../meritocrab-db/migrations/001_initial.sql")` to set up in-memory SQLite. If migrations change, all test files need the correct path.
- Webhook signature verification uses HMAC-SHA256 with constant-time comparison (`subtle` crate).

## Key Types

- `RepoConfig` (core) - Per-repository scoring configuration, loaded from `.meritocrab.toml`
- `AppState` (api) - Shared server state: `db_pool`, `github_client`, `llm_evaluator`, `webhook_secret`, `oauth_config`, `repo_config`
- `LlmEvaluator` (llm) - Trait for LLM providers. `evaluate(&self, content, context) -> Evaluation`
- `QualityLevel` (core) - Enum: `Spam`, `Low`, `Acceptable`, `High`

## Conventions

- Rust edition 2024, minimum Rust version 1.85
- All internal path dependencies must include `version = "0.1.0"` (crates.io requirement)
- Workspace-level metadata: version, edition, rust-version, license, repository, authors are inherited via `.workspace = true`
- Each crate has a `description` field for crates.io
- Error types per crate: `CoreError`, `DbError`, `GithubError`, `LlmError`, `ApiError`
- Database: SQLite for dev/test, PostgreSQL for production (via `sqlx::any`)

## CI / CD

GitHub Actions workflows in `.github/workflows/`:

- **ci.yml** — Runs `fmt`, `clippy`, `test` in parallel on push/PR to `master`
- **prepare-release.yml** — On push to `master`, Knope creates/updates a release PR with version bumps + changelog
- **release.yml** — When the release PR merges, publishes all crates to crates.io via Trusted Publishing (OIDC)

Release automation is configured in `knope.toml` (single-package mode, all 7 crates versioned in lockstep). The `versioned_files` list includes all 16 inter-crate dependency version strings that need updating on each release.

## Publishing

Publishing is automated via `cargo-release` in the release workflow. Manual publishing should not be needed, but if required, publish in dependency order:

1. `meritocrab-core` + `meritocrab-github` (parallel)
2. `meritocrab-db` + `meritocrab-llm` (parallel)
3. `meritocrab-api`
4. `meritocrab-server`
5. `meritocrab` (facade)

Wait ~30s between tiers for crates.io index propagation.

## Files to Never Commit

- `config.toml` (contains secrets)
- `*.pem` / `*.key` (private keys)
- `.env` (environment secrets)
- `*.db` / `*.db-shm` / `*.db-wal` (SQLite databases)
