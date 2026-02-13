# Design: GitHub Actions Workflow Mode for Meritocrab

## 1. Executive Summary

**Feasibility: Yes** — Meritocrab can run as GitHub Actions workflows instead of a standalone server, but it requires a **two-workflow pattern** for security.

The core challenge is fork PRs. Meritocrab's primary purpose is evaluating *untrusted* external contributors, but GitHub Actions has strict security boundaries around fork PRs to prevent exactly the kind of privileged access Meritocrab needs. The `pull_request` trigger is safe but has no secrets access; `pull_request_target` has secrets but is dangerous when combined with checkout of untrusted code.

The solution: split evaluation (unprivileged) from gating (privileged) into two workflows connected via `workflow_run` and artifacts.

**Key trade-offs vs. current GitHub App approach:**

- **Gains**: Zero infrastructure (no server, domain, SSL, ngrok), adoption via a single workflow file + repo secrets
- **Loses**: Maintainer dashboard, admin API, real-time latency (~1-2 min vs. ~1s), robust SQL-backed state

**Best for**: Small-to-medium repos seeking quick adoption without hosting infrastructure.

## 2. GitHub Actions Security Model for PR Workflows

### `pull_request` vs. `pull_request_target`

| Aspect | `pull_request` | `pull_request_target` |
|---|---|---|
| Workflow source | PR branch (untrusted) | Base branch (trusted) |
| Code context | Merge commit of PR | Base branch HEAD |
| Secrets access | **NO** (fork PRs) | **YES** |
| `GITHUB_TOKEN` scope | Read-only (forks) | Read/Write (even forks) |
| Risk level | Safe | **High** ("pwn request" risk) |

### Why `pull_request_target` + checkout is dangerous

A common anti-pattern:

```yaml
# DANGEROUS — DO NOT DO THIS
on: pull_request_target
jobs:
  evaluate:
    steps:
      - uses: actions/checkout@v4
        with:
          ref: ${{ github.event.pull_request.head.sha }}  # Checks out UNTRUSTED code
      - run: npm install  # Executes attacker-controlled code WITH secrets
```

This gives the fork's code full access to repository secrets and write tokens. An attacker can exfiltrate secrets via a modified `package.json` postinstall script, a compromised dependency, or even a modified workflow step.

The `tj-actions/changed-files` supply chain attack (March 2025) demonstrated this at scale: a compromised Action injected credential-stealing code that ran in thousands of repositories' CI pipelines. GitHub's subsequent security hardening (late 2025) added warnings for this pattern, but the fundamental risk remains — `pull_request_target` + checkout of PR code is inherently unsafe.

**Rule: Never check out or execute PR branch code in a workflow that has secrets access.**

### The `workflow_run` bridge

`workflow_run` triggers after another workflow completes. It:
- Runs on the **base branch** (trusted)
- Has full secrets access
- Can download artifacts from the triggering workflow
- Creates a clean security boundary between untrusted evaluation and privileged actions

## 3. Recommended Architecture — Two-Workflow Pattern

### Overview

```
┌─────────────────────────────────────────────────────────────┐
│  Fork contributor opens PR                                   │
└─────────────┬───────────────────────────────────────────────┘
              │
              ▼
┌─────────────────────────────────────────────────────────────┐
│  Workflow 1: pr-evaluate.yml                                 │
│  Trigger: pull_request (opened, synchronize)                 │
│  Runs on: merge commit (safe, no secrets)                    │
│                                                              │
│  1. Read PR metadata from github.event context               │
│     - title, body, author, diff stats, file list             │
│  2. Fetch diff via GitHub API (read-only GITHUB_TOKEN)       │
│  3. Package evaluation input as JSON artifact                │
│  4. Upload artifact: "pr-evaluation-input"                   │
│                                                              │
│  NO secrets, NO write permissions, NO checkout of PR code    │
└─────────────┬───────────────────────────────────────────────┘
              │ completes
              ▼
┌─────────────────────────────────────────────────────────────┐
│  Workflow 2: pr-gate.yml                                     │
│  Trigger: workflow_run (pr-evaluate completed)               │
│  Runs on: base branch (privileged)                           │
│                                                              │
│  1. Download artifact from Workflow 1                        │
│  2. Validate artifact structure (prevent injection)          │
│  3. Look up contributor credit from state store              │
│  4. If credit < threshold → close PR with generic message    │
│  5. Call LLM API to evaluate PR quality (has secrets)        │
│  6. Calculate credit delta using scoring rules               │
│  7. Update credit state store                                │
│  8. Post evaluation comment on PR                            │
│  9. Auto-blacklist if credit drops below threshold           │
│                                                              │
│  HAS secrets (LLM API key), HAS write permissions            │
└─────────────────────────────────────────────────────────────┘
```

### Why each workflow uses its specific trigger

- **Workflow 1 (`pull_request`)**: Runs safely on every fork PR. Cannot be exploited because it has no secrets and no write access. Its only job is to extract metadata that Workflow 2 needs.

- **Workflow 2 (`workflow_run`)**: Runs trusted code from the base branch. Never checks out the PR branch. Receives only structured JSON data (the artifact) from Workflow 1, not executable code.

### Permissions matrix

| Workflow | `GITHUB_TOKEN` Permissions | Repository Secrets |
|---|---|---|
| `pr-evaluate.yml` | `pull-requests: read`, `contents: read` | None |
| `pr-gate.yml` | `pull-requests: write`, `issues: write`, `contents: write` | LLM API key (`MERITOCRAB_LLM_API_KEY`) |

`contents: write` is needed in Workflow 2 only if using git-branch-based state storage (see Section 4, Option A). If using an external database, `contents: read` suffices.

### Artifact schema

The artifact passed between workflows should be a validated JSON structure:

```json
{
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
}
```

Workflow 2 must validate this schema before processing. Never pass raw artifact values to shell `run:` steps — use `github-script` with JavaScript string handling to prevent injection.

## 4. State Storage Options

The credit system needs persistent state across PRs. A contributor's credit score, event history, and blacklist status must survive between workflow runs.

### Option A: Git branch as database (Recommended)

Store structured JSON on a dedicated `meritocrab-data` branch:

```
meritocrab-data branch:
  credit-data/
    contributors.json     # { "12345678": { "username": "alice", "credit": 85, "is_blacklisted": false } }
    events.json           # [ { "contributor_id": 12345678, "type": "pr_opened", "delta": 5, ... } ]
```

Workflow 2 checks out this branch, reads/updates JSON, and commits back.

| Pros | Cons |
|---|---|
| No external dependencies | Merge conflicts if concurrent PRs (mitigate with retry loop) |
| Full audit trail via git history | Requires `contents: write` permission |
| Survives workflow reruns | JSON not ideal for complex queries |
| Inspectable by maintainers | Branch housekeeping needed (prune old events) |

**Concurrency mitigation**: Use a retry loop — if `git push` fails due to a concurrent update, pull, re-apply the delta, and push again. 3 retries with exponential backoff covers most cases.

### Option B: GitHub Actions cache

Use `actions/cache` with a stable key like `meritocrab-credit-v1`.

| Pros | Cons |
|---|---|
| Simple API | Caches evicted after 7 days of no access |
| Fast read/write | Cannot be shared across `workflow_run` boundary |
| No special permissions | Not designed for persistent state |

**Verdict**: Not viable. Cache eviction makes it unsuitable for a credit ledger, and cross-workflow sharing is unreliable.

### Option C: External database / API

Use the existing Meritocrab server (or a lightweight API) as a state backend.

| Pros | Cons |
|---|---|
| Robust, battle-tested | Defeats the "no server" advantage |
| Matches current architecture | Requires hosting infrastructure |
| Supports complex queries | Network dependency in CI |

**Verdict**: Viable as a hybrid approach (Actions for execution, external DB for state), but loses the main benefit of the Actions mode.

### Option D: Repository variables via GitHub API

Store credit as repo-level variables using the GitHub REST API.

| Pros | Cons |
|---|---|
| Simple for small state | 1KB limit per variable |
| No extra permissions needed | No structured queries |
| Native GitHub integration | Poor audit trail |
| | Doesn't scale beyond ~50 contributors |

**Verdict**: Only suitable for very small repos with few external contributors.

### Recommendation

**Option A (git branch)** is the best balance of simplicity, durability, and zero external dependencies. It preserves Meritocrab's "no infrastructure needed" value proposition while providing a real audit trail.

## 5. What Gets Lost vs. the GitHub App Approach

| Feature | GitHub App (current) | GitHub Actions |
|---|---|---|
| Real-time webhook processing | Instant (~1s) | Delayed (~1-2 min workflow queue + run) |
| Maintainer dashboard (OAuth) | Full web UI at `/auth/github` | **Not available** (would need separate deployment) |
| Admin API endpoints | 7 REST endpoints under `/api/` | **Not available** |
| `/credit check` command | Via issue comment webhook | Feasible (via `issue_comment` trigger, 3rd workflow) |
| `/credit override` command | Via issue comment webhook | Feasible (via `issue_comment` trigger, 3rd workflow) |
| `/credit blacklist` command | Via issue comment webhook | Feasible (via `issue_comment` trigger, 3rd workflow) |
| Shadow blacklist (delayed close) | Randomized 30-120s delay | Natural ~1-2 min workflow delay (less controllable) |
| Pending evaluation review queue | DB-backed with approve/override | **Not available** (could degrade to GitHub Issues) |
| Multi-repo monitoring | Single server handles many repos | Per-repo workflow file copy |
| State durability | SQLite/PostgreSQL (ACID) | Git branch JSON (eventual consistency) |
| Concurrent PR handling | Server handles async (semaphore-limited) | Workflow runs queue per branch |
| LLM eval concurrency control | `max_concurrent_llm_evals` config | No built-in control (one per workflow run) |
| Health monitoring | `/health` endpoint with status | Workflow run history only |
| Structured logging | `tracing` with request IDs | GitHub Actions log output |
| Graceful shutdown | SIGTERM handler, flush pending evals | N/A (stateless runs) |

### Features that translate well

- **PR gating**: Core use case works fully — credit check on PR open, close if insufficient.
- **LLM evaluation**: Works identically — just called from a workflow step instead of an Axum handler.
- **Credit scoring logic**: `meritocrab-core` is pure functions with no I/O — reusable as-is in a CLI binary or WASM module invoked by the workflow.
- **Per-repo config**: `.meritocrab.toml` can be read from the repo root in Workflow 2.
- **Comment commands**: `/credit check` and `/credit override` work via an `issue_comment`-triggered workflow.

### Features that don't translate

- **Maintainer dashboard**: The OAuth-protected web UI (`/auth/github`, admin API) cannot run in Actions. Would require a separate deployment, negating the "no server" benefit.
- **Evaluation review queue**: The current `pending` evaluation state with approve/override endpoints has no natural Actions equivalent. Could be approximated with GitHub Issues as a queue, but the UX would be significantly degraded.

## 6. Security Considerations

### Workflow isolation

1. **Never** use `pull_request_target` with `actions/checkout` of the PR ref. This is the single most important rule.

2. **Never** pass PR metadata directly to shell `run:` steps. PR titles and bodies can contain shell injection payloads:

   ```yaml
   # DANGEROUS
   - run: echo "PR title is ${{ github.event.pull_request.title }}"

   # SAFE — use github-script with JS string handling
   - uses: actions/github-script@v7
     with:
       script: |
         const title = context.payload.pull_request.title;
         core.info(`PR title: ${title}`);
   ```

3. **Pin all Actions to commit SHAs**, not tags:

   ```yaml
   # Good — immutable reference
   - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683  # v4.2.2

   # Risky — tag can be moved to point at malicious code
   - uses: actions/checkout@v4
   ```

### Secret management

- LLM API key stored as a repository secret (`MERITOCRAB_LLM_API_KEY`), accessible only in Workflow 2.
- Never log or echo secrets. Use `::add-mask::` if a value derived from secrets appears in output.
- Workflow 2's `GITHUB_TOKEN` has `contents: write` (for git-branch state). Scope this carefully in the `permissions:` block — don't use the default token permissions.

### Artifact trust boundary

Artifacts from Workflow 1 cross a trust boundary. Workflow 2 must:
- Validate JSON schema before processing
- Reject artifacts with unexpected fields
- Truncate oversized fields (e.g., diff content > 10KB)
- Never deserialize artifacts into executable code

### Supply chain hardening

- Use GitHub's native `actions/upload-artifact` and `actions/download-artifact` — don't use third-party artifact actions.
- Regularly audit pinned action SHAs against known-good releases.
- Consider using GitHub's artifact attestations (available since 2025) for additional integrity verification.

## 7. Limitations and Open Questions

### Concurrent PR race conditions

Two PRs opened simultaneously could both read stale credit data from the git branch. Workflow 2 runs are serialized per workflow but not across different PR events.

**Mitigation**: Retry-on-conflict loop (pull, re-read, re-apply delta, push). Accept that credit may be temporarily inconsistent — eventual consistency is acceptable for a reputation system.

### No maintainer dashboard

The web UI (`/auth/github`, admin API endpoints) cannot run in GitHub Actions. Options:
- Accept the limitation (use `/credit` commands instead)
- Deploy a read-only dashboard as a GitHub Pages site generated from the `meritocrab-data` branch
- Maintain the full server for repos that need the dashboard

### Workflow run latency

`workflow_run` adds ~30-60s delay between Workflow 1 completing and Workflow 2 starting. Combined with Workflow 1's own runtime and LLM API latency, total time from PR open to evaluation comment is ~1-2 minutes.

This is acceptable for most repos — contributors don't expect instant feedback on PRs. However, the shadow blacklist delay (currently randomized 30-120s) becomes less meaningful when the natural workflow delay already provides 1-2 minutes of delay.

### Issue comment commands

`/credit check`, `/credit override`, and `/credit blacklist` commands would need a third workflow triggered by `issue_comment`. This is feasible but adds complexity:

```yaml
# pr-commands.yml
on:
  issue_comment:
    types: [created]
jobs:
  handle-command:
    if: startsWith(github.event.comment.body, '/credit')
    # ... parse and execute command
```

The `issue_comment` trigger runs on the base branch with full permissions, so this is secure.

### Rate limits

| Limit | Value | Impact |
|---|---|---|
| GitHub Actions minutes (free, private) | 2,000 min/month | High-traffic repos could hit this |
| GitHub Actions minutes (free, public) | Unlimited | No issue for open source |
| GitHub API rate limit | 1,000 requests/hour (GITHUB_TOKEN) | Sufficient for most repos |
| Concurrent workflow runs | 20 (free) / 40-180 (paid) | Unlikely bottleneck |

### Migration path

Repos could start with GitHub Actions mode and migrate to the server mode if they outgrow it. The credit data on the `meritocrab-data` branch could be imported into a SQL database. The core scoring logic (`meritocrab-core`) is shared between both modes.

## 8. Comparison Summary

| Criterion | GitHub App (Server) | GitHub Actions |
|---|---|---|
| Setup complexity | High (server, domain, SSL, GitHub App config) | **Low** (workflow files + 1 repo secret) |
| Infrastructure cost | Server hosting required | **Free** (within GitHub Actions limits) |
| Security model | HMAC-SHA256 webhook verification | **GitHub's native workflow isolation** |
| Feature completeness | **Full** (dashboard, API, commands) | Partial (no dashboard, no admin API) |
| Fork PR support | **Full** | Full (with two-workflow pattern) |
| State durability | **Strong** (ACID SQL database) | Moderate (git branch JSON, eventual consistency) |
| Latency | **~1s** (webhook → response) | ~1-2 min (workflow queue + run) |
| Concurrent PR handling | **Async with semaphore control** | Serial per workflow, retry for conflicts |
| Maintainer experience | **Web dashboard + API + commands** | Commands only (via issue comments) |
| Adoption friction | Install GitHub App, configure server | **Copy workflow file, add 1 secret** |
| Multi-repo scaling | **Single server, many repos** | Per-repo workflow copy |
| Best for | Production, high-traffic repos | **Small-medium repos, quick adoption** |

## 9. Recommendation

Implement GitHub Actions mode as a **complementary deployment option**, not a replacement for the server mode.

**Phase 1**: Build a `meritocrab` CLI binary (reusing `meritocrab-core` and `meritocrab-llm`) that can:
- Read evaluation input from a JSON file (the artifact)
- Call the LLM API and return a quality evaluation
- Read/update credit state from a JSON file

**Phase 2**: Create the two workflow templates (`pr-evaluate.yml`, `pr-gate.yml`) that invoke the CLI binary. Publish them as a reusable workflow or a GitHub Actions starter workflow.

**Phase 3**: Add a `pr-commands.yml` workflow for `/credit` issue comment commands.

This approach maximizes code reuse — the scoring logic, LLM evaluation, and credit rules all come from existing crates. The CLI binary is a thin wrapper around `meritocrab-core` and `meritocrab-llm`, similar to how `meritocrab-server` wraps `meritocrab-api`.
