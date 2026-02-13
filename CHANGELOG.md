## 0.1.5 (2026-02-13)

### Features

- add meritocrab-cli crate with evaluate subcommand
- add credit state management subcommands
- add git-branch state backend with retry-on-conflict
- add pr-evaluate workflow for safe fork PR metadata extraction
- add pr-gate workflow for privileged PR evaluation and gating
- add pr-commands workflow for /credit issue comment commands

## 0.1.4 (2026-02-13)

### Fixes

- let cargo own Cargo.lock regeneration instead of knope text-replace

## 0.1.3 (2026-02-13)

### Fixes

- use explicit versions in sub-crates instead of workspace inheritance

## 0.1.2 (2026-02-13)

### Fixes

- align release workflow environment with Trusted Publishing config

## 0.1.1 (2026-02-13)

### Features

- scaffold Cargo workspace and implement credit scoring engine
- implement database layer with migrations and CRUD operations
- implement GitHub webhook verification and API client
- implement LLM evaluator trait with Claude, OpenAI, and mock implementations
- implement PR credit gate webhook endpoint with running server
- wire async LLM evaluation into PR and comment webhooks
- implement shadow blacklist with randomized delay closing
- implement maintainer dashboard API with GitHub OAuth
- implement /credit commands and per-repo configuration
- add production readiness with Docker, enhanced health, and comprehensive documentation
- add meritocrab facade crate and crates.io metadata

### Fixes

- pin knope-dev/action to v2.1.1
- resolve clippy warnings and rustfmt issues for CI
- split knope git add/commit into separate command steps
