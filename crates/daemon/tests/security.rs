// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Security hardening + fuzz-like tests (TC29).
//!
//! Each test targets one branch of the policy decision algorithm or
//! one invariant from `SECURITY.md` / `docs/security/PRIVILEGE_MODEL.md`.

use std::path::PathBuf;
use terminal_commanderd::{PolicyAction, PolicyDecision, PolicyEngine, PolicyProfile};

#[test]
fn structural_deny_sudo_all_profiles() {
    for prof in [
        PolicyProfile::DeveloperLocal,
        PolicyProfile::RepoOnly,
        PolicyProfile::ReadOnlyObserver,
        PolicyProfile::AdminDebug,
    ] {
        let e = PolicyEngine::new(prof);
        for cmd in ["sudo", "doas", "su", "pkexec", "kexec"] {
            let argv = vec![cmd.to_owned()];
            let cwd = PathBuf::from(".");
            let v = e.evaluate(&PolicyAction::CommandStart {
                argv: &argv,
                cwd: &cwd,
            });
            assert_eq!(v.decision, PolicyDecision::Deny, "{cmd} on {prof:?}");
        }
    }
}

#[test]
fn fully_qualified_sudo_path_also_denied() {
    let e = PolicyEngine::default_engine();
    for path in [
        "/usr/bin/sudo",
        "/usr/local/bin/sudo",
        "/bin/sudo",
        "/sbin/pkexec",
    ] {
        let argv = vec![path.to_owned()];
        let cwd = PathBuf::from(".");
        let v = e.evaluate(&PolicyAction::CommandStart {
            argv: &argv,
            cwd: &cwd,
        });
        assert_eq!(v.decision, PolicyDecision::Deny, "{path}");
    }
}

// Unix absolute-path + glob policy semantics: on Windows, canonicalize_lexical
// resolves these synthetic non-existent paths against the real FS (drive root +
// \\?\ verbatim prefix, backslashes) so the Unix glob regexes miss and the
// verdicts flip. Same class as the policy::tests gated in commit 81b2396; the
// daemon's production target is Linux/WSL (ARCHITECTURE.md section 10). Still
// run on Linux/CI.
#[cfg(unix)]
#[test]
fn sensitive_path_default_deny_paths_all_variants() {
    let e = PolicyEngine::default_engine();
    let paths = [
        "/home/dev/.ssh/id_rsa",
        "/home/dev/.ssh/id_ed25519",
        "/home/dev/.ssh/id_ecdsa",
        "/etc/shadow",
        "/etc/sudoers",
        "/home/dev/.pgpass",
        "/home/dev/.netrc",
        "/home/dev/.aws/credentials",
        "/home/dev/.aws/config",
        "/home/dev/.kube/config",
        "/home/dev/.docker/config.json",
        "/home/dev/.npmrc",
        "/home/dev/.pypirc",
        "/home/dev/.vault-token",
    ];
    for p in paths {
        let pb = PathBuf::from(p);
        let v_read = e.evaluate(&PolicyAction::FileRead { path: &pb });
        assert_eq!(v_read.decision, PolicyDecision::Deny, "{p} read");
        let v_watch = e.evaluate(&PolicyAction::FileWatch { path: &pb });
        assert_eq!(v_watch.decision, PolicyDecision::Deny, "{p} watch");
    }
}

// Unix path-glob semantics -- see the gate comment above.
#[cfg(unix)]
#[test]
fn paths_read_allow_enforced_via_public_api() {
    // TC22 A1: a configured read_allow denies an off-list FileRead with
    // reason no_allow_rule, and allows an on-list one. Exercised through the
    // public `with_paths` builder (the surface the daemon bootstrap uses).
    let e = PolicyEngine::new(PolicyProfile::DeveloperLocal).with_paths(
        &["/home/me/projects/**".to_owned()],
        &[],
        &[],
        &[],
    );
    let hit = PathBuf::from("/home/me/projects/app/src/lib.rs");
    assert_eq!(
        e.evaluate(&PolicyAction::FileRead { path: &hit }).decision,
        PolicyDecision::Allow
    );
    let miss = PathBuf::from("/etc/hosts");
    let v = e.evaluate(&PolicyAction::FileRead { path: &miss });
    assert_eq!(v.decision, PolicyDecision::Deny);
    assert!(
        v.reason.contains("no_allow_rule"),
        "off-allow-list read must carry no_allow_rule: {}",
        v.reason
    );
}

// Unix path-glob semantics -- see the gate comment above.
#[cfg(unix)]
#[test]
fn paths_deny_extra_beats_allow_via_public_api() {
    // TC22 A1: deny_extra is a hard deny that beats the allow-list.
    let e = PolicyEngine::new(PolicyProfile::DeveloperLocal).with_paths(
        &["/home/me/projects/**".to_owned()],
        &[],
        &[],
        &["/home/me/projects/**/target/**".to_owned()],
    );
    let denied = PathBuf::from("/home/me/projects/app/target/debug/secret");
    let v = e.evaluate(&PolicyAction::FileRead { path: &denied });
    assert_eq!(v.decision, PolicyDecision::Deny);
    assert!(
        v.reason.contains("default_deny_match"),
        "deny_extra must carry default_deny_match: {}",
        v.reason
    );
}

#[test]
fn paths_empty_lists_preserve_zero_config_allow_via_public_api() {
    // Regression: unconfigured path lists do not enforce; zero-config reads
    // and watches still pass.
    let e = PolicyEngine::new(PolicyProfile::DeveloperLocal).with_paths(&[], &[], &[], &[]);
    let p = PathBuf::from("/some/ordinary/file.rs");
    assert_eq!(
        e.evaluate(&PolicyAction::FileRead { path: &p }).decision,
        PolicyDecision::Allow
    );
    assert_eq!(
        e.evaluate(&PolicyAction::FileWatch { path: &p }).decision,
        PolicyDecision::Allow
    );
}

#[test]
fn read_only_observer_denies_every_mutation() {
    let e = PolicyEngine::new(PolicyProfile::ReadOnlyObserver);
    let argv = vec!["cargo".to_owned()];
    let cwd = PathBuf::from(".");
    let mutations = [
        PolicyAction::CommandStart {
            argv: &argv,
            cwd: &cwd,
        },
        PolicyAction::CommandStdin,
        PolicyAction::CommandSignal,
        PolicyAction::RegistryCreate,
        PolicyAction::RegistryActivate,
    ];
    for a in &mutations {
        let v = e.evaluate(a);
        assert_eq!(v.decision, PolicyDecision::Deny, "{a:?}");
    }
}

#[test]
fn admin_debug_denies_registry_mutations() {
    let e = PolicyEngine::new(PolicyProfile::AdminDebug);
    for a in [PolicyAction::RegistryCreate, PolicyAction::RegistryActivate] {
        let v = e.evaluate(&a);
        assert_eq!(v.decision, PolicyDecision::Deny);
    }
}

#[test]
fn pattern_redos_caught_by_validation_or_size_limit() {
    use regex::RegexBuilder;
    // Pattern that would blow up an unbounded DFA. The combined
    // 1024-alternation regex pushes past our 64 KiB size_limit.
    use std::fmt::Write as _;
    let mut pat = String::from("^(");
    for i in 0..1024 {
        if i > 0 {
            pat.push('|');
        }
        let _ = write!(pat, "a{i}b{i}c{i}d{i}e{i}");
    }
    pat.push_str(")$");
    let r = RegexBuilder::new(&pat).size_limit(65_536).build();
    assert!(r.is_err(), "size_limit must reject oversize pattern");
}

#[test]
fn mcp_crate_contains_no_command_spawn() {
    // Grep-style guard: the MCP crate must NOT directly spawn
    // processes (PRIVILEGE_MODEL.md section 3).
    let src = std::fs::read_to_string("../mcp/src/lib.rs").expect("read mcp lib");
    assert!(
        !src.contains("Command::new"),
        "terminal-commander-mcp must not use Command::new"
    );
    assert!(
        !src.contains("std::process::Command"),
        "terminal-commander-mcp must not import std::process::Command"
    );
}

#[test]
fn mcp_crate_contains_no_tcp_listener() {
    let src = std::fs::read_to_string("../mcp/src/lib.rs").expect("read mcp lib");
    // No TCP / UDP listener in the MCP crate (PRIVILEGE_MODEL.md
    // section 9: "Open network socket: NO").
    assert!(
        !src.contains("TcpListener"),
        "no TcpListener allowed in MCP"
    );
    assert!(!src.contains("UdpSocket"), "no UdpSocket allowed in MCP");
}
