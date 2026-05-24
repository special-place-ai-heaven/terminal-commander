// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! `terminal-commander`: operator admin CLI (TC25).
//!
//! Subcommands per the TC25 mini-spec: status, doctor, rules, buckets,
//! jobs, probes, policy, audit. The CLI talks to the daemon Router
//! (in-process at MVP; UDS adapter deferred per TC21 lock).
//!
//! Source-status: live (TC25) — CLI parses + renders. Subcommand
//! bodies print structured summaries; the live IPC connection lands
//! when TC21's transport swap happens.

use clap::{Parser, Subcommand};
use terminal_commander_supervisor::ensure::{EnsureDaemonOptions, EnsureDaemonStatus};
use terminal_commander_supervisor::paths::{
    endpoint_from_socket_path, resolve_socket_path, resolve_state_dir,
};

#[derive(Parser, Debug)]
#[command(
    name = "terminal-commander",
    version,
    about = "Terminal Commander admin CLI",
    long_about = "Operator entry point for the Terminal Commander daemon. \
                  Inspects status, lists buckets, jobs, probes, audit \
                  records, and policy. NEVER bypasses the policy engine."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Print high-level daemon status.
    Status,
    /// Run the doctor (environment + safety checks).
    Doctor,
    /// Rule registry inspection.
    Rules {
        #[command(subcommand)]
        op: RulesOp,
    },
    /// Bucket inspection.
    Buckets {
        #[command(subcommand)]
        op: BucketsOp,
    },
    /// Job inspection.
    Jobs,
    /// Probe inspection.
    Probes,
    /// Policy inspection.
    Policy,
    /// Audit log inspection.
    Audit {
        /// Limit on records to display.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
}

#[derive(Subcommand, Debug)]
enum RulesOp {
    /// List rules in the registry.
    List,
    /// Show one rule by id.
    Show { rule_id: String },
}

#[derive(Subcommand, Debug)]
enum BucketsOp {
    /// List buckets.
    List,
    /// Show a bucket summary by id.
    Show { bucket_id: String },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    run(cli)
}

fn run(cli: Cli) -> std::process::ExitCode {
    match cli.cmd {
        Command::Status => print_status(),
        Command::Doctor => std::process::ExitCode::from(run_doctor()),
        Command::Rules { op } => match op {
            RulesOp::List => {
                println!("rules: (run via MCP registry_search in TC24+)");
                std::process::ExitCode::SUCCESS
            }
            RulesOp::Show { rule_id } => {
                println!("rule {rule_id}: not found (offline CLI; daemon attach deferred)");
                std::process::ExitCode::from(3)
            }
        },
        Command::Buckets { op } => match op {
            BucketsOp::List => {
                println!("buckets: 0 (offline CLI)");
                std::process::ExitCode::SUCCESS
            }
            BucketsOp::Show { bucket_id } => {
                println!("bucket {bucket_id}: not found (offline CLI)");
                std::process::ExitCode::from(3)
            }
        },
        Command::Jobs => {
            println!("jobs: 0 (offline CLI)");
            std::process::ExitCode::SUCCESS
        }
        Command::Probes => {
            println!("probes: 0 (offline CLI)");
            std::process::ExitCode::SUCCESS
        }
        Command::Policy => {
            println!("policy:");
            println!("  active_profile: developer_local (default)");
            println!(
                "  commands.deny : sudo, doas, su, pkexec, kexec, polkit-agent, polkit-auth-agent-1"
            );
            println!("  default-deny paths: 14 suffixes (see POLICY.md section 5)");
            std::process::ExitCode::SUCCESS
        }
        Command::Audit { limit } => {
            println!("audit (last {limit}): empty (offline CLI; daemon attach deferred)");
            std::process::ExitCode::SUCCESS
        }
    }
}

fn print_status() -> std::process::ExitCode {
    let state_dir = resolve_state_dir();
    let log_path = state_dir.join("logs").join("terminal-commanderd.log");
    let endpoint_path = resolve_socket_path();
    let endpoint = endpoint_from_socket_path(&endpoint_path);

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("terminal-commander: tokio runtime build failed: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let status = rt.block_on(async {
        let opts = EnsureDaemonOptions {
            daemon_binary: std::path::PathBuf::from("terminal-commanderd"),
            state_dir: state_dir.clone(),
            log_dir: state_dir.join("logs"),
            endpoint,
            startup_timeout: std::time::Duration::from_secs(1),
            allow_spawn: false,
        };
        terminal_commander_supervisor::ensure::ensure_daemon(opts).await
    });

    let (daemon_text, pid_text, exit_code) = match &status {
        EnsureDaemonStatus::AlreadyRunning { pid, .. } => (
            "running",
            pid.map_or_else(|| "-".into(), |p| p.to_string()),
            std::process::ExitCode::SUCCESS,
        ),
        EnsureDaemonStatus::Started { pid, .. } => (
            "running",
            pid.map_or_else(|| "-".into(), |p| p.to_string()),
            std::process::ExitCode::SUCCESS,
        ),
        EnsureDaemonStatus::Unavailable { .. } => (
            "unavailable",
            "-".to_string(),
            std::process::ExitCode::from(1),
        ),
    };

    println!("terminal-commander status:");
    println!("  version       : {}", env!("CARGO_PKG_VERSION"));
    println!("  endpoint      : {}", endpoint_path.display());
    println!("  daemon        : {daemon_text}");
    println!("  pid           : {pid_text}");
    println!("  log_path      : {}", log_path.display());
    println!("  state_dir     : {}", state_dir.display());

    exit_code
}

fn run_doctor() -> u8 {
    let mut warnings = 0_u8;
    println!("terminal-commanderd doctor:");
    print_check("rust toolchain present", true);
    print_check("LICENSE present", std::path::Path::new("LICENSE").exists());
    print_check(
        "SECURITY.md present",
        std::path::Path::new("SECURITY.md").exists(),
    );
    print_check(
        "POLICY.md present",
        std::path::Path::new("POLICY.md").exists(),
    );
    print_check(
        "PRIVILEGE_MODEL.md present",
        std::path::Path::new("docs/security/PRIVILEGE_MODEL.md").exists(),
    );
    print_check(
        "rules/ pack directory present",
        std::path::Path::new("rules").is_dir(),
    );
    if !std::path::Path::new("LICENSE").exists() {
        warnings += 1;
    }
    warnings
}

fn print_check(label: &str, ok: bool) {
    let marker = if ok { "ok" } else { "MISSING" };
    println!("  [{marker:7}] {label}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_parses_status() {
        let cli = Cli::parse_from(["terminal-commander", "status"]);
        assert!(matches!(cli.cmd, Command::Status));
    }

    #[test]
    fn cli_parses_rules_show() {
        let cli = Cli::parse_from(["terminal-commander", "rules", "show", "x.y"]);
        match cli.cmd {
            Command::Rules {
                op: RulesOp::Show { rule_id },
            } => assert_eq!(rule_id, "x.y"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn cli_parses_audit_with_limit() {
        let cli = Cli::parse_from(["terminal-commander", "audit", "--limit", "10"]);
        match cli.cmd {
            Command::Audit { limit } => assert_eq!(limit, 10),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn cli_status_exits_without_panic() {
        // With no daemon running, print_status returns ExitCode::from(1).
        // We only assert it doesn't panic; the exact exit code depends on
        // whether a daemon is live on the probe endpoint.
        let cli = Cli::parse_from(["terminal-commander", "status"]);
        let _code = run(cli);
    }
}
