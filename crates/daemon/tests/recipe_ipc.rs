// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! IPC smoke for the recipe store: upsert, get, search, activate,
//! list_active, deactivate, list_versions, tombstone. No MCP tools.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{
    ActivationScope, RecipeDefinition, RecipeStatus, shell_interpreter_denied,
};
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcErrorCode, IpcRequest, IpcResponse, IpcServer,
    ListLimitParams, RecipeActivateParams, RecipeDeactivateParams, RecipeGetParams,
    RecipeImportSeedsParams, RecipeListVersionsParams, RecipeSearchParams, RecipeTombstoneParams,
    RecipeUpsertParams,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-ipc-recipe-{tag}-{}-{n}", std::process::id()));
    p
}

fn recipe(status: RecipeStatus) -> RecipeDefinition {
    RecipeDefinition {
        recipe_id: "git.status".to_owned(),
        version: 1,
        title: "Git status".to_owned(),
        summary: "Short working tree status".to_owned(),
        argv: vec!["git".to_owned(), "status".to_owned(), "--short".to_owned()],
        status,
        tags: vec!["git".to_owned(), "vcs".to_owned()],
        cwd: None,
        env_allowlist: vec![],
        timeout_ms: None,
        rule_pack_ids: vec![],
        placeholders: vec![],
    }
}

fn shell_argv() -> RecipeDefinition {
    let mut def = recipe(RecipeStatus::Draft);
    def.argv = vec!["bash".to_owned(), "-c".to_owned(), "echo hi".to_owned()];
    def
}

#[test]
#[allow(clippy::too_many_lines)] // one IPC lifecycle, same shape as registry_ipc
fn recipe_ipc_lifecycle_and_interpreter_deny() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("life");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let denied = client
            .call(
                1,
                IpcRequest::RecipeUpsert(RecipeUpsertParams {
                    definition: shell_argv(),
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(denied.code, IpcErrorCode::RecipeInvalid);
        assert!(denied.message.contains("shell interpreter"));

        let upsert = client
            .call(
                2,
                IpcRequest::RecipeUpsert(RecipeUpsertParams {
                    definition: recipe(RecipeStatus::Active),
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeUpsert(created) = upsert else {
            panic!("upsert: {upsert:?}");
        };
        assert_eq!(created.version, 1);

        let got = client
            .call(
                3,
                IpcRequest::RecipeGet(RecipeGetParams {
                    recipe_id: "git.status".to_owned(),
                    version: None,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeGet(body) = got else {
            panic!("get: {got:?}");
        };
        assert_eq!(body.definition.argv[0], "git");

        let found = client
            .call(
                4,
                IpcRequest::RecipeSearch(RecipeSearchParams {
                    query: "status".to_owned(),
                    limit: Some(10),
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeSearch(hits) = found else {
            panic!("search: {found:?}");
        };
        assert_eq!(hits.hits.len(), 1);

        let scope = ActivationScope::Global;
        let activated = client
            .call(
                5,
                IpcRequest::RecipeActivate(RecipeActivateParams {
                    recipe_id: "git.status".to_owned(),
                    version: None,
                    scope: Some(scope),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeActivate(act) = activated else {
            panic!("activate: {activated:?}");
        };
        assert!(!act.was_already_active);

        let listed = client
            .call(
                6,
                IpcRequest::RecipeListActive(terminal_commanderd::ListLimitParams { limit: None }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeListActive(active) = listed else {
            panic!("list: {listed:?}");
        };
        assert_eq!(active.entries.len(), 1);
        assert_eq!(active.entries[0].recipe_id, "git.status");

        let closed = client
            .call(
                7,
                IpcRequest::RecipeDeactivate(RecipeDeactivateParams {
                    recipe_id: "git.status".to_owned(),
                    version: 1,
                    scope: Some(scope),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeDeactivate(de) = closed else {
            panic!("deactivate: {closed:?}");
        };
        assert!(de.was_deactivated);

        let versions = client
            .call(
                8,
                IpcRequest::RecipeListVersions(RecipeListVersionsParams {
                    recipe_id: "git.status".to_owned(),
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeListVersions(listed_versions) = versions else {
            panic!("versions: {versions:?}");
        };
        assert_eq!(listed_versions.versions.len(), 1);

        let tombstoned = client
            .call(
                9,
                IpcRequest::RecipeTombstone(RecipeTombstoneParams {
                    recipe_id: "git.status".to_owned(),
                }),
            )
            .await
            .unwrap();
        assert!(matches!(tombstoned, IpcResponse::RecipeTombstone(_)));

        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}

#[test]
#[allow(clippy::too_many_lines)] // one import/activate lifecycle, same shape as the test above
fn recipe_seed_import_stays_tested_until_operator_activates() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("seeds");
        let cfg = DaemonConfig::defaults_in(&data);
        assert!(!cfg.policy.llm_can_activate_recipes);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        assert!(!state.policy.llm_can_activate_recipes());
        assert!(!state.policy.caps_allow_shell());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let denied = client
            .call(
                1,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: true,
                    scope: Some(ActivationScope::Global),
                    from_mcp: true,
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(denied.code, IpcErrorCode::PolicyDenied);
        assert!(denied.message.contains("recipe_activate_requires_admin"));

        let imported = client
            .call(
                2,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: false,
                    scope: None,
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeImportSeeds(report) = imported else {
            panic!("import: {imported:?}");
        };
        assert!(report.imported.len() >= 8, "{:?}", report.imported);
        assert_eq!(report.imported.len(), 8);
        assert!(report.activated.is_empty());
        assert!(report.failed.is_empty());
        assert!(!report.imported.iter().any(|id| id == "rg.files"));

        let missing_scope = client
            .call(
                3,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: true,
                    scope: None,
                    from_mcp: false,
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(missing_scope.code, IpcErrorCode::ScopeInvalid);

        for id in &report.imported {
            let got = client
                .call(
                    4,
                    IpcRequest::RecipeGet(RecipeGetParams {
                        recipe_id: id.clone(),
                        version: None,
                    }),
                )
                .await
                .unwrap();
            let IpcResponse::RecipeGet(body) = got else {
                panic!("get {id}: {got:?}");
            };
            assert_eq!(body.definition.status, RecipeStatus::Tested);
            assert!(!body.definition.argv.is_empty());
            assert!(
                shell_interpreter_denied(&body.definition.argv[0]).is_none(),
                "{id}"
            );
        }

        let activated = client
            .call(
                5,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: true,
                    scope: Some(ActivationScope::Global),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeImportSeeds(live) = activated else {
            panic!("activate import: {activated:?}");
        };
        assert_eq!(live.imported.len(), 8);
        assert_eq!(live.activated.len(), 8);
        assert!(live.failed.is_empty());

        let listed = client
            .call(
                6,
                IpcRequest::RecipeListActive(ListLimitParams { limit: None }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeListActive(active) = listed else {
            panic!("list: {listed:?}");
        };
        assert_eq!(active.entries.len(), 8);

        let again = client
            .call(
                7,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: true,
                    scope: Some(ActivationScope::Global),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeImportSeeds(second) = again else {
            panic!("reimport: {again:?}");
        };
        assert!(second.imported.is_empty());
        assert_eq!(second.skipped.len(), 8);
        assert_eq!(second.activated.len(), 8);

        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}
