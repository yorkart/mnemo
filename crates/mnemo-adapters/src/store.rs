use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use mnemo_ports::StorageError;
use rusqlite::Connection;

use crate::helpers::{add_column_if_missing, lock_err, sql_err};

pub type StoreError = StorageError;

#[derive(Clone)]
pub struct SqliteStore {
    pub(crate) conn: Arc<Mutex<Connection>>,
    pub(crate) artifact_dir: Option<PathBuf>,
    pub(crate) extraction_provider: ExtractionProviderConfig,
}

#[derive(Clone, Debug)]
pub(crate) enum ExtractionProviderConfig {
    LocalRules,
    CodexCli { command: String },
}

impl ExtractionProviderConfig {
    pub(crate) fn from_env() -> Self {
        let provider = std::env::var("MNEMO_EXTRACTION_PROVIDER")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "codex_cli_v1".to_string());
        match provider.trim().to_ascii_lowercase().as_str() {
            "local" | "local_rules" | "local_rules_v1" => Self::LocalRules,
            "codex" | "codex_cli" | "codex_cli_v1" => Self::CodexCli {
                command: std::env::var("MNEMO_CODEX_COMMAND")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(default_codex_command),
            },
            other => panic!("unsupported MNEMO_EXTRACTION_PROVIDER: {other}"),
        }
    }

    pub(crate) fn local_rules() -> Self {
        Self::LocalRules
    }

    #[allow(dead_code)]
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::LocalRules => "local_rules_v1",
            Self::CodexCli { .. } => "codex_cli_v1",
        }
    }
}

fn default_codex_command() -> String {
    let model_arg = std::env::var("MNEMO_CODEX_MODEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|model| format!(" --model {}", shell_quote(&model)))
        .unwrap_or_default();
    format!("codex exec --skip-git-repo-check{model_arg} \"$(cat)\"")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

impl SqliteStore {
    pub fn open(path: &str) -> Result<Self, StoreError> {
        let artifact_dir = std::env::var("MNEMO_ARTIFACT_DIR")
            .ok()
            .filter(|value| !value.is_empty() && value != "off" && value != "disabled")
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from("mnemo-artifacts")));
        Self::open_with_artifact_dir(path, artifact_dir)
    }

    pub fn open_with_artifact_dir(
        path: &str,
        artifact_dir: Option<PathBuf>,
    ) -> Result<Self, StoreError> {
        let conn = Connection::open(path).map_err(sql_err)?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            artifact_dir,
            extraction_provider: ExtractionProviderConfig::from_env(),
        };
        store.init()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self, StoreError> {
        Self::in_memory_with_extraction(ExtractionProviderConfig::local_rules())
    }

    pub(crate) fn in_memory_with_extraction(
        extraction_provider: ExtractionProviderConfig,
    ) -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().map_err(sql_err)?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            artifact_dir: None,
            extraction_provider,
        };
        store.init()?;
        Ok(store)
    }

    fn init(&self) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS events (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              namespace_key TEXT NOT NULL,
              event_id TEXT NOT NULL,
              event_type TEXT,
              role TEXT,
              content TEXT NOT NULL,
              occurred_at TEXT,
              memory_hints TEXT NOT NULL,
              metadata TEXT NOT NULL,
              created_at TEXT NOT NULL,
              eligible INTEGER NOT NULL DEFAULT 1,
              external_context INTEGER NOT NULL DEFAULT 0,
              UNIQUE(namespace_key, event_id)
            );

            CREATE TABLE IF NOT EXISTS memories (
              memory_id TEXT PRIMARY KEY,
              namespace_key TEXT NOT NULL,
              namespace_json TEXT NOT NULL,
              content TEXT NOT NULL,
              origin TEXT NOT NULL,
              memory_type TEXT NOT NULL,
              importance TEXT NOT NULL,
              status TEXT NOT NULL,
              source_event_id TEXT,
              conflict_key TEXT,
              valid_from TEXT,
              valid_until TEXT,
              supersedes_json TEXT NOT NULL DEFAULT '[]',
              superseded_by_json TEXT NOT NULL DEFAULT '[]',
              metadata TEXT NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_memories_namespace ON memories(namespace_key);
            CREATE INDEX IF NOT EXISTS idx_memories_status ON memories(status);
            CREATE INDEX IF NOT EXISTS idx_memories_conflict ON memories(namespace_key, conflict_key);
            CREATE INDEX IF NOT EXISTS idx_memories_temporal ON memories(namespace_key, valid_from, valid_until);

            CREATE TABLE IF NOT EXISTS memory_index (
              memory_id TEXT PRIMARY KEY,
              namespace_key TEXT NOT NULL,
              terms_json TEXT NOT NULL,
              embedding_json TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_memory_index_namespace ON memory_index(namespace_key);

            CREATE TABLE IF NOT EXISTS conflict_suggestions (
              id TEXT PRIMARY KEY,
              namespace_key TEXT NOT NULL,
              conflict_key TEXT NOT NULL,
              candidate_memory_id TEXT NOT NULL,
              existing_memory_id TEXT NOT NULL,
              status TEXT NOT NULL,
              suggestion_json TEXT NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              UNIQUE(namespace_key, conflict_key, candidate_memory_id, existing_memory_id)
            );

            CREATE INDEX IF NOT EXISTS idx_conflict_namespace ON conflict_suggestions(namespace_key, conflict_key, status);

            CREATE TABLE IF NOT EXISTS entities (
              entity_id TEXT PRIMARY KEY,
              namespace_key TEXT NOT NULL,
              name TEXT NOT NULL,
              kind TEXT NOT NULL,
              aliases_json TEXT NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              UNIQUE(namespace_key, name, kind)
            );

            CREATE TABLE IF NOT EXISTS memory_entities (
              memory_id TEXT NOT NULL,
              entity_id TEXT NOT NULL,
              namespace_key TEXT NOT NULL,
              created_at TEXT NOT NULL,
              PRIMARY KEY(memory_id, entity_id)
            );

            CREATE TABLE IF NOT EXISTS relations (
              relation_id TEXT PRIMARY KEY,
              namespace_key TEXT NOT NULL,
              source_entity_id TEXT NOT NULL,
              target_entity_id TEXT NOT NULL,
              relation_type TEXT NOT NULL,
              memory_id TEXT NOT NULL,
              confidence REAL NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              UNIQUE(namespace_key, source_entity_id, target_entity_id, relation_type, memory_id)
            );

            CREATE INDEX IF NOT EXISTS idx_memory_entities_entity ON memory_entities(namespace_key, entity_id);
            CREATE INDEX IF NOT EXISTS idx_relations_namespace_type ON relations(namespace_key, relation_type);

            CREATE TABLE IF NOT EXISTS jobs (
              job_id TEXT PRIMARY KEY,
              namespace_key TEXT NOT NULL,
              namespace_json TEXT NOT NULL,
              job_type TEXT NOT NULL,
              status TEXT NOT NULL,
              result_json TEXT NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS usage_feedback (
              id TEXT PRIMARY KEY,
              namespace_key TEXT NOT NULL,
              payload TEXT NOT NULL,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS usage_aggregate (
              memory_id TEXT PRIMARY KEY,
              namespace_key TEXT NOT NULL,
              usage_count INTEGER NOT NULL DEFAULT 0,
              positive_count INTEGER NOT NULL DEFAULT 0,
              negative_count INTEGER NOT NULL DEFAULT 0,
              last_used_at TEXT,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS policies (
              namespace_key TEXT PRIMARY KEY,
              policy_json TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS outbox_tasks (
              id TEXT PRIMARY KEY,
              namespace_key TEXT NOT NULL,
              task_type TEXT NOT NULL,
              payload TEXT NOT NULL,
              idempotency_key TEXT NOT NULL,
              status TEXT NOT NULL,
              attempts INTEGER NOT NULL DEFAULT 0,
              next_run_at TEXT,
              lease_owner TEXT,
              lease_until TEXT,
              last_error TEXT,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS forgotten_tombstones (
              id TEXT PRIMARY KEY,
              namespace_key TEXT NOT NULL,
              target_json TEXT NOT NULL,
              mode TEXT NOT NULL,
              reason TEXT,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS idempotency (
              scope_key TEXT PRIMARY KEY,
              request_hash TEXT NOT NULL,
              response_json TEXT NOT NULL,
              status TEXT NOT NULL,
              expires_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS context_pack_cache (
              cache_key TEXT PRIMARY KEY,
              namespace_key TEXT NOT NULL,
              response_json TEXT NOT NULL,
              state_version TEXT NOT NULL,
              generated_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS namespace_cursors (
              namespace_key TEXT PRIMARY KEY,
              organized_event_id INTEGER NOT NULL DEFAULT 0,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS audit_log (
              id TEXT PRIMARY KEY,
              namespace_key TEXT NOT NULL,
              action TEXT NOT NULL,
              resource_type TEXT NOT NULL,
              resource_id TEXT,
              metadata_json TEXT NOT NULL,
              request_id TEXT,
              created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_audit_namespace ON audit_log(namespace_key);
            "#,
        )
        .map_err(sql_err)?;
        add_column_if_missing(
            &conn,
            "memories",
            "supersedes_json",
            "TEXT NOT NULL DEFAULT '[]'",
        )?;
        add_column_if_missing(
            &conn,
            "memories",
            "superseded_by_json",
            "TEXT NOT NULL DEFAULT '[]'",
        )?;
        add_column_if_missing(&conn, "outbox_tasks", "next_run_at", "TEXT")?;
        add_column_if_missing(&conn, "outbox_tasks", "lease_owner", "TEXT")?;
        add_column_if_missing(&conn, "outbox_tasks", "lease_until", "TEXT")?;
        Ok(())
    }
}
