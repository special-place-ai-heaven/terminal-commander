// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! recipe_run stays on the argv lane, and MCP activate/deactivate is denied
//! while `llm_can_activate_recipes` is false.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{ActivationScope, RecipeDefinition, RecipeStatus};
use terminal_commanderd::ipc::protocol::{AuditSinceParams, AuditSinceResponse};
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcErrorCode, IpcRequest, IpcResponse, IpcServer,
    RecipeActivateParams, RecipeDeactivateParams, RecipeRunParams, RecipeTestParams,
    RecipeUpsertParams,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!(
        "tc-ipc-recipe-run-{tag}-{}-{n}",
        std::process::id()
    ));
    p
}

fn recipe(id: &str, argv: Vec<String>, placeholders: Vec<String>) -> RecipeDefinition {
    RecipeDefinition {
        recipe_id: id.to_owned(),
        version: 1,
        title: "Run".to_owned(),
        summary: "Argv recipe".to_owned(),
        argv,
        status: RecipeStatus::Active,
        tags: vec![],
        cwd: None,
        env_allowlist: vec![],
        timeout_ms: None,
        rule_pack_ids: vec![],
        placeholders,
    }
}

fn assert_admin_denied(err: &terminal_commanderd::IpcError) {
    assert_eq!(err.code, IpcErrorCode::PolicyDenied);
    assert!(
        err.message.contains("recipe_activate_requires_admin"),
        "{}",
        err.message
    );
}

#[test]
#[allow(clippy::too_many_lines)] // one IPC lifecycle: deny, argv run, shell deny, audit
fn recipe_run_denies_mcp_activate_and_stays_on_argv() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("gate");
        let cfg = DaemonConfig::defaults_in(&data);
        assert!(!cfg.policy.llm_can_activate_recipes);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        assert!(!state.policy.llm_can_activate_recipes());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));
        let scope = ActivationScope::Global;

        let upserted = client
            .call(
                1,
                IpcRequest::RecipeUpsert(RecipeUpsertParams {
                    definition: recipe("echo.true", vec!["true".to_owned()], vec![]),
                }),
            )
            .await
            .unwrap();
        assert!(matches!(upserted, IpcResponse::RecipeUpsert(_)));

        let listed = client
            .call(
                2,
                IpcRequest::RecipeListActive(terminal_commanderd::ListLimitParams { limit: None }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeListActive(active) = listed else {
            panic!("list: {listed:?}");
        };
        assert!(active.entries.is_empty(), "upsert must not activate");

        let tested = client
            .call(
                3,
                IpcRequest::RecipeTest(RecipeTestParams {
                    definition: Some(recipe("echo.true", vec!["true".to_owned()], vec![])),
                    recipe_id: None,
                    version: None,
                    fills: BTreeMap::new(),
                    expect_argv0: Some("true".to_owned()),
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeTest(dry) = tested else {
            panic!("test: {tested:?}");
        };
        assert!(dry.ok && dry.expects_met && !dry.activated);
        assert_eq!(dry.argv, ["true"]);

        let denied_act = client
            .call(
                4,
                IpcRequest::RecipeActivate(RecipeActivateParams {
                    recipe_id: "echo.true".to_owned(),
                    version: None,
                    scope: Some(scope),
                    from_mcp: true,
                }),
            )
            .await
            .unwrap_err();
        assert_admin_denied(&denied_act);

        let denied_deact = client
            .call(
                5,
                IpcRequest::RecipeDeactivate(RecipeDeactivateParams {
                    recipe_id: "echo.true".to_owned(),
                    version: Some(1),
                    scope: Some(scope),
                    from_mcp: true,
                }),
            )
            .await
            .unwrap_err();
        assert_admin_denied(&denied_deact);

        let not_active = client
            .call(
                6,
                IpcRequest::RecipeRun(RecipeRunParams {
                    recipe_id: "echo.true".to_owned(),
                    version: None,
                    scope: Some(scope),
                    fills: BTreeMap::new(),
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(not_active.code, IpcErrorCode::RecipeNotActive);
        assert!(not_active.message.contains("recipe_list_active"));

        client
            .call(
                7,
                IpcRequest::RecipeActivate(RecipeActivateParams {
                    recipe_id: "echo.true".to_owned(),
                    version: None,
                    scope: Some(scope),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();

        let ran = client
            .call(
                8,
                IpcRequest::RecipeRun(RecipeRunParams {
                    recipe_id: "echo.true".to_owned(),
                    version: None,
                    scope: Some(scope),
                    fills: BTreeMap::new(),
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeRun(body) = ran else {
            panic!("run: {ran:?}");
        };
        assert_eq!(body.lane, "argv");
        assert_eq!(body.argv, ["true"]);
        assert!(!body.watched);

        let mut shell = recipe("slot.bin", vec!["{bin}".to_owned()], vec!["bin".to_owned()]);
        shell.recipe_id = "slot.bin".to_owned();
        client
            .call(
                9,
                IpcRequest::RecipeUpsert(RecipeUpsertParams { definition: shell }),
            )
            .await
            .unwrap();
        client
            .call(
                10,
                IpcRequest::RecipeActivate(RecipeActivateParams {
                    recipe_id: "slot.bin".to_owned(),
                    version: None,
                    scope: Some(scope),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let mut fills = BTreeMap::new();
        fills.insert("bin".to_owned(), "bash".to_owned());
        let shell_denied = client
            .call(
                11,
                IpcRequest::RecipeRun(RecipeRunParams {
                    recipe_id: "slot.bin".to_owned(),
                    version: None,
                    scope: Some(scope),
                    fills,
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(shell_denied.code, IpcErrorCode::ShellInterpreterDenied);

        // emit_audit stores `ipc_{method}`, so the distinct row is ipc_recipe_run.
        let recipe_audit = audit_hits(&client, "ipc_recipe_run").await;
        assert!(
            !recipe_audit.is_empty(),
            "recipe_run must emit its own audit action"
        );
        assert!(
            recipe_audit.iter().any(|row| {
                row.actor.as_deref() == Some("admin")
                    && row
                        .metadata_json
                        .as_deref()
                        .is_some_and(|meta| meta.contains("echo.true"))
            }),
            "recipe_run audit carries recipe_id and actor: {recipe_audit:?}"
        );
        let activations = audit_hits(&client, "ipc_recipe_activate").await;
        assert!(
            activations.iter().any(|row| {
                row.actor.as_deref() == Some("admin")
                    && row.actor.as_deref() != Some("ipc")
                    && row
                        .metadata_json
                        .as_deref()
                        .is_some_and(|meta| meta.contains("echo.true"))
            }),
            "recipe_activate audit carries recipe_id: {activations:?}"
        );
        assert!(audit_actions(&client, "ipc_shell_exec").await.is_empty());
        assert!(
            audit_actions(&client, "command_shell_start")
                .await
                .is_empty()
        );

        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}

#[test]
fn llm_can_activate_recipes_true_allows_mcp_actor() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("flag");
        let mut cfg = DaemonConfig::defaults_in(&data);
        cfg.policy.llm_can_activate_recipes = true;
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        assert!(state.policy.llm_can_activate_recipes());
        let handle = IpcServer::new(Arc::clone(&state), state.config.socket_path())
            .spawn()
            .unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));
        client
            .call(
                1,
                IpcRequest::RecipeUpsert(RecipeUpsertParams {
                    definition: recipe("echo.true", vec!["true".to_owned()], vec![]),
                }),
            )
            .await
            .unwrap();
        let activated = client
            .call(
                2,
                IpcRequest::RecipeActivate(RecipeActivateParams {
                    recipe_id: "echo.true".to_owned(),
                    version: None,
                    scope: Some(ActivationScope::Global),
                    from_mcp: true,
                }),
            )
            .await
            .unwrap();
        assert!(matches!(activated, IpcResponse::RecipeActivate(_)));
        let hits = audit_hits(&client, "ipc_recipe_activate").await;
        assert!(
            hits.iter().any(|row| row.actor.as_deref() == Some("mcp")),
            "flag-on MCP activate is actor mcp: {hits:?}"
        );
        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}

#[derive(Debug)]
struct AuditHit {
    action: String,
    actor: Option<String>,
    metadata_json: Option<String>,
}

async fn audit_hits(client: &DaemonClient, action: &str) -> Vec<AuditHit> {
    static ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(100);
    let id = ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let response = client
        .call(
            id,
            IpcRequest::AuditSince(AuditSinceParams {
                cursor: 0,
                action_filter: Some(action.to_owned()),
                decision_filter: None,
                limit: Some(50),
            }),
        )
        .await
        .unwrap();
    let IpcResponse::AuditSince(AuditSinceResponse { rows, .. }) = response else {
        panic!("audit: {response:?}");
    };
    rows.into_iter()
        .map(|row| AuditHit {
            action: row.action,
            actor: row.actor,
            metadata_json: row.metadata_json,
        })
        .collect()
}

async fn audit_actions(client: &DaemonClient, action: &str) -> Vec<String> {
    audit_hits(client, action)
        .await
        .into_iter()
        .map(|row| row.action)
        .collect()
}
