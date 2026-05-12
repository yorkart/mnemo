use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "mnemo", version, about = "Mnemo memory service")]
pub struct Cli {
    #[arg(
        long,
        env = "MNEMO_BASE_URL",
        global = true,
        default_value = "http://127.0.0.1:8787"
    )]
    pub base_url: String,
    #[arg(long, env = "MNEMO_TOKEN", global = true)]
    pub token: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Serve {
        #[arg(long, env = "MNEMO_BIND")]
        bind: Option<std::net::SocketAddr>,
        #[arg(long, env = "MNEMO_SQLITE_PATH")]
        db: Option<std::path::PathBuf>,
        #[arg(long, env = "MNEMO_ARTIFACT_DIR")]
        artifact_dir: Option<std::path::PathBuf>,
    },
    Health {
        #[arg(long)]
        json: bool,
    },
    Ingest {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        text: String,
        #[arg(long)]
        event_id: Option<String>,
        #[arg(long, default_value = "user")]
        role: String,
        #[arg(long)]
        json: bool,
    },
    IngestFile {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        file: std::path::PathBuf,
        #[arg(long)]
        event_id: Option<String>,
        #[arg(long, default_value = "user")]
        role: String,
        #[arg(long)]
        json: bool,
    },
    Remember {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        text: String,
        #[arg(long, default_value = "fact")]
        memory_type: String,
        #[arg(long, default_value = "normal")]
        importance: String,
        #[arg(long)]
        idempotency_key: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Query {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        q: String,
        #[arg(long, default_value_t = 8)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    Context {
        #[arg(long)]
        namespace: String,
        #[arg(long, default_value = "agent_bootstrap")]
        purpose: String,
        #[arg(long, default_value_t = 1200)]
        max_tokens: usize,
        #[arg(long)]
        json: bool,
    },
    Usage {
        #[command(subcommand)]
        command: UsageCommand,
    },
    Wrapup {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        wait: bool,
        #[arg(long, default_value_t = 0)]
        timeout_ms: u64,
        #[arg(long)]
        idempotency_key: Option<String>,
        #[arg(long)]
        extraction_instructions: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Job {
        #[arg(long)]
        namespace: String,
        job_id: String,
        #[arg(long)]
        json: bool,
    },
    Status {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        json: bool,
    },
    Jobs {
        #[arg(long)]
        namespace: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Policy {
        #[command(subcommand)]
        command: PolicyCommand,
    },
    Memories {
        #[command(subcommand)]
        command: MemoriesCommand,
    },
    Events {
        #[command(subcommand)]
        command: EventsCommand,
    },
    Forget {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        memory_id: String,
        #[arg(long, default_value = "soft_delete")]
        mode: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        idempotency_key: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum UsageCommand {
    Report {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        memory_id: String,
        #[arg(long, default_value = "cited")]
        usage: String,
        #[arg(long)]
        response_id: Option<String>,
        #[arg(long)]
        idempotency_key: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum PolicyCommand {
    Get {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        json: bool,
    },
    Set {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        policy_json: Option<String>,
        #[arg(long)]
        auto_organize: Option<bool>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum MemoriesCommand {
    Search {
        #[arg(long)]
        namespace: String,
        #[arg(long, default_value = "")]
        q: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Get {
        #[arg(long)]
        namespace: String,
        memory_id: String,
        #[arg(long)]
        json: bool,
    },
    Patch {
        #[arg(long)]
        namespace: String,
        memory_id: String,
        #[arg(long)]
        content: Option<String>,
        #[arg(long)]
        importance: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum EventsCommand {
    Search {
        #[arg(long)]
        namespace: String,
        #[arg(long, default_value = "")]
        q: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long)]
        json: bool,
    },
}
