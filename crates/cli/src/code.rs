// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! CLI handlers for `terminal-commander code {run, stubs, idioms}` and
//! `terminal-commander q <op> [ARGS_JSON] [-a KEY=VALUE] [--args-file FILE]`.
//!
//! Offline subcommands (`code stubs`, `code idioms`) render directly from
//! `terminal-commander-codemode` and exit 0 without probing a daemon.
//!
//! Live subcommands (`code run`, `q`) honor the CLI honesty contract: they probe
//! the daemon via `ensure_daemon` (`allow_spawn: false`) first, exit 69
//! (`EX_UNAVAILABLE`) with empty stdout when unreachable, and run over a 2-thread
//! multi-thread Tokio runtime backed by [`DaemonClientCaller`] when live.

#![allow(clippy::redundant_pub_crate)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Subcommand};
use serde_json::{Map, Value};
use terminal_commander_codemode::{
    AsyncBridgeBackend, DaemonClientCaller, ExecuteRequest, ExecuteStatus, TcCodeLimits, TcCodeOp,
    dispatch_op_async, execute_script_async, render_idioms, render_stubs,
};
use terminal_commander_supervisor::ensure::{
    Endpoint, EnsureDaemonOptions, EnsureDaemonStatus, ensure_daemon,
};
use terminal_commander_supervisor::paths::{
    endpoint_from_socket_path, resolve_socket_path, resolve_state_dir,
};

use crate::ipc::EX_UNAVAILABLE;

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Subcommands under `terminal-commander code`.
#[derive(Subcommand, Debug)]
pub(crate) enum CodeCommand {
    /// Execute a Python 3.14-subset Code Mode script against the live daemon and print compact JSON.
    ///
    /// Windows `.cmd` shim caution: `cmd.exe` mangles embedded newlines in `-c "..."` arguments.
    /// Prefer `terminal-commander code run -` (pipe via stdin) or `terminal-commander code run script.py`
    /// for multiline scripts.
    Run(CodeRunArgs),
    /// Print Python `TypedDict` stubs for Code Mode functions (offline; no daemon required).
    Stubs {
        /// Optional keyword or function-name filter; omit to list all 60 one-line signatures.
        query: Option<String>,
        /// Maximum full `TypedDict` stubs to print when `query` is provided (default 5, max 60).
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Print the self-contained Code Mode Python idioms manual (offline; no daemon required).
    Idioms,
}

/// Arguments for `terminal-commander code run`.
#[derive(Args, Debug)]
pub(crate) struct CodeRunArgs {
    /// Script source path, or `-` to read Python source from stdin (recommended on Windows `.cmd`).
    #[arg(value_name = "FILE_OR_DASH", conflicts_with = "inline_code")]
    pub source: Option<String>,
    /// Inline one-line Python snippet (caution on Windows `.cmd`: multiline `-c` is mangled by `cmd.exe`; use `-` or a file instead).
    #[arg(short = 'c', long = "code")]
    pub inline_code: Option<String>,
    /// Optional wall-clock execution budget in milliseconds (default 120000, max 300000).
    #[arg(long)]
    pub timeout_ms: Option<u64>,
    /// Optional mutating-call cap override (default 8, max 32).
    #[arg(long)]
    pub max_mutations: Option<usize>,
}

/// Arguments for `terminal-commander q` / `terminal-commander query`.
#[derive(Args, Debug)]
pub(crate) struct QueryArgs {
    /// Granular operation name (one of the 60 TC operations, e.g. `health`, `run_and_watch`, `policy_status`).
    pub op: String,
    /// Optional JSON object string of keyword arguments (e.g. `'{"limit":5}'`).
    #[arg(value_name = "ARGS_JSON")]
    pub args_json: Option<String>,
    /// Individual `KEY=VALUE` argument overrides (VALUE parsed as JSON if valid, else string).
    #[arg(short = 'a', long = "arg", value_name = "KEY=VALUE")]
    pub kv_args: Vec<String>,
    /// Optional path to a JSON file containing an argument object (`-` for stdin).
    #[arg(long = "args-file", value_name = "FILE")]
    pub args_file: Option<PathBuf>,
    /// Optional wall-clock budget in milliseconds (default 30000, max 300000).
    #[arg(long)]
    pub timeout_ms: Option<u64>,
}

pub(crate) fn run_code(cmd: CodeCommand) -> ExitCode {
    match cmd {
        CodeCommand::Stubs { query, limit } => {
            print!("{}", render_stubs(query.as_deref(), limit));
            ExitCode::SUCCESS
        }
        CodeCommand::Idioms => {
            print!("{}", render_idioms());
            ExitCode::SUCCESS
        }
        CodeCommand::Run(args) => run_code_execute(&args),
    }
}

fn read_script_source(args: &CodeRunArgs) -> Result<String, String> {
    if let Some(inline) = &args.inline_code {
        return Ok(inline.clone());
    }
    match args.source.as_deref() {
        Some("-") | None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("failed reading script from stdin: {e}"))?;
            if buf.trim().is_empty() {
                return Err(
                    "empty script on stdin; pipe Python source to `terminal-commander code run -` or pass a file path"
                        .to_owned(),
                );
            }
            Ok(buf)
        }
        Some(path_str) => std::fs::read_to_string(Path::new(path_str))
            .map_err(|e| format!("failed reading script file '{path_str}': {e}")),
    }
}

fn build_multi_thread_runtime() -> Result<tokio::runtime::Runtime, ExitCode> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| {
            eprintln!("terminal-commander: tokio runtime build failed: {e}");
            ExitCode::from(2)
        })
}

async fn probe_live_daemon_endpoint(command_label: &str) -> Result<PathBuf, ExitCode> {
    let state_dir = resolve_state_dir();
    let socket_path = resolve_socket_path();
    let endpoint = endpoint_from_socket_path(&socket_path);
    let opts = EnsureDaemonOptions {
        daemon_binary: PathBuf::from("terminal-commanderd"),
        state_dir: state_dir.clone(),
        log_dir: state_dir.join("logs"),
        endpoint,
        startup_timeout: PROBE_TIMEOUT,
        allow_spawn: false,
    };

    match ensure_daemon(opts).await {
        EnsureDaemonStatus::AlreadyRunning { endpoint, .. }
        | EnsureDaemonStatus::Started { endpoint, .. } => Ok(endpoint_path_buf(&endpoint)),
        EnsureDaemonStatus::Unavailable {
            reason,
            diagnostics,
        } => {
            eprintln!(
                "terminal-commander: {command_label} unavailable: requires live daemon IPC; \
                 refusing to synthesize empty or not-found data."
            );
            eprintln!("  reason   : {reason:?}");
            if let Some(err) = &diagnostics.last_error {
                eprintln!("  detail   : {err}");
            }
            Err(ExitCode::from(EX_UNAVAILABLE))
        }
    }
}

fn endpoint_path_buf(endpoint: &Endpoint) -> PathBuf {
    match endpoint {
        Endpoint::UnixSocket { path } => path.clone(),
        Endpoint::WindowsPipe { name } => PathBuf::from(name),
    }
}

fn run_code_execute(args: &CodeRunArgs) -> ExitCode {
    let source = match read_script_source(args) {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("terminal-commander: code run: {msg}");
            return ExitCode::from(2);
        }
    };

    let mut limits = TcCodeLimits::default();
    if let Some(timeout_ms) = args.timeout_ms {
        limits.max_wall_duration = Duration::from_millis(timeout_ms);
    }
    if let Some(max_mutations) = args.max_mutations {
        limits.max_mutation_calls = max_mutations;
    }
    let request = ExecuteRequest {
        code: source,
        limits: limits.clamped(),
        spill_dir: None,
    };

    let Ok(rt) = build_multi_thread_runtime() else {
        return ExitCode::from(2);
    };

    rt.block_on(async move {
        let endpoint = match probe_live_daemon_endpoint("code run").await {
            Ok(ep) => ep,
            Err(code) => return code,
        };
        let caller = DaemonClientCaller::new(endpoint);
        let backend = AsyncBridgeBackend::new(tokio::runtime::Handle::current(), caller);
        match execute_script_async(request, backend).await {
            Ok(outcome) => {
                match serde_json::to_string(&outcome) {
                    Ok(json_line) => println!("{json_line}"),
                    Err(e) => {
                        eprintln!("terminal-commander: code run serialize failed: {e}");
                        return ExitCode::from(1);
                    }
                }
                match outcome.status {
                    ExecuteStatus::Ok => ExitCode::SUCCESS,
                    ExecuteStatus::Error => ExitCode::from(1),
                }
            }
            Err(err) => {
                eprintln!("terminal-commander: code run failed: {err}");
                ExitCode::from(1)
            }
        }
    })
}

fn parse_kv_value(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Value::String(String::new());
    }
    serde_json::from_str::<Value>(trimmed).unwrap_or_else(|_| Value::String(raw.to_owned()))
}

fn parse_query_kwargs(args: &QueryArgs) -> Result<Map<String, Value>, String> {
    let mut merged = Map::new();

    if let Some(file_path) = &args.args_file {
        let raw = if file_path == Path::new("-") {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("failed reading --args-file from stdin: {e}"))?;
            buf
        } else {
            std::fs::read_to_string(file_path)
                .map_err(|e| format!("failed reading --args-file '{}': {e}", file_path.display()))?
        };
        let parsed: Value = serde_json::from_str(&raw)
            .map_err(|e| format!("--args-file must contain a JSON object: {e}"))?;
        let obj = parsed
            .as_object()
            .ok_or_else(|| "--args-file JSON root must be an object".to_owned())?;
        for (k, v) in obj {
            merged.insert(k.clone(), v.clone());
        }
    }

    if let Some(raw_json) = &args.args_json {
        let parsed: Value = serde_json::from_str(raw_json)
            .map_err(|e| format!("ARGS_JSON must be valid JSON object: {e}"))?;
        let obj = parsed
            .as_object()
            .ok_or_else(|| "ARGS_JSON root must be a JSON object".to_owned())?;
        for (k, v) in obj {
            merged.insert(k.clone(), v.clone());
        }
    }

    for entry in &args.kv_args {
        let Some((key, val)) = entry.split_once('=') else {
            return Err(format!(
                "invalid -a/--arg '{entry}': expected KEY=VALUE (e.g. -a limit=5)"
            ));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(format!("invalid -a/--arg '{entry}': KEY must not be empty"));
        }
        merged.insert(key.to_owned(), parse_kv_value(val));
    }

    Ok(merged)
}

pub(crate) fn run_query(args: &QueryArgs) -> ExitCode {
    let Some(op) = TcCodeOp::from_name(args.op.trim()) else {
        eprintln!(
            "terminal-commander: q: unknown operation '{}'; run `terminal-commander code stubs` to list all 60 operations",
            args.op
        );
        return ExitCode::from(1);
    };

    let kwargs = match parse_query_kwargs(args) {
        Ok(m) => m,
        Err(msg) => {
            eprintln!("terminal-commander: q {}: {msg}", op.as_str());
            return ExitCode::from(2);
        }
    };

    let wall_budget = Duration::from_millis(args.timeout_ms.unwrap_or(30_000).clamp(1, 300_000));
    let label = format!("q {}", op.as_str());

    let Ok(rt) = build_multi_thread_runtime() else {
        return ExitCode::from(2);
    };

    rt.block_on(async move {
        let endpoint = match probe_live_daemon_endpoint(&label).await {
            Ok(ep) => ep,
            Err(code) => return code,
        };
        let caller = DaemonClientCaller::new(endpoint);
        match dispatch_op_async(&caller, op, kwargs, wall_budget).await {
            Ok(val) => match serde_json::to_string(&val) {
                Ok(json_line) => {
                    println!("{json_line}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("terminal-commander: {label} serialize failed: {e}");
                    ExitCode::from(1)
                }
            },
            Err(err) => {
                eprintln!("terminal-commander: {label} failed: {err}");
                ExitCode::from(1)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kv_parser_coerces_json_scalars_and_falls_back_to_string() {
        let args = QueryArgs {
            op: "run_and_watch".to_owned(),
            args_json: Some(r#"{"wait_ms": 1000}"#.to_owned()),
            kv_args: vec![
                "limit=5".to_owned(),
                "compact=true".to_owned(),
                r#"argv=["echo","hi"]"#.to_owned(),
                "tag=plain-string".to_owned(),
            ],
            args_file: None,
            timeout_ms: None,
        };
        let parsed = parse_query_kwargs(&args).expect("parse kwargs");
        assert_eq!(parsed.get("wait_ms"), Some(&json!(1000)));
        assert_eq!(parsed.get("limit"), Some(&json!(5)));
        assert_eq!(parsed.get("compact"), Some(&json!(true)));
        assert_eq!(parsed.get("argv"), Some(&json!(["echo", "hi"])));
        assert_eq!(parsed.get("tag"), Some(&json!("plain-string")));
    }
}
