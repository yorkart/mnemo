PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;

CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS namespace_policies (
    namespace_key TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    workspace_id TEXT,
    thread_id TEXT,
    agent_id TEXT,
    source TEXT,
    auto_generate_memories INTEGER NOT NULL DEFAULT 1,
    auto_use_memories INTEGER NOT NULL DEFAULT 1,
    context_pack_max_tokens INTEGER NOT NULL DEFAULT 1200,
    external_context_policy TEXT NOT NULL DEFAULT 'ignore_unless_explicit',
    conflict_resolution_mode TEXT NOT NULL DEFAULT 'supersede_by_conflict_key',
    max_unused_days INTEGER,
    max_thread_age_days INTEGER,
    min_thread_idle_seconds INTEGER,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS thread_states (
    namespace_key TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    workspace_id TEXT,
    thread_id TEXT NOT NULL,
    agent_id TEXT,
    source TEXT,
    memory_mode TEXT NOT NULL CHECK (memory_mode IN ('enabled', 'disabled', 'polluted')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS thread_state_changes (
    change_id INTEGER PRIMARY KEY AUTOINCREMENT,
    namespace_key TEXT NOT NULL,
    old_memory_mode TEXT,
    new_memory_mode TEXT NOT NULL,
    reason TEXT,
    changed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    namespace_key TEXT NOT NULL,
    event_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    workspace_id TEXT,
    thread_id TEXT,
    agent_id TEXT,
    source TEXT,
    event_type TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    eligible INTEGER NOT NULL DEFAULT 0,
    explicit_memory_intent INTEGER NOT NULL DEFAULT 0,
    external_context INTEGER NOT NULL DEFAULT 0,
    fingerprint TEXT NOT NULL,
    inserted_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(namespace_key, event_id)
);

CREATE TABLE IF NOT EXISTS memories (
    memory_id TEXT PRIMARY KEY,
    namespace_key TEXT NOT NULL,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    workspace_id TEXT,
    thread_id TEXT,
    agent_id TEXT,
    source TEXT,
    content TEXT NOT NULL,
    memory_type TEXT NOT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('explicit', 'inferred', 'manual')),
    status TEXT NOT NULL CHECK (status IN ('active', 'inactive', 'superseded', 'conflicted', 'expired', 'forgotten')),
    importance TEXT NOT NULL CHECK (importance IN ('low', 'normal', 'high', 'critical')),
    conflict_key TEXT,
    valid_from TEXT,
    supersedes_json TEXT NOT NULL DEFAULT '[]',
    superseded_by_json TEXT NOT NULL DEFAULT '[]',
    source_event_ids_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS memory_versions (
    version_id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id TEXT NOT NULL,
    content TEXT NOT NULL,
    memory_type TEXT NOT NULL,
    origin TEXT NOT NULL,
    status TEXT NOT NULL,
    importance TEXT NOT NULL,
    conflict_key TEXT,
    valid_from TEXT,
    source_event_ids_json TEXT NOT NULL DEFAULT '[]',
    version_reason TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY(memory_id) REFERENCES memories(memory_id)
);

CREATE TABLE IF NOT EXISTS memory_relations (
    relation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    from_memory_id TEXT NOT NULL,
    to_memory_id TEXT NOT NULL,
    relation_type TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(from_memory_id, to_memory_id, relation_type),
    FOREIGN KEY(from_memory_id) REFERENCES memories(memory_id),
    FOREIGN KEY(to_memory_id) REFERENCES memories(memory_id)
);

CREATE TABLE IF NOT EXISTS session_summaries (
    session_summary_id TEXT PRIMARY KEY,
    namespace_key TEXT NOT NULL,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    workspace_id TEXT,
    thread_id TEXT,
    agent_id TEXT,
    source TEXT,
    content TEXT NOT NULL,
    source_event_ids_json TEXT NOT NULL DEFAULT '[]',
    inferred_memory_ids_json TEXT NOT NULL DEFAULT '[]',
    generated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS jobs (
    job_id TEXT PRIMARY KEY,
    job_type TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
    namespace_key TEXT NOT NULL,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    workspace_id TEXT,
    thread_id TEXT,
    agent_id TEXT,
    source TEXT,
    query TEXT,
    generate_memories INTEGER,
    attempts INTEGER NOT NULL DEFAULT 0,
    retry_at TEXT,
    lease_until TEXT,
    error TEXT,
    output_summary_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS context_pack_cache_entries (
    cache_key TEXT PRIMARY KEY,
    context_pack_id TEXT NOT NULL,
    version TEXT NOT NULL,
    generated_at TEXT NOT NULL,
    namespace_key TEXT,
    content TEXT NOT NULL,
    max_tokens INTEGER NOT NULL,
    estimated_tokens INTEGER NOT NULL,
    budget_exceeded_items INTEGER NOT NULL DEFAULT 0,
    conflicted_items INTEGER NOT NULL DEFAULT 0,
    items_json TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE IF NOT EXISTS usage_reports (
    usage_id TEXT PRIMARY KEY,
    namespace_key TEXT NOT NULL,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    workspace_id TEXT,
    thread_id TEXT,
    agent_id TEXT,
    source TEXT,
    context_pack_id TEXT NOT NULL,
    signal TEXT NOT NULL CHECK (signal IN ('positive', 'neutral', 'negative')),
    memory_ids_json TEXT NOT NULL DEFAULT '[]',
    notes TEXT,
    reported_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS usage_citations (
    citation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    usage_id TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    signal TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY(usage_id) REFERENCES usage_reports(usage_id)
);

CREATE TABLE IF NOT EXISTS forget_tombstones (
    tombstone_id TEXT PRIMARY KEY,
    namespace_key TEXT NOT NULL,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    workspace_id TEXT,
    thread_id TEXT,
    agent_id TEXT,
    source TEXT,
    memory_ids_json TEXT NOT NULL DEFAULT '[]',
    contents_json TEXT NOT NULL DEFAULT '[]',
    reason TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_events_namespace_inserted ON events(namespace_key, id);
CREATE INDEX IF NOT EXISTS idx_events_namespace_thread ON events(namespace_key, thread_id, id);
CREATE INDEX IF NOT EXISTS idx_memories_namespace_status ON memories(namespace_key, status);
CREATE INDEX IF NOT EXISTS idx_memories_namespace_conflict ON memories(namespace_key, conflict_key);
CREATE INDEX IF NOT EXISTS idx_memories_namespace_importance ON memories(namespace_key, memory_type, importance);
CREATE INDEX IF NOT EXISTS idx_session_summaries_namespace_generated ON session_summaries(namespace_key, generated_at);
CREATE INDEX IF NOT EXISTS idx_jobs_status_type ON jobs(status, job_type, updated_at);
CREATE INDEX IF NOT EXISTS idx_usage_namespace_reported ON usage_reports(namespace_key, reported_at);
CREATE INDEX IF NOT EXISTS idx_forget_namespace_created ON forget_tombstones(namespace_key, created_at);

INSERT OR IGNORE INTO schema_migrations(version, name) VALUES (1, '0001_init');
PRAGMA user_version = 1;
