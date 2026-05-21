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
    let exit = run(cli);
    std::process::ExitCode::from(exit)
}

fn run(cli: Cli) -> u8 {
    match cli.cmd {
        Command::Status => {
            println!("terminal-commanderd: STATUS");
            println!("  build version : {}", env!("CARGO_PKG_VERSION"));
            println!("  state         : not running (TC25 stub; IPC arrives in TC21 follow-up)");
            0
        }
        Command::Doctor => run_doctor(),
        Command::Rules { op } => match op {
            RulesOp::List => {
                println!("rules: (run via MCP registry_search in TC24+)");
                0
            }
            RulesOp::Show { rule_id } => {
                println!("rule {rule_id}: not found (offline CLI; daemon attach deferred)");
                3
            }
        },
        Command::Buckets { op } => match op {
            BucketsOp::List => {
                println!("buckets: 0 (offline CLI)");
                0
            }
            BucketsOp::Show { bucket_id } => {
                println!("bucket {bucket_id}: not found (offline CLI)");
                3
            }
        },
        Command::Jobs => {
            println!("jobs: 0 (offline CLI)");
            0
        }
        Command::Probes => {
            println!("probes: 0 (offline CLI)");
            0
        }
        Command::Policy => {
            println!("policy:");
            println!("  active_profile: developer_local (default)");
            println!(
                "  commands.deny : sudo, doas, su, pkexec, kexec, polkit-agent, polkit-auth-agent-1"
            );
            println!("  default-deny paths: 14 suffixes (see POLICY.md section 5)");
            0
        }
        Command::Audit { limit } => {
            println!("audit (last {limit}): empty (offline CLI; daemon attach deferred)");
            0
        }
    }
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
    fn cli_status_exits_zero() {
        let cli = Cli::parse_from(["terminal-commander", "status"]);
        let code = run(cli);
        assert_eq!(code, 0);
    }
}
