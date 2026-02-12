-- Initial schema for social credit system

-- Contributors table: per-user per-repo credit tracking
CREATE TABLE IF NOT EXISTS contributors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    github_user_id INTEGER NOT NULL,
    repo_owner TEXT NOT NULL,
    repo_name TEXT NOT NULL,
    credit_score INTEGER NOT NULL DEFAULT 100,
    role TEXT,
    is_blacklisted INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(github_user_id, repo_owner, repo_name)
);

CREATE INDEX idx_contributors_lookup ON contributors(github_user_id, repo_owner, repo_name);
CREATE INDEX idx_contributors_blacklist ON contributors(is_blacklisted);

-- Credit events table: immutable audit log of all credit changes
CREATE TABLE IF NOT EXISTS credit_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    contributor_id INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    delta INTEGER NOT NULL,
    credit_before INTEGER NOT NULL,
    credit_after INTEGER NOT NULL,
    llm_evaluation TEXT,
    maintainer_override TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (contributor_id) REFERENCES contributors(id) ON DELETE CASCADE
);

CREATE INDEX idx_credit_events_contributor ON credit_events(contributor_id, created_at);

-- Pending evaluations table: maintainer review queue
CREATE TABLE IF NOT EXISTS pending_evaluations (
    id TEXT PRIMARY KEY,
    contributor_id INTEGER NOT NULL,
    repo_owner TEXT NOT NULL,
    repo_name TEXT NOT NULL,
    llm_classification TEXT NOT NULL,
    confidence REAL NOT NULL,
    proposed_delta INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'approved', 'overridden', 'auto_applied')),
    maintainer_note TEXT,
    final_delta INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (contributor_id) REFERENCES contributors(id) ON DELETE CASCADE
);

CREATE INDEX idx_pending_evaluations_status ON pending_evaluations(repo_owner, repo_name, status, created_at);
CREATE INDEX idx_pending_evaluations_contributor ON pending_evaluations(contributor_id);

-- Repo configs table: cached configuration per repository
CREATE TABLE IF NOT EXISTS repo_configs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    owner TEXT NOT NULL,
    repo TEXT NOT NULL,
    config_json TEXT NOT NULL,
    cached_at TEXT NOT NULL DEFAULT (datetime('now')),
    ttl INTEGER NOT NULL,
    UNIQUE(owner, repo)
);

CREATE INDEX idx_repo_configs_lookup ON repo_configs(owner, repo);
