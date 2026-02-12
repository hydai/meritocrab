# Open Source Social Credit — Implementation Plan

## Context

Build a reputation/credit system for open source repositories that grades contributors based on contribution quality. The system uses LLM evaluation to detect spam and reward quality, gates PR submissions behind a credit threshold, and provides a shadow blacklist to silently handle bad actors without alerting them. Maintainers, committers, and reviewers bypass all checks.

**Stack**: Rust, Axum, SQLite/PostgreSQL, GitHub Webhooks, Configurable LLM (Claude/OpenAI)

---

## Architecture Overview

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

---

## Workspace Structure

```
socialcredit/
  Cargo.toml                    # workspace root
  config.example.toml
  crates/
    sc-server/src/main.rs       # entry point
    sc-core/src/
      credit.rs                 # scoring engine (pure functions)
      policy.rs                 # gate check, blacklist logic
      evaluation.rs             # evaluation state machine
      config.rs                 # RepoConfig, ServerConfig
      error.rs
    sc-github/src/
      webhook.rs                # HMAC verification extractor
      api.rs                    # close PR, add comment, check roles
      types.rs                  # event payload types
      auth.rs                   # GitHub App token management
    sc-llm/src/
      traits.rs                 # LlmEvaluator trait
      claude.rs                 # Claude API impl
      openai.rs                 # OpenAI API impl
      prompt.rs                 # evaluation prompt templates
      mock.rs                   # deterministic mock for tests
    sc-db/src/
      contributors.rs           # contributor CRUD
      credit_events.rs          # audit log
      evaluations.rs            # pending evaluation queue
      repo_configs.rs           # config cache
    sc-db/migrations/
      001_initial.sql
    sc-api/src/
      webhook_handler.rs        # POST /webhooks/github
      admin_handlers.rs         # maintainer dashboard
      auth_middleware.rs        # GitHub OAuth
      state.rs                  # AppState (DI root)
      error.rs                  # ApiError -> HTTP response
```

---

## Database Schema

Four tables:
- **contributors** — per-user per-repo: `github_user_id`, `credit_score` (default 100), `role`, `is_blacklisted`
- **credit_events** — immutable audit log: `event_type`, `delta`, `credit_before/after`, `llm_evaluation` (JSON), `maintainer_override`
- **pending_evaluations** — maintainer review queue: `llm_classification`, `confidence`, `proposed_delta`, `status` (pending/approved/overridden)
- **repo_configs** — cached per-repo `.socialcredit.toml`

---

## Credit Scoring

| Event | Spam | Low Quality | Acceptable | High Quality |
|-------|------|-------------|------------|--------------|
| PR opened | -25 | -5 | +5 | +15 |
| Comment | -10 | -2 | +1 | +3 |
| PR merged | — | — | — | +20 |
| Review submitted | — | — | — | +5 |

- Starting credit: **100**
- PR threshold: **50** (below = PRs auto-closed)
- Auto-blacklist at: **0**
- All values configurable per-repo via `.socialcredit.toml`

---

## Core Workflows

### PR Opened
1. Verify HMAC -> lookup/create contributor -> check role (bypass if maintainer)
2. Check blacklist -> shadow-close with generic message if blacklisted (randomized 30-120s delay)
3. Check credit >= threshold -> close with "build your score" message if insufficient
4. Spawn async LLM eval -> apply delta if confidence >= 0.85, else queue for maintainer review

### Comment Created
1. Verify HMAC -> lookup contributor -> skip credit for privileged roles
2. Blacklisted users: comment stays but earns no credit
3. Spawn async LLM eval -> adjust credit or queue

### Maintainer Review
- **Dashboard API**: `GET /api/repos/{owner}/{repo}/evaluations?status=pending` -> review queue
- **Comment commands**: `/credit override @user +10 "reason"`, `/credit blacklist @user`, `/credit check @user`

### Shadow Blacklist
- PRs closed after randomized delay with generic message
- Comments silently ignored for credit
- No user-facing indication of blacklist status
- Auto-triggered when credit <= 0

---

## Key Dependencies

| Purpose | Crate | Version |
|---------|-------|---------|
| HTTP framework | `axum` | `0.8.8` |
| HTTP middleware | `tower` | `0.5.3` |
| HTTP middleware (extras) | `tower-http` | `0.6.8` |
| GitHub API | `octocrab` | `0.49.5` |
| Database | `sqlx` (sqlite + postgres + any) | `0.8.6` |
| LLM HTTP calls | `reqwest` | `0.13.2` |
| HMAC verification | `hmac` + `sha2` + `subtle` | `0.12.1` + `0.10.9` + `2.6.1` |
| Hex encoding | `hex` | `0.4.3` |
| Secret management | `secrecy` | `0.10.3` |
| Serialization | `serde` + `serde_json` | `1.0.228` + `1.0.149` |
| Config (TOML) | `toml` | `0.8.22` |
| Config (layered) | `config` | `0.15.19` |
| Errors | `thiserror` + `anyhow` | `2.0.18` + `1.0.101` |
| Logging | `tracing` + `tracing-subscriber` | `0.1.44` + `0.3.20` |
| Async runtime | `tokio` | `1.49.0` |
| Async trait | `async-trait` | `0.1.89` |
| Time | `chrono` | `0.4.42` |
| Testing (HTTP mock) | `wiremock` | `0.6.5` |

---

## Implementation Phases

### Phase 1 — MVP Foundation
1. Scaffold Cargo workspace with 6 crates
2. `sc-core`: credit config, scoring formula, policy evaluation (pure functions + unit tests)
3. `sc-db`: SQLite migrations, contributor/credit_event CRUD
4. `sc-github`: HMAC signature verification extractor
5. `sc-api`: webhook handler for `pull_request` opened — credit gate only
6. `sc-server`: config loading, DB init, axum server startup

**Deliverable**: Running server that gates PRs based on credit scores.

### Phase 2 — LLM Integration
1. `sc-llm`: trait definition, prompt templates, Claude + OpenAI implementations, mock
2. Wire LLM eval into webhook handler (async spawn, semaphore-limited)
3. Handle `issue_comment` events with credit adjustment
4. Pending evaluation queue

### Phase 3 — Maintainer Dashboard
1. GitHub OAuth middleware
2. Admin API endpoints (review queue, override, manual adjust, blacklist mgmt)
3. Per-repo `.socialcredit.toml` loading + caching

### Phase 4 — Shadow Blacklist & Polish
1. Randomized-delay PR closing for blacklisted users
2. Auto-blacklist on credit <= 0
3. `/credit` comment commands
4. Role detection via GitHub collaborator API
5. Rate limiting, health endpoint, structured logging

### Phase 5 — Production Readiness
1. Dockerfile (multi-stage), Docker Compose (server + PostgreSQL)
2. PostgreSQL compatibility testing
3. README, setup guide, API docs
4. End-to-end tests with wiremock

---

## Key Design Decisions

1. **6-crate workspace** — `sc-core` has zero I/O deps, trivially testable. Compile-time enforcement of boundaries.
2. **Webhook returns 200 immediately** — LLM eval is async. Avoids GitHub retry storms from slow LLM calls.
3. **Credit gate uses existing score, not current LLM result** — Synchronous DB lookup for gating, async LLM for score adjustment. No blocking on LLM.
4. **Shadow blacklist uses randomized delays** — 30-120s delay before closing looks organic, not automated.
5. **Per-repo config via GitHub API** — Repo is source of truth. Cached with TTL.
6. **sqlx `Any` driver** — Runtime database switching (SQLite dev, PostgreSQL prod). Trade-off: no compile-time query checking, compensated by integration tests.

---

## Verification Plan

1. **Unit tests**: `cargo test -p sc-core` — credit calculations, policy evaluation
2. **Integration tests**: `cargo test -p sc-api` — full webhook flow with mock LLM + in-memory SQLite
3. **Manual E2E**:
   - Start server locally -> configure a test repo webhook -> open PR -> verify credit check behavior
   - Test with mock LLM first, then real Claude/OpenAI API
4. **Shadow blacklist test**: Set user credit to 0 -> open PR -> verify delayed close with generic message
5. **Maintainer bypass test**: Add user as repo collaborator -> open PR -> verify no credit check
