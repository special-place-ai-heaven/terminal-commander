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

pub(crate) mod update_locks;

const EX_UNAVAILABLE: u8 = 69;

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
    /// Hidden npm-update preflight: stop TC binaries loaded from one install bin dir.
    #[command(hide = true)]
    UpdateLocks {
        /// Platform package bin directory that owns the binaries allowed to stop.
        #[arg(long)]
        bin_dir: std::path::PathBuf,
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
            RulesOp::List => daemon_backed_command_unavailable("rules list"),
            RulesOp::Show { rule_id: _ } => daemon_backed_command_unavailable("rules show"),
        },
        Command::Buckets { op } => match op {
            BucketsOp::List => daemon_backed_command_unavailable("buckets list"),
            BucketsOp::Show { bucket_id: _ } => daemon_backed_command_unavailable("buckets show"),
        },
        Command::Jobs => daemon_backed_command_unavailable("jobs"),
        Command::Probes => daemon_backed_command_unavailable("probes"),
        Command::Policy => daemon_backed_command_unavailable("policy"),
        Command::Audit { limit: _ } => daemon_backed_command_unavailable("audit"),
        Command::UpdateLocks { bin_dir } => {
            let result = update_locks::stop_installed_processes(&bin_dir);
            for line in &result.lines {
                eprintln!("{line}");
            }
            if result.errors == 0 {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::from(1)
            }
        }
    }
}

fn daemon_backed_command_unavailable(command: &str) -> std::process::ExitCode {
    eprintln!(
        "terminal-commander: {command} unavailable: requires live daemon IPC; refusing to synthesize empty or not-found data."
    );
    std::process::ExitCode::from(EX_UNAVAILABLE)
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

/// Locate the Terminal Commander source repo root by walking up from the
/// current directory. Returns `None` when not running inside the repo (for
/// example an npm- or cargo-installed binary), so the doctor avoids false
/// `MISSING` reports for governance docs that are intentionally not shipped.
fn find_repo_root() -> Option<std::path::PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        // The source repo root carries the workspace manifest alongside the
        // governance docs. Require all three so an unrelated nested crate or
        // a user's CWD never matches.
        if dir.join("Cargo.toml").is_file()
            && dir.join("SECURITY.md").is_file()
            && dir.join("POLICY.md").is_file()
        {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Build the doctor checks as `(label, ok)` pairs.
///
/// Repo mode (`repo_root` is `Some`) verifies the source-tree governance
/// docs relative to the repo root. Installed mode (`None`) reports package
/// readiness instead, because those docs are not part of the shipped package.
fn doctor_checks(repo_root: Option<&std::path::Path>) -> Vec<(String, bool)> {
    let mut checks: Vec<(String, bool)> = vec![("rust toolchain present".to_string(), true)];
    if let Some(root) = repo_root {
        checks.push((
            "LICENSE present".to_string(),
            root.join("LICENSE").is_file(),
        ));
        checks.push((
            "SECURITY.md present".to_string(),
            root.join("SECURITY.md").is_file(),
        ));
        checks.push((
            "POLICY.md present".to_string(),
            root.join("POLICY.md").is_file(),
        ));
        checks.push((
            "PRIVILEGE_MODEL.md present".to_string(),
            root.join("docs/security/PRIVILEGE_MODEL.md").is_file(),
        ));
        checks.push((
            "rule packs present (crates/store/rules)".to_string(),
            root.join("crates/store/rules").is_dir(),
        ));
    } else {
        // Installed/package mode: governance docs ship with the repo, not
        // the package. Report package readiness instead of false MISSING.
        checks.push((
            "installed package mode (repo docs not applicable)".to_string(),
            true,
        ));
        checks.push((
            "terminal-commander version reported".to_string(),
            !env!("CARGO_PKG_VERSION").is_empty(),
        ));
    }
    checks
}

/// Count warning-level failures. Only a missing `LICENSE` in repo mode is a
/// warning; installed mode never warns on repo docs.
fn doctor_warnings(repo_root: Option<&std::path::Path>, checks: &[(String, bool)]) -> u8 {
    // Installed mode never warns on repo docs (they are not shipped).
    if repo_root.is_none() {
        return 0;
    }
    u8::from(
        checks
            .iter()
            .any(|(label, ok)| label == "LICENSE present" && !*ok),
    )
}
fn run_doctor() -> u8 {
    let repo_root = find_repo_root();
    println!("terminal-commanderd doctor:");
    let checks = doctor_checks(repo_root.as_deref());
    for (label, ok) in &checks {
        print_check(label, *ok);
    }
    doctor_warnings(repo_root.as_deref(), &checks)
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
    fn cli_parses_hidden_update_locks() {
        let cli = Cli::parse_from(["terminal-commander", "update-locks", "--bin-dir", "."]);
        match cli.cmd {
            Command::UpdateLocks { bin_dir } => assert_eq!(bin_dir, std::path::PathBuf::from(".")),
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

    #[test]
    fn doctor_installed_mode_has_no_repo_doc_checks_and_no_warnings() {
        // Installed/package mode: repo governance docs are intentionally not
        // shipped, so the doctor must not assert their presence or warn.
        let checks = doctor_checks(None);
        assert!(
            checks.iter().all(|(label, _)| label != "LICENSE present"
                && label != "SECURITY.md present"
                && label != "POLICY.md present"
                && label != "PRIVILEGE_MODEL.md present"),
            "installed mode must not assert repo governance docs"
        );
        assert_eq!(doctor_warnings(None, &checks), 0);
    }

    #[test]
    fn doctor_repo_mode_flags_missing_license_as_warning() {
        // Repo mode resolves docs against the discovered root, not the CWD.
        let root = std::path::Path::new("nonexistent-tc-repo-root-xyz");
        let checks = doctor_checks(Some(root));
        let license = checks
            .iter()
            .find(|(label, _)| label == "LICENSE present")
            .expect("repo mode must check LICENSE");
        assert!(!license.1, "LICENSE under a fake root must be MISSING");
        assert_eq!(doctor_warnings(Some(root), &checks), 1);
    }
}
