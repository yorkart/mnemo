#![forbid(unsafe_code)]

mod args;
mod commands;
mod http_client;

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(Some(output)) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<Option<String>, String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("health") => commands::health(&args[1..]),
        Some("serve") => commands::serve(&args[1..]),
        Some("event") => commands::event_command(&args[1..]),
        Some("db") => commands::db_command(&args[1..]),
        Some("remember") => commands::remember(&args[1..]),
        Some("context") => commands::context(&args[1..]),
        Some("thread") => commands::thread_command(&args[1..]),
        Some("policy") => commands::policy_command(&args[1..]),
        Some("wrapup") => commands::wrapup(&args[1..]),
        Some("job") => commands::job_command(&args[1..]),
        Some("memory") => commands::memory_command(&args[1..]),
        Some("summary") => commands::summary_command(&args[1..]),
        Some("usage") => commands::usage_command(&args[1..]),
        Some("conflict") => commands::conflict_command(&args[1..]),
        Some("worker") => commands::worker_command(&args[1..]),
        Some("forget") => commands::forget(&args[1..]),
        Some("-h") | Some("--help") | None => {
            print_help();
            Ok(None)
        }
        Some(command) => Err(format!("unknown command: {command}")),
    }
}

fn print_help() {
    println!(
        r#"mnemo

USAGE:
  mnemo health [--remote]
  mnemo serve [--addr 127.0.0.1:8080] [--data ./mnemo-artifacts/store.json] [--sqlite ./mnemo.db] [--wrapup-command ./wrapup-provider]
  mnemo db schema [--dialect sqlite]
  mnemo db init --path ./mnemo.db
  mnemo event add --user u1 [--event-id evt-1] --content "..."
  mnemo event batch --file events.json
  mnemo event search --user u1 [--workspace w1] [--thread t1] [--q text]
  mnemo remember --user u1 --content "..." [--conflict-key key]
  mnemo context --user u1 [--workspace w1] [--max-tokens 1200]
  mnemo policy get --user u1 [--workspace w1]
  mnemo policy set --user u1 --auto-use false --context-pack-max-tokens 800
  mnemo thread memory-mode set --user u1 --thread t1 --mode enabled|disabled|polluted
  mnemo wrapup --user u1 --thread t1 [--source chat] [--async] [--simulate-failure]
  mnemo job get --id job-000001
  mnemo job retry --id job-000001
  mnemo worker run-once [--lease-seconds 300]
  mnemo worker run [--max-jobs 10] [--idle-sleep-ms 1000] [--idle-exit-after 3] [--lease-seconds 300]
  mnemo memory patch --user u1 --id mem-000001 --status inactive
  mnemo memory search --user u1 [--q text] [--include-inactive]
  mnemo usage report --user u1 --context-pack-id ctx --signal positive --memory-id mem-000001
  mnemo usage list --user u1
  mnemo conflict search --user u1
  mnemo conflict resolve --user u1 --conflict-key key --winner-memory-id mem-000001
  mnemo forget --user u1 --memory-id mem-000001

ENV:
  MNEMO_BASE_URL=http://127.0.0.1:8080
  MNEMO_TOKEN=optional-bearer-token
  MNEMO_DATA_PATH=./mnemo-artifacts/store.json
  MNEMO_SQLITE_PATH=./mnemo.db
  MNEMO_WRAPUP_COMMAND=./wrapup-provider"#
    );
}
