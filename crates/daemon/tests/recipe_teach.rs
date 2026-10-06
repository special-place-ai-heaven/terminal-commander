// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Shell-misuse denies steer to `recipe_run` only when one activated
//! recipe matches. No match, a draft, or an ambiguous pair keeps argv teach.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{ActivationScope, JobId, RecipeDefinition, RecipeStatus};
use terminal_commanderd::{
    CommandStartParams, CommandStopParams, DaemonClient, DaemonConfig, DaemonState, IpcError,
    IpcErrorCode, IpcRequest, IpcResponse, IpcServer, RecipeActivateParams, RecipeTombstoneParams,
    RecipeUpsertParams, ServerHandle, ShellExecParams,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-recipe-teach-{tag}-{}-{n}", std::process::id()));
    p
}

/// `developer_local` hardened with `[policy.caps] allow_shell = false`: the
/// default profile grants `allow_shell`, so deny tests opt out explicitly.
/// `developer_local` with `allow_shell = false`: the MCP recipe admin gate
/// and the shell deny are both hardening.
fn shell_off_cfg(data: &std::path::Path) -> DaemonConfig {
    let mut cfg = DaemonConfig::defaults_in(data);
    cfg.policy.profile = terminal_commanderd::PolicyProfile::DeveloperLocal;
    cfg.policy.caps = Some(terminal_commanderd::PolicyCapsSection {
        allow_shell: Some(false),
        ..Default::default()
    });
    cfg
}

fn recipe(id: &str, argv: &[&str], tags: &[&str]) -> RecipeDefinition {
    RecipeDefinition {
        recipe_id: id.to_owned(),
        version: 1,
        title: "Run".to_owned(),
        summary: "Argv recipe".to_owned(),
        argv: argv.iter().map(|s| (*s).to_owned()).collect(),
        status: RecipeStatus::Active,
        tags: tags.iter().map(|s| (*s).to_owned()).collect(),
        cwd: None,
        env_allowlist: vec![],
        timeout_ms: None,
        rule_pack_ids: vec![],
        placeholders: vec![],
    }
}

struct Session {
    client: DaemonClient,
    next: u64,
}

impl Session {
    async fn shell(&mut self, line: &str) -> IpcError {
        self.next += 1;
        self.client
            .call(
                self.next,
                IpcRequest::ShellExec(ShellExecParams {
                    shell_line: line.to_owned(),
                    shell: None,
                    cwd: None,
                    env: Vec::new(),
                    rules: Vec::new(),
                    bucket_config: None,
                    tag: None,
                    receipt_shape: None,
                    limits: None,
                }),
            )
            .await
            .expect_err("shell_exec is denied while allow_shell is false")
    }

    async fn upsert(&mut self, definition: RecipeDefinition) {
        self.next += 1;
        self.client
            .call(
                self.next,
                IpcRequest::RecipeUpsert(RecipeUpsertParams { definition }),
            )
            .await
            .expect("upsert");
    }

    async fn activate(&mut self, recipe_id: &str) {
        self.next += 1;
        self.client
            .call(
                self.next,
                IpcRequest::RecipeActivate(RecipeActivateParams {
                    recipe_id: recipe_id.to_owned(),
                    version: None,
                    scope: Some(ActivationScope::Global),
                    from_mcp: false,
                }),
            )
            .await
            .expect("admin activate");
    }

    async fn interpreter(&mut self, argv: &[&str]) -> IpcError {
        self.next += 1;
        self.client
            .call(
                self.next,
                IpcRequest::CommandStartCombed(CommandStartParams {
                    environment: None,
                    argv: argv.iter().map(|s| (*s).to_owned()).collect(),
                    cwd: None,
                    env: Vec::new(),
                    bucket_config: None,
                    rules: Vec::new(),
                    grace_ms: Some(2_000),
                    tag: None,
                    dedup_nonce: None,
                    receipt_shape: None,
                    strip_ansi: true,
                    limits: None,
                }),
            )
            .await
            .expect_err("interpreter argv is denied")
    }
}

fn assert_argv_teach(err: &IpcError) {
    assert_eq!(err.code, IpcErrorCode::PolicyDenied);
    let teach = err.teach.as_ref().expect("A2 teach");
    assert!(teach.recipe_id.is_none(), "no-match must stay argv teach");
    assert!(teach.recipe_scope.is_none(), "argv teach has no scope");
    assert!(!err.message.contains("set allow_shell"));
    assert!(!err.message.to_ascii_lowercase().contains("enable shell"));
}

fn assert_recipe(err: &IpcError, recipe_id: &str) {
    let teach = err.teach.as_ref().expect("A2 teach");
    assert_eq!(teach.recipe_id.as_deref(), Some(recipe_id));
    assert_eq!(teach.recipe_scope, Some(ActivationScope::Global));
    assert!(!err.message.contains("set allow_shell"));
    assert!(!err.message.to_ascii_lowercase().contains("enable shell"));
}

#[test]
#[allow(clippy::too_many_lines)] // one daemon, match and no-match in order
fn shell_deny_steers_to_matching_activated_recipe_only() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("steer");
        let mut cfg = shell_off_cfg(&data);
        cfg.recipe_admin_test_seam = true;
        assert!(cfg.policy.llm_can_activate_recipes.is_none());
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        assert!(!state.policy.llm_can_activate_recipes());
        assert!(!state.policy.caps_allow_shell());
        let handle: ServerHandle = IpcServer::new(Arc::clone(&state), state.config.socket_path())
            .spawn()
            .unwrap();
        let mut session = Session {
            client: DaemonClient::new(handle.socket_path().to_path_buf())
                .with_timeout(Duration::from_secs(5)),
            next: 0,
        };

        session
            .upsert(recipe(
                "cargo.check",
                &["cargo", "check"],
                &["rust", "cargo"],
            ))
            .await;
        session.activate("cargo.check").await;
        let miss = session.shell("git status").await;
        assert_argv_teach(&miss);
        assert_eq!(
            miss.teach.as_ref().expect("teach").deny_class,
            terminal_commander_ipc::ShellDenyClass::ShellCapabilityOff
        );

        session
            .upsert(recipe(
                "git.status",
                &["git", "status", "--short"],
                &["git", "vcs"],
            ))
            .await;
        let inactive = session.shell("git status").await;
        assert_argv_teach(&inactive);

        session.activate("git.status").await;
        let matched = session.shell("git status --short").await;
        assert_eq!(matched.code, IpcErrorCode::PolicyDenied);
        assert_recipe(&matched, "git.status");
        assert_eq!(
            matched.teach.as_ref().expect("teach").denied_tool,
            "shell_exec"
        );
        let wrapped = session
            .shell(r#"powershell.exe -NoProfile -Command "git status""#)
            .await;
        assert_recipe(&wrapped, "git.status");

        session
            .upsert(recipe("git.diff", &["git", "diff"], &["git", "vcs"]))
            .await;
        session.activate("git.diff").await;
        let ambiguous = session.shell("git").await;
        assert_argv_teach(&ambiguous);
        let status = session.shell("git status").await;
        assert_recipe(&status, "git.status");
        let diff = session.shell("git diff --stat").await;
        assert_recipe(&diff, "git.diff");

        let interpreter = session.interpreter(&["bash", "-c", "git status"]).await;
        assert_eq!(interpreter.code, IpcErrorCode::ShellInterpreterDenied);
        assert_recipe(&interpreter, "git.status");
        assert_eq!(
            interpreter.teach.as_ref().expect("teach").denied_tool,
            "command_start_combed"
        );
        assert_eq!(
            interpreter
                .teach
                .as_ref()
                .expect("teach")
                .denied_capability
                .as_deref(),
            Some("allow_shell")
        );

        let other = session.shell("echo a | wc -c").await;
        assert_argv_teach(&other);
        assert!(!state.policy.caps_allow_shell());

        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}

#[test]
fn recipe_tombstone_and_dead_job_scope_do_not_steer() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("steer-life");
        let mut cfg = shell_off_cfg(&data);
        cfg.recipe_admin_test_seam = true;
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let handle = IpcServer::new(Arc::clone(&state), state.config.socket_path())
            .spawn()
            .unwrap();
        let mut session = Session {
            client: DaemonClient::new(handle.socket_path().to_path_buf())
                .with_timeout(Duration::from_secs(5)),
            next: 0,
        };
        session
            .upsert(recipe(
                "git.status",
                &["git", "status", "--short"],
                &["git", "vcs"],
            ))
            .await;
        let job_scope = ActivationScope::Job {
            job_id: JobId::new(),
        };
        assert!(
            state
                .store
                .record_recipe_activation_scoped("git.status", 1, job_scope, None, Some("test"))
                .unwrap()
        );
        let dead = session.shell("git status --short").await;
        assert_argv_teach(&dead);

        session.activate("git.status").await;
        let matched = session.shell("git status --short").await;
        assert_recipe(&matched, "git.status");

        session.next += 1;
        session
            .client
            .call(
                session.next,
                IpcRequest::RecipeTombstone(RecipeTombstoneParams {
                    recipe_id: "git.status".to_owned(),
                }),
            )
            .await
            .expect("tombstone");
        let retired = session.shell("git status --short").await;
        assert_argv_teach(&retired);

        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}

#[test]
fn two_runnable_scopes_fall_back_to_argv() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("steer-scopes");
        let mut cfg = shell_off_cfg(&data);
        cfg.recipe_admin_test_seam = true;
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let handle = IpcServer::new(Arc::clone(&state), state.config.socket_path())
            .spawn()
            .unwrap();
        let mut session = Session {
            client: DaemonClient::new(handle.socket_path().to_path_buf())
                .with_timeout(Duration::from_secs(5)),
            next: 0,
        };
        session
            .upsert(recipe(
                "git.status",
                &["git", "status", "--short"],
                &["git", "vcs"],
            ))
            .await;
        session.activate("git.status").await;
        session.next += 1;
        let started = session
            .client
            .call(
                session.next,
                IpcRequest::CommandStartCombed(CommandStartParams {
                    environment: None,
                    argv: vec!["sleep".to_owned(), "30".to_owned()],
                    cwd: None,
                    env: Vec::new(),
                    bucket_config: None,
                    rules: Vec::new(),
                    grace_ms: Some(2_000),
                    tag: None,
                    dedup_nonce: None,
                    receipt_shape: None,
                    strip_ansi: true,
                    limits: None,
                }),
            )
            .await
            .expect("sleep");
        let IpcResponse::CommandStartCombed(body) = started else {
            panic!("start: {started:?}");
        };
        let job_scope = ActivationScope::Job {
            job_id: body.job_id,
        };
        assert!(
            state
                .store
                .record_recipe_activation_scoped("git.status", 1, job_scope, None, Some("test"))
                .unwrap()
        );
        let matched = session.shell("git status --short").await;
        assert_argv_teach(&matched);
        session.next += 1;
        let _ = session
            .client
            .call(
                session.next,
                IpcRequest::CommandStop(CommandStopParams {
                    job_id: body.job_id,
                }),
            )
            .await;
        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}
